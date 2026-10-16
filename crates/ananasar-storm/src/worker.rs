//! The per-connection worker threads: the request stream and connection churn.

use crate::config::Config;
use crate::fault::{FaultKind, malformed};
use crate::request::{Request, Verdict, generate, validate};
use crate::rng::Rng;
use crate::stats::{Live, Recorder, Stats};
use ananasar_net::read_frame;
use std::io::{self, ErrorKind, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What every worker needs.
pub(crate) struct Ctx<'a> {
    pub cfg: &'a Config,
    pub deadline: Instant,
    pub stop: &'a AtomicBool,
    pub live: &'a Live,
}

impl Ctx<'_> {
    fn running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed) && Instant::now() < self.deadline
    }

    /// Sleep for `d`; false if the run ended first.
    fn pause(&self, d: Duration) -> bool {
        self.wait(Instant::now().checked_add(d).unwrap_or(self.deadline))
    }

    /// Sleep until `until`; false if the run ended first.
    fn wait(&self, until: Instant) -> bool {
        loop {
            let now = Instant::now();
            if self.stop.load(Ordering::Relaxed) || now >= self.deadline {
                return false;
            }
            if now >= until {
                return true;
            }
            std::thread::sleep(until.saturating_duration_since(now).min(Duration::from_millis(20)));
        }
    }
}

fn micros(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Open a connection with Nagle off (so split segments really are separate)
/// and the configured reply timeout.
pub(crate) fn open(cfg: &Config) -> io::Result<TcpStream> {
    let stream = ananasar_net::connect(&cfg.host, cfg.port, false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(cfg.reply_timeout))?;
    stream.set_write_timeout(Some(cfg.reply_timeout))?;
    Ok(stream)
}

/// Count `n` requests as lost to `e`.
fn fail(rec: &mut Recorder<'_>, e: &io::Error, n: u64) {
    use crate::request::MismatchKind;
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => rec.timeout(n),
        ErrorKind::InvalidData => rec.mismatch(MismatchKind::Malformed, n),
        ErrorKind::UnexpectedEof
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::NotConnected => rec.reset(n),
        _ => rec.io_error(n),
    }
}

/// Send `reqs` back to back in one write (or in two, split at `split.0` bytes
/// with a pause of `split.1` between), then read and check one reply each.
/// Returns whether the connection can be used again.
pub(crate) fn exchange(
    stream: &mut TcpStream,
    reqs: &[Request],
    split: Option<(usize, Duration)>,
    rec: &mut Recorder<'_>,
) -> bool {
    let n = u64::try_from(reqs.len()).unwrap_or(u64::MAX);
    let bytes: Vec<u8> = reqs.iter().flat_map(|r| r.frame.iter().copied()).collect();
    rec.sent(n);
    let t0 = Instant::now();
    let written = match split {
        Some((at, delay)) => {
            let (a, b) = bytes.split_at(at.min(bytes.len()));
            stream.write_all(a).and_then(|()| stream.flush()).and_then(|()| {
                std::thread::sleep(delay);
                stream.write_all(b)
            })
        }
        None => stream.write_all(&bytes),
    };
    if let Err(e) = written {
        fail(rec, &e, n);
        return false;
    }
    for (i, req) in reqs.iter().enumerate() {
        match read_frame(stream) {
            Ok(reply) => match validate(req, &reply) {
                Verdict::Ok => rec.ok(micros(t0)),
                Verdict::Exception(code) => rec.exception(code, micros(t0)),
                Verdict::Mismatch(kind) => rec.mismatch(kind, 1),
            },
            Err(e) => {
                fail(rec, &e, n.saturating_sub(u64::try_from(i).unwrap_or(u64::MAX)));
                return false;
            }
        }
    }
    true
}

/// Send malformed `bytes` and note how the target reacts within `wait`.
fn inject(stream: &mut TcpStream, bytes: &[u8], wait: Duration, rec: &mut Recorder<'_>) {
    // A zero timeout is an error for sockets; the configuration keeps it above 0.
    stream.set_read_timeout(Some(wait.max(Duration::from_millis(1)))).ok();
    if stream.write_all(bytes).is_err() {
        rec.stats.fault_closed = rec.stats.fault_closed.saturating_add(1);
        return;
    }
    let slot = match read_frame(stream) {
        Ok(_) => &mut rec.stats.fault_replied,
        Err(e) if e.kind() == ErrorKind::InvalidData => &mut rec.stats.fault_replied,
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => &mut rec.stats.fault_silent,
        Err(_) => &mut rec.stats.fault_closed,
    };
    *slot = slot.saturating_add(1);
}

/// One connection sending a paced request stream, with faults.
pub(crate) fn run_requests(ctx: &Ctx<'_>, id: u64) -> Stats {
    let cfg = ctx.cfg;
    let mut rec = Recorder::new(ctx.live);
    let mut rng = Rng::derive(cfg.seed, id);
    let mut conn: Option<TcpStream> = None;
    let mut opened_before = false;
    let mut txn: u16 = 0;
    let started = Instant::now();
    let mut due = started;
    while ctx.running() {
        let rate = cfg.rate_at(started.elapsed());
        if rate > 0.0 {
            let interval = Duration::from_secs_f64(1.0 / rate.max(0.001));
            let now = Instant::now();
            if due > now {
                if !ctx.wait(due) {
                    break;
                }
            } else if now.saturating_duration_since(due) > interval {
                // Far behind: do not burst to catch up.
                due = now;
            }
            due = due.checked_add(interval).unwrap_or(due);
        }

        let mut stream = if let Some(s) = conn.take() {
            s
        } else {
            let t = Instant::now();
            match open(cfg) {
                Ok(s) => {
                    rec.connected(micros(t), opened_before);
                    opened_before = true;
                    s
                }
                Err(_) => {
                    rec.connect_error();
                    if !ctx.pause(Duration::from_millis(100)) {
                        break;
                    }
                    continue;
                }
            }
        };

        let fault = cfg.faults.pick(&mut rng);
        txn = txn.wrapping_add(1);
        let req = generate(&cfg.mix, &mut rng, txn, cfg.unit_id);
        let healthy = match fault {
            Some(kind) if kind.is_malformed() => {
                rec.fault(kind);
                let bytes = malformed(kind, &req, &mut rng);
                inject(&mut stream, &bytes, cfg.fault_wait, &mut rec);
                false // the stream state is unknown: start over
            }
            Some(FaultKind::Coalesced) => {
                rec.fault(FaultKind::Coalesced);
                let mut reqs = vec![req];
                for _ in 1..cfg.coalesce_count {
                    txn = txn.wrapping_add(1);
                    reqs.push(generate(&cfg.mix, &mut rng, txn, cfg.unit_id));
                }
                exchange(&mut stream, &reqs, None, &mut rec)
            }
            Some(FaultKind::Split) => {
                rec.fault(FaultKind::Split);
                let last = u16::try_from(req.frame.len().saturating_sub(1)).unwrap_or(1);
                let at = usize::from(rng.range_u16(1, last.max(1)));
                exchange(&mut stream, &[req], Some((at, cfg.split_delay)), &mut rec)
            }
            _ => exchange(&mut stream, &[req], None, &mut rec),
        };
        if healthy {
            conn = Some(stream);
        }
    }
    rec.stats
}

/// One thread of connection churn: connect, optionally send one request,
/// disconnect, repeat.
pub(crate) fn run_churn(ctx: &Ctx<'_>, id: u64) -> Stats {
    let cfg = ctx.cfg;
    let mut rec = Recorder::new(ctx.live);
    let mut rng = Rng::derive(cfg.seed, id);
    let mut txn: u16 = 0;
    let mut due = Instant::now();
    while ctx.running() {
        if !cfg.churn_interval.is_zero() {
            if !ctx.wait(due) {
                break;
            }
            due = Instant::now().checked_add(cfg.churn_interval).unwrap_or(due);
        }
        let t = Instant::now();
        match open(cfg) {
            Ok(mut stream) => {
                rec.connected(micros(t), false);
                if cfg.churn_request {
                    txn = txn.wrapping_add(1);
                    let req = generate(&cfg.mix, &mut rng, txn, cfg.unit_id);
                    exchange(&mut stream, &[req], None, &mut rec);
                }
                drop(stream);
                rec.cycle();
            }
            Err(_) => {
                rec.connect_error();
                // Do not spin on a target that refuses every connection.
                if !ctx.pause(Duration::from_millis(5)) {
                    break;
                }
            }
        }
    }
    rec.stats
}
