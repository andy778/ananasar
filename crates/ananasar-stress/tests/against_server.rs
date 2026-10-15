//! End-to-end runs against our own server on a loopback port.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::registers::RegisterFile;
use ananasar_core::protocol::Exception;
use ananasar_core::server::ServerConfig;
use ananasar_net::Server;
use ananasar_stress::{Config, FaultKind, Report, run};
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

    let dir = std::env::temp_dir().join(format!("ananasar-stress-it-{}", std::process::id()));
    let files = r.write_to(&dir).unwrap();
    let json = std::fs::read_to_string(&files[0]).unwrap();
    assert!(json.contains("\"seed\": 1234") && json.contains(&format!("\"port\": {}", srv.port)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exceptions_are_counted_by_code_and_are_not_mismatches() {
    let srv = TestServer::start_with(ServerConfig { forced_exception: Some(Exception::SlaveDeviceBusy), ..ServerConfig::default() });
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
    let malformed: u64 = FaultKind::ALL.iter().filter(|k| k.is_malformed()).map(|k| s.faults.get(k).copied().unwrap_or(0)).sum();
    assert_eq!(malformed, s.fault_replied + s.fault_closed + s.fault_silent);
    assert!(s.reconnects >= malformed.saturating_sub(1), "reconnects {} for {malformed} malformed frames", s.reconnects);
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
