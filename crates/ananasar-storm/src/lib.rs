//! Modbus/TCP storm generator for load and robustness checks.
//!
//! Drives a target with N parallel connections at a controlled request rate
//! (fixed or ramped), a configurable function mix, optional fault injection
//! (malformed frames, coalesced and split requests) and connection churn,
//! and measures latency, errors and whether replies match their requests.
//!
//! It is for equipment you own or are authorised to test, and it works at
//! the Modbus/TCP level only: it is not a link-level storm generator.
//! Threads over blocking `std::net` sockets, no async runtime.
#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, reason = "test code: a panic is a failed test")
)]

pub mod config;
pub mod fault;
pub mod hist;
pub mod report;
pub mod request;
pub mod rng;
pub mod stats;
mod worker;

pub use config::{Config, Faults, Mix};
pub use fault::FaultKind;
pub use report::{Report, Tick};
pub use stats::Stats;

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use worker::{Ctx, open, run_churn, run_requests};

/// The tool version, from the workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The notice every run prints before it starts.
pub const SAFETY_NOTICE: &str =
    "Use this only against equipment you own or are authorised to test: storm traffic can disturb or disable devices.";

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// A seed for runs that do not give one: the clock, so it differs per run
/// but is printed and recorded for reproduction.
pub fn fresh_seed() -> u64 {
    unix_ms() ^ u64::from(std::process::id()).rotate_left(32)
}

fn load(c: &AtomicU64) -> u64 {
    c.load(Ordering::Relaxed)
}

/// Run the storm described by `cfg` until its duration ends or `stop` is
/// set. `on_tick` is called about once a second with that second's counters.
///
/// The configuration is checked first; an error means nothing was sent.
pub fn run(cfg: &Config, stop: &AtomicBool, on_tick: &mut dyn FnMut(&Tick)) -> Result<Report, String> {
    cfg.check()?;
    let live = stats::Live::default();
    let started_unix_ms = unix_ms();
    let started = Instant::now();
    let deadline = started.checked_add(cfg.duration).unwrap_or(started);
    let ctx = Ctx { cfg, deadline, stop, live: &live };

    // Idle connections held for the whole run.
    let mut held: Vec<TcpStream> = Vec::new();
    let mut half_open_failed = 0u64;
    for _ in 0..cfg.half_open {
        match open(cfg) {
            Ok(s) => held.push(s),
            Err(_) => half_open_failed = half_open_failed.saturating_add(1),
        }
    }

    let mut timeline: Vec<Tick> = Vec::new();
    let mut merged = Stats::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..cfg.connections as u64)
            .map(|id| {
                let ctx = &ctx;
                scope.spawn(move || if ctx.cfg.churn { run_churn(ctx, id) } else { run_requests(ctx, id) })
            })
            .collect();

        let mut last = stats_snapshot(&live);
        let mut last_at = started;
        let mut next = started.checked_add(Duration::from_secs(1)).unwrap_or(deadline);
        loop {
            let now = Instant::now();
            if stop.load(Ordering::Relaxed) || now >= deadline {
                break;
            }
            if now < next {
                std::thread::sleep(next.saturating_duration_since(now).min(Duration::from_millis(20)));
                continue;
            }
            let tick = make_tick(&live, &mut last, timeline.len(), now.saturating_duration_since(started));
            on_tick(&tick);
            timeline.push(tick);
            last_at = now;
            next = next.checked_add(Duration::from_secs(1)).unwrap_or(deadline);
        }
        // Workers stop at the deadline (or on `stop`) after at most one reply timeout.
        for h in handles {
            match h.join() {
                Ok(s) => merged.merge(&s),
                Err(_) => merged.worker_panics = merged.worker_panics.saturating_add(1),
            }
        }
        let end = Instant::now();
        if end.saturating_duration_since(last_at) >= Duration::from_millis(250) {
            let tick = make_tick(&live, &mut last, timeline.len(), end.saturating_duration_since(started));
            on_tick(&tick);
            timeline.push(tick);
        }
    });

    merged.half_open_held = u64::try_from(held.len()).unwrap_or(u64::MAX);
    merged.half_open_failed = half_open_failed;
    drop(held);
    Ok(Report {
        config: cfg.clone(),
        version: VERSION,
        started_unix_ms,
        elapsed: started.elapsed().min(cfg.duration.saturating_add(cfg.reply_timeout)),
        stats: merged,
        timeline,
    })
}

#[derive(Clone, Copy, Default)]
struct Snapshot {
    sent: u64,
    replies: u64,
    exceptions: u64,
    timeouts: u64,
    errors: u64,
    mismatches: u64,
    faults: u64,
    cycles: u64,
    latency_sum_us: u64,
}

fn stats_snapshot(l: &stats::Live) -> Snapshot {
    Snapshot {
        sent: load(&l.sent),
        replies: load(&l.replies),
        exceptions: load(&l.exceptions),
        timeouts: load(&l.timeouts),
        errors: load(&l.errors),
        mismatches: load(&l.mismatches),
        faults: load(&l.faults),
        cycles: load(&l.cycles),
        latency_sum_us: load(&l.latency_sum_us),
    }
}

fn make_tick(live: &stats::Live, last: &mut Snapshot, done: usize, elapsed: Duration) -> Tick {
    let now = stats_snapshot(live);
    let d = |a: u64, b: u64| a.saturating_sub(b);
    let replies = d(now.replies, last.replies);
    let tick = Tick {
        index: u64::try_from(done).unwrap_or(u64::MAX).saturating_add(1),
        unix_ms: unix_ms(),
        elapsed_s: elapsed.as_secs_f64(),
        sent: d(now.sent, last.sent),
        replies,
        exceptions: d(now.exceptions, last.exceptions),
        timeouts: d(now.timeouts, last.timeouts),
        errors: d(now.errors, last.errors),
        mismatches: d(now.mismatches, last.mismatches),
        faults: d(now.faults, last.faults),
        cycles: d(now.cycles, last.cycles),
        mean_latency_us: d(now.latency_sum_us, last.latency_sum_us).checked_div(replies),
    };
    *last = now;
    tick
}
