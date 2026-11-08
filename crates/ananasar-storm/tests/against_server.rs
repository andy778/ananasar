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
    assert!(r.stats.churn_cycles >= 10, "{}", r.stats.churn_cycles);
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
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(r.stats.connect_errors > 0);
    assert_eq!((r.stats.connects, r.stats.churn_cycles, r.stats.sent), (0, 0, 0));
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
