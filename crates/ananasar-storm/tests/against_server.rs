//! End-to-end runs against our own server on a loopback port.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::protocol::Exception;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::Server;
use ananasar_storm::{Config, FaultKind, Report, run};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct TestServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        Self::start_with(ServerConfig::default())
    }

    fn start_with(config: ServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let server = Server {
            regs: Arc::new(Mutex::new(RegisterFile::new())),
            cfg: Arc::new(Mutex::new(config)),
            clients: Arc::new(AtomicUsize::new(0)),
            stop: stop.clone(),
            notify: Arc::new(|_| {}),
            log: None,
            stats: Arc::default(),
        };
        let thread = std::thread::spawn(move || server.run(&listener));
        Self { port, stop, thread: Some(thread) }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

fn base(port: u16) -> Config {
    let mut c = Config::new("127.0.0.1", port);
    c.duration = Duration::from_secs(1);
    c.seed = 1234;
    c
}

fn go(cfg: &Config) -> Report {
    run(cfg, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

#[test]
fn clean_run_has_consistent_counts_and_no_mismatches() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.connections = 2; // the server accepts two clients
    cfg.rate = 200.0;
    cfg.mix.weights = [30, 20, 10, 20, 10, 10]; // every function code, writes included
    cfg.mix.qty_max = 20;
    let r = go(&cfg);
    let s = &r.stats;
    assert!(s.sent >= 100, "sent {}", s.sent);
    assert_eq!(s.sent, s.accounted(), "every request must have one outcome");
    assert_eq!(s.mismatch_total(), 0, "{:?}", s.mismatches);
    assert_eq!((s.timeouts, s.resets, s.io_errors, s.connect_errors), (0, 0, 0, 0));
    assert_eq!(s.ok, s.sent);
    assert_eq!(s.latency.count(), s.ok);
    assert_eq!(s.fault_total(), 0);
    assert!(s.latency.percentile(99.0) >= s.latency.percentile(50.0));
    assert!(!r.timeline.is_empty());
    // About the configured rate: 2 x 200/s for one second, with pacing slack.
    assert!(r.achieved_rate() > 150.0 && r.achieved_rate() < 450.0, "{}", r.achieved_rate());

    let dir = std::env::temp_dir().join(format!("ananasar-storm-it-{}", std::process::id()));
    let files = r.write_to(&dir).unwrap();
    let json = std::fs::read_to_string(&files[0]).unwrap();
    assert!(json.contains("\"seed\": 1234") && json.contains(&format!("\"port\": {}", srv.port)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exceptions_are_counted_by_code_and_are_not_mismatches() {
    let srv = TestServer::start_with(ServerConfig {
        forced_exception: Some(Exception::SlaveDeviceBusy),
        ..ServerConfig::default()
    });
    let mut cfg = base(srv.port);
    cfg.connections = 1;
    cfg.rate = 100.0;
    let r = go(&cfg);
    let s = &r.stats;
    assert!(s.sent > 50);
    assert_eq!(s.exceptions.get(&6), Some(&s.sent), "{:?}", s.exceptions);
    assert_eq!((s.ok, s.mismatch_total(), s.timeouts), (0, 0, 0));
    assert_eq!(s.sent, s.accounted());
}

#[test]
fn fault_injection_keeps_the_books_straight() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.connections = 1;
    cfg.rate = 100.0;
    cfg.authorised = true;
    cfg.fault_wait = Duration::from_millis(50);
    cfg.split_delay = Duration::from_millis(5);
    for kind in FaultKind::ALL {
        cfg.faults.set(kind, 10.0);
    }
    let r = go(&cfg);
    let s = &r.stats;
    assert!(s.fault_total() >= 5, "faults {}", s.fault_total());
    assert_eq!(s.sent, s.accounted());
    assert_eq!(s.mismatch_total(), 0, "our server must not send wrong replies: {:?}", s.mismatches);
    // Each malformed frame ends in one observed reaction.
    let malformed: u64 =
        FaultKind::ALL.iter().filter(|k| k.is_malformed()).map(|k| s.faults.get(k).copied().unwrap_or(0)).sum();
    assert_eq!(malformed, s.fault_replied + s.fault_closed + s.fault_silent);
    assert!(
        s.reconnects >= malformed.saturating_sub(1),
        "reconnects {} for {malformed} malformed frames",
        s.reconnects
    );
}

#[test]
fn churn_with_an_idle_connection_held_open() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.connections = 1;
    cfg.churn = true;
    cfg.churn_interval = Duration::from_millis(25);
    cfg.half_open = 1;
    let r = go(&cfg);
    // Ideally about 40 cycles; a loaded runner (macOS, with the other tests running) managed 8.
    // The floor is a tenth of the expected count. The ceiling cannot be
    // exceeded on a slow runner (a late wake-up only lowers the count), so it
    // is tight: one cycle per interval plus the first, which is not waited for.
    let expected = cfg.duration.as_millis() / cfg.churn_interval.as_millis();
    let cycles = u128::from(r.stats.churn_cycles);
    assert!(cycles >= expected / 10, "{cycles} cycles, expected about {expected}");
    assert!(cycles <= expected + 1 + expected / 4, "{cycles} cycles, paced at {expected}");
    assert_eq!(r.stats.half_open_held, 1);
    assert_eq!(r.stats.connect_errors, 0);
    assert_eq!(r.stats.sent, 0);
    assert_eq!(r.stats.connect_latency.count(), r.stats.connects);
}

#[test]
fn unauthorised_risky_runs_are_refused_before_any_traffic() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.faults.set(FaultKind::BadLength, 1.0);
    let err = run(&cfg, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
    assert!(err.contains("--i-am-authorised"), "{err}");
}

#[test]
fn stop_flag_ends_the_run_early() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.duration = Duration::from_secs(30);
    cfg.connections = 1;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        flag.store(true, Ordering::SeqCst);
    });
    let r = run(&cfg, &stop, &mut |_| {}).unwrap();
    assert!(r.elapsed < Duration::from_secs(5), "{:?}", r.elapsed);
    assert_eq!(r.stats.sent, r.stats.accounted());
}

#[test]
fn a_dead_port_counts_connect_errors() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut cfg = base(port);
    cfg.connections = 1;
    let r = go(&cfg);
    assert!(r.stats.connect_errors > 0);
    assert_eq!(r.stats.sent, 0);
    assert_retries_are_paced(r.stats.connect_errors, 100);
}

/// A refused-port run of one second retried about once per `pause_ms`, no
/// less than a few times and no more than the pause allows.
///
/// The floor is 1 on Windows, where one refused connect takes about a second;
/// elsewhere a loop that gives up after its first error fails it. The ceiling
/// holds on any runner (late wake-ups only lower the count) and fails a loop
/// that does not pause, which would count thousands.
fn assert_retries_are_paced(connect_errors: u64, pause_ms: u64) {
    let expected = 1000 / pause_ms;
    let floor = if cfg!(windows) { 1 } else { 3.min(expected) };
    assert!(connect_errors >= floor, "{connect_errors} connect errors, expected about {expected}");
    assert!(connect_errors <= 2 * expected + 5, "{connect_errors} connect errors, paced at {expected}");
}

#[test]
fn progress_carries_live_latency_percentiles() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.connections = 1;
    cfg.rate = 100.0;
    cfg.duration = Duration::from_secs(2);
    let mut seen = Vec::new();
    let r = ananasar_storm::run_with(&cfg, &AtomicBool::new(false), &mut |p| {
        seen.push((p.tick.index, p.latency.count(), p.latency.percentile(95.0)));
    })
    .unwrap();
    assert!(!seen.is_empty());
    let last = seen.last().unwrap();
    assert!(last.1 > 50 && last.2.is_some(), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0].1 <= w[1].1), "the histogram only grows: {seen:?}");
    assert!(last.1 <= r.stats.latency.count());
}

/// A loopback peer that misbehaves on purpose: it accepts every connection
/// and hands it to `handle`, until dropped.
struct BadPeer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl BadPeer {
    fn start(handle: impl Fn(std::net::TcpStream) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        s.set_nonblocking(false).unwrap();
                        handle(s);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { port, stop, thread: Some(thread) }
    }
}

impl Drop for BadPeer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

fn short(port: u16) -> Config {
    let mut c = base(port);
    c.duration = Duration::from_secs(1); // the shortest the configuration allows
    c.reply_timeout = Duration::from_millis(100);
    c.connections = 1;
    c.rate = 50.0;
    c
}

#[test]
fn churn_against_a_refused_port_counts_connect_errors_and_ends_on_time() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut cfg = short(port);
    cfg.churn = true;
    cfg.churn_interval = Duration::ZERO;
    cfg.authorised = true; // an interval of zero is an unlimited rate
    let t = std::time::Instant::now();
    let r = go(&cfg);
    // Windows takes about a second to give up on one refused connect, so allow
    // one such attempt to straddle the deadline; a spinning loop would not end.
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(r.stats.connect_errors > 0);
    assert_eq!((r.stats.connects, r.stats.churn_cycles, r.stats.sent), (0, 0, 0));
    assert_retries_are_paced(r.stats.connect_errors, 5);
}

#[test]
fn a_peer_that_closes_at_once_counts_resets() {
    let peer = BadPeer::start(drop);
    let r = go(&short(peer.port));
    let s = &r.stats;
    assert!(s.resets > 0, "{s:?}");
    assert_eq!((s.timeouts, s.io_errors, s.ok), (0, 0, 0));
    assert_eq!(s.sent, s.accounted());
}

#[test]
fn a_split_request_to_a_closing_peer_counts_resets() {
    let peer = BadPeer::start(|mut s| {
        // Take the first segment, then hang up before the second arrives.
        let mut b = [0u8; 16];
        let _ = s.read(&mut b).unwrap_or(0);
    });
    let mut cfg = short(peer.port);
    cfg.authorised = true;
    cfg.faults.set(FaultKind::Split, 100.0);
    cfg.split_delay = Duration::from_millis(30);
    let s = go(&cfg).stats;
    assert!(s.resets > 0, "{s:?}");
    assert_eq!((s.timeouts, s.io_errors, s.ok), (0, 0, 0));
    assert_eq!(s.sent, s.accounted());
}

#[test]
fn a_silent_peer_counts_timeouts() {
    let held = Arc::new(Mutex::new(Vec::new()));
    let keep = held.clone();
    let peer = BadPeer::start(move |s| keep.lock().unwrap().push(s));
    let s = go(&short(peer.port)).stats;
    assert!(s.timeouts > 0, "{s:?}");
    assert_eq!((s.resets, s.io_errors, s.ok), (0, 0, 0));
    assert_eq!(s.sent, s.accounted());
}

#[test]
fn a_malformed_reply_counts_as_a_malformed_mismatch() {
    let peer = BadPeer::start(|mut s| {
        let mut b = [0u8; 64];
        if s.read(&mut b).unwrap_or(0) > 0 {
            // A declared length of 0 cannot hold a unit id and a function code.
            s.write_all(&[0, 1, 0, 0, 0, 0, 1, 3]).ok();
        }
    });
    let s = go(&short(peer.port)).stats;
    let malformed = s.mismatches.get(&ananasar_storm::request::MismatchKind::Malformed).copied().unwrap_or(0);
    assert!(malformed > 0, "{s:?}");
    assert_eq!((s.timeouts, s.ok), (0, 0));
    assert_eq!(s.sent, s.accounted());
}

/// Run `cfg` with a stop request 300 ms in; how long it took.
fn elapsed_with_stop_during_pause(cfg: &Config) -> Duration {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let t = std::time::Instant::now();
    run(cfg, &stop, &mut |_| {}).unwrap();
    t.elapsed()
}

#[test]
fn a_stop_request_ends_a_request_worker_that_is_waiting_for_its_slot() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.duration = Duration::from_secs(60);
    cfg.connections = 1;
    cfg.rate = 0.1; // the second request is due ten seconds in
    // Waiting out the slot would take about 10 s; the margin is for slow runners.
    let took = elapsed_with_stop_during_pause(&cfg);
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_stop_request_ends_a_churn_worker_that_is_waiting_for_its_slot() {
    let srv = TestServer::start();
    let mut cfg = base(srv.port);
    cfg.duration = Duration::from_secs(60);
    cfg.connections = 1;
    cfg.churn = true;
    cfg.churn_interval = Duration::from_secs(10);
    let took = elapsed_with_stop_during_pause(&cfg);
    assert!(took < Duration::from_secs(5), "{took:?}");
}

/// Run one second of `kind` at 100 % against `port`.
fn fault_run(port: u16, kind: FaultKind) -> ananasar_storm::Stats {
    let mut cfg = short(port);
    cfg.authorised = true;
    cfg.fault_wait = Duration::from_millis(500);
    cfg.faults.set(kind, 100.0);
    go(&cfg).stats
}

/// The counters a malformed-frame run of `kind` should leave: `n` faults of
/// that kind, all in one reaction bucket, and no ordinary requests.
fn assert_reaction(s: &ananasar_storm::Stats, kind: FaultKind, replied: bool, closed: bool, silent: bool) {
    let n = s.faults.get(&kind).copied().unwrap_or(0);
    assert!(n > 0, "{s:?}");
    assert_eq!(s.sent, 0, "a malformed frame is not a counted request: {s:?}");
    let want = |on: bool| if on { n } else { 0 };
    assert_eq!(
        (s.fault_replied, s.fault_closed, s.fault_silent),
        (want(replied), want(closed), want(silent)),
        "{n} faults: {s:?}"
    );
}

#[test]
fn a_malformed_frame_answered_with_a_frame_counts_as_replied() {
    let peer = BadPeer::start(|mut s| {
        let mut b = [0u8; 64];
        if s.read(&mut b).unwrap_or(0) > 0 {
            // An exception reply: unit 1, function 0x83, code 2.
            s.write_all(&[0, 1, 0, 0, 0, 3, 1, 0x83, 2]).ok();
        }
    });
    let s = fault_run(peer.port, FaultKind::BadLength);
    assert_reaction(&s, FaultKind::BadLength, true, false, false);
}

#[test]
fn a_malformed_frame_answered_with_garbage_counts_as_replied() {
    let peer = BadPeer::start(|mut s| {
        let mut b = [0u8; 64];
        if s.read(&mut b).unwrap_or(0) > 0 {
            // A declared length of 0 is not a frame (an `InvalidData` read).
            s.write_all(&[0, 1, 0, 0, 0, 0, 1, 3]).ok();
        }
    });
    let s = fault_run(peer.port, FaultKind::BadLength);
    assert_reaction(&s, FaultKind::BadLength, true, false, false);
}

#[test]
fn a_malformed_frame_that_gets_a_hang_up_counts_as_closed() {
    let peer = BadPeer::start(drop);
    let s = fault_run(peer.port, FaultKind::BadProtocol);
    assert_reaction(&s, FaultKind::BadProtocol, false, true, false);
}

#[test]
fn a_malformed_frame_that_gets_silence_counts_as_silent() {
    let held = Arc::new(Mutex::new(Vec::new()));
    let keep = held.clone();
    let peer = BadPeer::start(move |s| keep.lock().unwrap().push(s));
    let s = fault_run(peer.port, FaultKind::TruncatedPdu);
    assert_reaction(&s, FaultKind::TruncatedPdu, false, false, true);
}

/// A peer that records what each connection sent, as one entry per `read`
/// and without ever replying, so the client times out and reconnects.
struct Recording {
    reads: Arc<Mutex<Vec<Vec<Vec<u8>>>>>,
    done: Arc<AtomicBool>,
    peer: BadPeer,
}

impl Recording {
    fn start() -> Self {
        let reads: Arc<Mutex<Vec<Vec<Vec<u8>>>>> = Arc::default();
        let done = Arc::new(AtomicBool::new(false));
        let (log, stop) = (reads.clone(), done.clone());
        let peer = BadPeer::start(move |mut s| {
            let (log, stop) = (log.clone(), stop.clone());
            let index = {
                let mut all = log.lock().unwrap();
                all.push(Vec::new());
                all.len() - 1
            };
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_millis(100))).ok();
                let mut buf = [0u8; 512];
                while !stop.load(Ordering::SeqCst) {
                    match s.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => log.lock().unwrap()[index].push(buf[..n].to_vec()),
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        }
                        Err(_) => break,
                    }
                }
            });
        });
        Self { reads, done, peer }
    }

    /// The bytes each connection that sent anything received, per read.
    fn connections(&self) -> Vec<Vec<Vec<u8>>> {
        self.reads.lock().unwrap().iter().filter(|c| !c.is_empty()).cloned().collect()
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

/// The lengths of the MBAP frames laid back to back in `bytes`.
fn frame_lengths(mut bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    while bytes.len() >= 6 {
        let len = 6 + usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        out.push(len.min(bytes.len()));
        bytes = &bytes[len.min(bytes.len())..];
    }
    out
}

#[test]
fn a_coalesced_fault_sends_several_requests_in_one_go() {
    let rec = Recording::start();
    let mut cfg = short(rec.peer.port);
    cfg.authorised = true;
    cfg.coalesce_count = 3;
    cfg.faults.set(FaultKind::Coalesced, 100.0);
    let s = go(&cfg).stats;
    assert!(s.faults.get(&FaultKind::Coalesced).copied().unwrap_or(0) > 0, "{s:?}");
    let conns = rec.connections();
    assert!(!conns.is_empty());
    for c in &conns {
        // A plain request would put one frame on the wire.
        let all: Vec<u8> = c.concat();
        assert_eq!(frame_lengths(&all).len(), 3, "{c:?}");
    }
    assert_eq!(s.sent, 3 * u64::try_from(conns.len()).unwrap());
}

#[test]
fn a_split_fault_arrives_in_more_than_one_read() {
    let rec = Recording::start();
    let mut cfg = short(rec.peer.port);
    cfg.authorised = true;
    cfg.reply_timeout = Duration::from_millis(600);
    cfg.split_delay = Duration::from_millis(200);
    cfg.faults.set(FaultKind::Split, 100.0);
    let s = go(&cfg).stats;
    assert!(s.faults.get(&FaultKind::Split).copied().unwrap_or(0) > 0, "{s:?}");
    let conns = rec.connections();
    assert!(!conns.is_empty());
    for c in &conns {
        // Still exactly one request, whole once both parts are in.
        assert_eq!(frame_lengths(&c.concat()).len(), 1, "{c:?}");
    }
    // The delay is far longer than a read takes, so at least one connection
    // shows the two parts apart; a plain request is a single read.
    assert!(conns.iter().any(|c| c.len() >= 2), "{conns:?}");
}
