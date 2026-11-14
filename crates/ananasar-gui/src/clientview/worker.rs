//! Poll worker thread: owns the socket and the [`PollEngine`], driven by [`Cmd`]s from the UI.

use super::connection::partner_text;
use super::polling::{MAX_READ, MAX_WRITE, clamp_limit};
use crate::embed::SharedRegs;
use ananasar_core::client::{AddressBase, ManualCommand, PollEngine, PollList};
use ananasar_core::protocol::{self, Function};
use ananasar_core::sync::lock;
use ananasar_net::read_frame;
use eframe::egui;
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Cycle,
    WaitAfter,
    Immediate,
    Pause,
}

#[allow(dead_code)]
pub(super) enum Cmd {
    Connect { host: String, port: u16, uid: u8, keepalive: bool },
    Disconnect,
    SetMode(Mode),
    SetLists { rh: Vec<u16>, ri: Vec<u16>, w: Vec<u16> },
    SetLimits { read: u16, write: u16 },
    SetInterval(u64),
    SetBase(AddressBase),
    SetZeroTi(bool),
    Manual(ManualCommand),
    Quit,
}

pub(super) struct Shared {
    pub(super) log: Mutex<VecDeque<String>>,
    /// The GUI server's register file, shared with the main window.
    pub(super) regs: SharedRegs,
    pub(super) connected: AtomicBool,
    /// View > Values as Signed: read values are logged as signed 16-bit.
    pub(super) signed: AtomicBool,
    pub(super) status: Mutex<String>,
    /// Peer of the live connection, shown in the partner line.
    pub(super) partner: Mutex<Option<std::net::SocketAddr>>,
    /// Wakes the UI when the worker changes anything it shows.
    pub(super) ctx: egui::Context,
}

impl Shared {
    pub(super) fn set_status(&self, s: impl Into<String>) {
        *lock(&self.status) = s.into();
        self.ctx.request_repaint();
    }

    pub(super) fn log_line(&self, s: impl Into<String>) {
        {
            let mut log = lock(&self.log);
            log.push_back(s.into());
            while log.len() > 500 {
                log.pop_front();
            }
        }
        self.ctx.request_repaint();
    }
}

pub(super) fn spawn_worker(shared: Arc<Shared>, rx: Receiver<Cmd>) {
    std::thread::spawn(move || {
        let mut stream: Option<TcpStream> = None;
        let mut engine = PollEngine::new(1);
        let mut mode = Mode::Pause;
        let mut interval = Duration::from_millis(1000);
        let mut next_poll = Instant::now();
        loop {
            let active = matches!(mode, Mode::Cycle | Mode::WaitAfter | Mode::Immediate)
                && stream.is_some()
                && !engine.is_idle();
            // Sleep until a command arrives or, while polling, until the next
            // poll is due. An idle worker blocks here and uses no CPU.
            let received = if active {
                match rx.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                    Ok(cmd) => Some(cmd),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => return,
                }
            };

            if let Some(cmd) = received {
                match cmd {
                    Cmd::Quit => return,
                    Cmd::Connect { host, port, uid, keepalive } => {
                        let zero_ti = engine.force_zero_txn;
                        engine = PollEngine::new(uid);
                        engine.force_zero_txn = zero_ti;
                        match ananasar_net::connect(&host, port, keepalive) {
                            Ok(s) => {
                                s.set_read_timeout(Some(Duration::from_secs(3))).ok();
                                let peer = s.peer_addr().ok();
                                *lock(&shared.partner) = peer;
                                stream = Some(s);
                                shared.connected.store(true, Ordering::SeqCst);
                                shared.set_status("");
                                shared.log_line(format!("connected to {}", partner_text(peer)));
                                next_poll = Instant::now();
                            }
                            Err(e) => {
                                shared.set_status(format!("Connect failed: {e}"));
                                shared.log_line(format!("connect failed: {e}"));
                            }
                        }
                    }
                    Cmd::Disconnect => {
                        stream = None;
                        shared.connected.store(false, Ordering::SeqCst);
                        *lock(&shared.partner) = None;
                        shared.set_status("");
                        shared.log_line("disconnected");
                    }
                    Cmd::SetMode(m) => {
                        mode = m;
                        next_poll = Instant::now();
                        shared.log_line(format!("mode: {m:?}"));
                    }
                    Cmd::SetLists { rh, ri, w } => {
                        engine.read_holding = PollList::new(rh);
                        engine.read_input = PollList::new(ri);
                        engine.write = PollList::new(w);
                    }
                    Cmd::SetLimits { read, write } => {
                        engine.max_read = clamp_limit(read, MAX_READ);
                        engine.max_write = clamp_limit(write, MAX_WRITE);
                    }
                    Cmd::SetInterval(ms) => interval = Duration::from_millis(ms),
                    Cmd::SetBase(b) => engine.base = b,
                    Cmd::SetZeroTi(z) => engine.force_zero_txn = z,
                    Cmd::Manual(cmd) => {
                        if let Some(s) = stream.as_mut() {
                            let frame = engine.build_manual(cmd);
                            if s.write_all(&frame).is_ok()
                                && let Ok(resp) = read_frame(s)
                            {
                                log_resp(&shared, &resp);
                            }
                        }
                    }
                }
                continue;
            }

            // The wait ran out: a poll is due.
            let query = engine.next_query(&lock(&shared.regs));
            if let (Some(q), Some(s)) = (query, stream.as_mut())
                && poll_once(s, &q, &shared).is_err()
            {
                stream = None;
                shared.connected.store(false, Ordering::SeqCst);
                *lock(&shared.partner) = None;
                shared.set_status("");
                shared.log_line("connection lost");
            }
            let wait = match mode {
                Mode::Immediate => Duration::from_millis(1),
                _ => interval,
            };
            next_poll = Instant::now().checked_add(wait).unwrap_or_else(Instant::now);
        }
    });
}

fn poll_once(stream: &mut TcpStream, query: &[u8], shared: &Shared) -> std::io::Result<()> {
    let (func, start, count) = parse_request(query);
    stream.write_all(query)?;
    let resp = read_frame(stream)?;
    if let Ok((hdr, f, pdu)) = protocol::frame(&resp) {
        if f & 0x80 != 0 {
            shared.log_line(format!(
                "TI {}: exception FC 0x{:02X} code {}",
                hdr.transaction_id,
                f & 0x7F,
                pdu.first().copied().unwrap_or(0)
            ));
        } else if matches!(Function::from_u8(f), Some(Function::ReadHolding) | Some(Function::ReadInput)) {
            store_read(shared, func, start, pdu);
            let signed = shared.signed.load(Ordering::Relaxed);
            let shown: Vec<String> = pdu
                .get(1..)
                .unwrap_or(&[])
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| crate::value::fmt(u16::from_be_bytes(c), false, signed))
                .collect();
            shared.log_line(format!("TI {}: read @{start} x{count} = [{}]", hdr.transaction_id, shown.join(", ")));
        } else {
            shared.log_line(format!("TI {}: FC 0x{f:02X} ok", hdr.transaction_id));
        }
    }
    Ok(())
}

fn log_resp(shared: &Shared, resp: &[u8]) {
    if let Ok((hdr, f, pdu)) = protocol::frame(resp) {
        if f & 0x80 != 0 {
            shared.log_line(format!(
                "TI {}: exception FC 0x{:02X} code {}",
                hdr.transaction_id,
                f & 0x7F,
                pdu.first().copied().unwrap_or(0)
            ));
        } else {
            shared.log_line(format!("TI {}: manual FC 0x{f:02X} ok", hdr.transaction_id));
        }
    }
}

fn store_read(shared: &Shared, func: u8, start: u16, pdu: &[u8]) {
    if func != Function::ReadHolding as u8 || pdu.is_empty() {
        return; // only holding reads are stored
    }
    let mut regs = lock(&shared.regs);
    let words = pdu.get(1..).unwrap_or_default().as_chunks::<2>().0;
    for (addr, &c) in (start..=u16::MAX).zip(words) {
        regs.set_holding(addr, u16::from_be_bytes(c));
    }
}

fn parse_request(query: &[u8]) -> (u8, u16, u16) {
    match query.get(7..12) {
        Some(&[func, a0, a1, c0, c1]) => (func, u16::from_be_bytes([a0, a1]), u16::from_be_bytes([c0, c1])),
        _ => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientview::manual::build_command;
    use ananasar_core::registers::{Bank, RegisterFile};
    use ananasar_core::server::ServerConfig;
    use ananasar_net::{Server, Stats};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{Sender, channel};

    const DEADLINE: Duration = Duration::from_secs(5);

    /// Polls `cond` every few milliseconds until it holds or the deadline passes.
    fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        let start = Instant::now();
        while !cond() {
            assert!(start.elapsed() < DEADLINE, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A server from `ananasar-net` on an OS-assigned loopback port.
    struct TestServer {
        port: u16,
        regs: SharedRegs,
        stop: Arc<AtomicBool>,
    }

    impl TestServer {
        fn start() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
            let port = listener.local_addr().expect("local addr").port();
            let regs: SharedRegs = Arc::new(Mutex::new(RegisterFile::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let server = Server {
                regs: regs.clone(),
                cfg: Arc::new(Mutex::new(ServerConfig::default())),
                clients: Arc::new(AtomicUsize::new(0)),
                stop: stop.clone(),
                notify: Arc::new(|_| {}),
                log: None,
                stats: Arc::new(Mutex::new(Stats::default())),
            };
            std::thread::spawn(move || server.run(&listener));
            Self { port, regs, stop }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    struct Fixture {
        shared: Arc<Shared>,
        tx: Sender<Cmd>,
    }

    impl Fixture {
        fn new() -> Self {
            let shared = Arc::new(Shared {
                log: Mutex::new(VecDeque::new()),
                regs: Arc::new(Mutex::new(RegisterFile::new())),
                connected: AtomicBool::new(false),
                signed: AtomicBool::new(false),
                status: Mutex::new(String::new()),
                partner: Mutex::new(None),
                ctx: egui::Context::default(),
            });
            let (tx, rx) = channel();
            spawn_worker(shared.clone(), rx);
            Self { shared, tx }
        }

        fn send(&self, cmd: Cmd) {
            assert!(self.tx.send(cmd).is_ok(), "the worker thread is gone");
        }

        fn connect(&self, port: u16) {
            self.send(Cmd::Connect { host: "127.0.0.1".into(), port, uid: 1, keepalive: false });
        }

        fn connected(&self) -> bool {
            self.shared.connected.load(Ordering::SeqCst)
        }

        fn log_has(&self, needle: &str) -> bool {
            lock(&self.shared.log).iter().any(|l| l.contains(needle))
        }

        fn wait_log(&self, needle: &str) {
            wait_for(needle, || self.log_has(needle));
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.tx.send(Cmd::Quit).ok();
        }
    }

    #[test]
    fn connects_then_disconnects() {
        let server = TestServer::start();
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.wait_log("connected to ");
        assert!(lock(&w.shared.partner).is_some());
        assert!(lock(&w.shared.status).is_empty());

        w.send(Cmd::Disconnect);
        w.wait_log("disconnected");
        assert!(!w.connected());
        assert!(lock(&w.shared.partner).is_none());
    }

    #[test]
    fn connect_failure_sets_the_status() {
        // Bind and drop a listener to get a loopback port with nothing on it.
        let port = TcpListener::bind(("127.0.0.1", 0)).expect("bind").local_addr().expect("addr").port();
        let w = Fixture::new();
        w.connect(port);
        w.wait_log("connect failed:");
        assert!(lock(&w.shared.status).starts_with("Connect failed:"));
        assert!(!w.connected());
    }

    #[test]
    fn cyclic_read_stores_values_and_logs_them() {
        let server = TestServer::start();
        {
            let mut regs = lock(&server.regs);
            regs.set_holding(10, 111);
            regs.set_holding(11, 222);
            regs.set_holding(12, 333);
        }
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetLists { rh: vec![10, 11, 12], ri: vec![], w: vec![] });
        w.send(Cmd::SetMode(Mode::Cycle));
        wait_for("the values in the register file", || {
            let regs = lock(&w.shared.regs);
            [111, 222, 333].iter().zip(10u16..).all(|(&v, a)| regs.get(Bank::Holding, a) == v)
        });
        w.wait_log("read @10 x3 = [111, 222, 333]");
    }

    #[test]
    fn signed_values_are_logged_signed() {
        let server = TestServer::start();
        {
            let mut regs = lock(&server.regs);
            regs.set_holding(20, 65531);
            regs.set_holding(21, 7);
        }
        let w = Fixture::new();
        w.shared.signed.store(true, Ordering::Relaxed);
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetLists { rh: vec![20, 21], ri: vec![], w: vec![] });
        w.send(Cmd::SetMode(Mode::Cycle));
        w.wait_log("read @20 x2 = [-5, 7]");
        // The register file keeps the raw 16 bits.
        assert_eq!(lock(&w.shared.regs).get(Bank::Holding, 20), 65531);
    }

    #[test]
    fn immediate_and_wait_after_modes_poll_too() {
        let server = TestServer::start();
        lock(&server.regs).set_holding(3, 7);
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetBase(AddressBase::Zero));
        w.send(Cmd::SetZeroTi(true));
        w.send(Cmd::SetLimits { read: 5, write: 5 });
        w.send(Cmd::SetLists { rh: vec![3], ri: vec![], w: vec![] });
        for mode in [Mode::WaitAfter, Mode::Immediate] {
            lock(&w.shared.log).clear();
            lock(&w.shared.regs).set_holding(3, 0);
            w.send(Cmd::SetMode(mode));
            w.wait_log(&format!("mode: {mode:?}"));
            wait_for("a polled value", || lock(&w.shared.regs).get(Bank::Holding, 3) == 7);
        }
        w.send(Cmd::SetMode(Mode::Pause));
        w.wait_log("mode: Pause");
    }

    #[test]
    fn input_register_reads_are_logged_but_not_stored() {
        let server = TestServer::start();
        lock(&server.regs).set_input(4, 9);
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetLists { rh: vec![], ri: vec![4], w: vec![] });
        w.send(Cmd::SetMode(Mode::Cycle));
        w.wait_log("read @4 x1 = [9]");
        assert_eq!(lock(&w.shared.regs).get(Bank::Holding, 4), 0);
    }

    #[test]
    fn exception_replies_are_logged() {
        // A fake server that answers the first request with "illegal data address".
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let fake = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accept");
            // Fail instead of hanging when the worker never sends a request.
            s.set_read_timeout(Some(Duration::from_secs(5))).expect("read timeout");
            let req = read_frame(&mut s).expect("request");
            let reply = [req[0], req[1], 0, 0, 0, 3, 1, 0x83, 2];
            s.write_all(&reply).expect("reply");
            // Keep the socket open until the worker has read the reply.
            std::thread::sleep(Duration::from_millis(200));
        });
        let w = Fixture::new();
        w.connect(port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetLists { rh: vec![1], ri: vec![], w: vec![] });
        w.send(Cmd::SetMode(Mode::Cycle));
        w.wait_log("exception FC 0x03 code 2");
        fake.join().expect("fake server");
    }

    #[test]
    fn manual_single_write_uses_fc6() {
        let server = TestServer::start();
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        let cmd = build_command("5", "100", "", "", false, false).expect("valid command");
        w.send(Cmd::Manual(cmd));
        wait_for("the server register", || lock(&server.regs).get(Bank::Holding, 5) == 100);
        w.wait_log("manual FC 0x06 ok");
    }

    #[test]
    fn manual_single_write_can_use_fc16() {
        let server = TestServer::start();
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        let cmd = build_command("6", "200", "", "", false, true).expect("valid command");
        w.send(Cmd::Manual(cmd));
        wait_for("the server register", || lock(&server.regs).get(Bank::Holding, 6) == 200);
        w.wait_log("manual FC 0x10 ok");
    }

    #[test]
    fn manual_command_without_a_connection_is_ignored() {
        let w = Fixture::new();
        let cmd = build_command("5", "1", "", "", false, false).expect("valid command");
        w.send(Cmd::Manual(cmd));
        // A later command is handled, so the worker did not stall on the manual one.
        w.send(Cmd::Disconnect);
        w.wait_log("disconnected");
        assert!(!w.log_has("manual"));
    }

    #[test]
    fn a_dropped_connection_is_reported_as_lost() {
        // A listener that accepts one client and closes it at once.
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let accepter = std::thread::spawn(move || drop(listener.accept()));
        let w = Fixture::new();
        w.connect(port);
        wait_for("the connection", || w.connected());
        accepter.join().expect("accept thread");
        w.send(Cmd::SetInterval(10));
        w.send(Cmd::SetLists { rh: vec![0], ri: vec![], w: vec![] });
        w.send(Cmd::SetMode(Mode::Cycle));
        w.wait_log("connection lost");
        wait_for("the connected flag to clear", || !w.connected());
        assert!(lock(&w.shared.partner).is_none());
    }

    #[test]
    fn quit_ends_the_thread() {
        let w = Fixture::new();
        w.send(Cmd::Quit);
        // The thread owned the receiver, so once it returns, sending fails.
        wait_for("the worker to end", || w.tx.send(Cmd::Disconnect).is_err());
    }

    #[test]
    fn log_keeps_the_last_500_lines() {
        let w = Fixture::new();
        for i in 0..520 {
            w.shared.log_line(format!("line {i}"));
        }
        let log = lock(&w.shared.log);
        assert_eq!(log.len(), 500);
        assert_eq!(log.front().map(String::as_str), Some("line 20"));
    }

    #[test]
    fn log_cap_is_exactly_500_lines() {
        let w = Fixture::new();
        // Run on a thread with a deadline: a broken cap loops instead of failing.
        let (done_tx, done_rx) = channel();
        let shared = w.shared.clone();
        std::thread::spawn(move || {
            for i in 0..500 {
                shared.log_line(format!("line {i}"));
            }
            let state = || {
                let log = lock(&shared.log);
                (log.len(), log.front().cloned())
            };
            let at_cap = state();
            shared.log_line("line 500");
            let over = state();
            done_tx.send((at_cap, over)).ok();
        });
        let (at_cap, over) = done_rx.recv_timeout(DEADLINE).expect("the log cap loop ended");
        assert_eq!(at_cap, (500, Some("line 0".to_string())), "500 lines drop nothing");
        assert_eq!(over, (500, Some("line 1".to_string())), "the 501st line drops the oldest");
    }

    #[test]
    fn immediate_mode_polls_without_waiting_for_the_interval() {
        let server = TestServer::start();
        let w = Fixture::new();
        w.connect(server.port);
        wait_for("the connection", || w.connected());
        w.send(Cmd::SetInterval(60_000));
        w.send(Cmd::SetLists { rh: vec![1], ri: vec![], w: vec![] });
        w.send(Cmd::SetMode(Mode::Immediate));
        let polls = || lock(&w.shared.log).iter().filter(|l| l.contains("read @")).count();
        wait_for("repeated polls despite the long interval", || polls() >= 5);
    }

    #[test]
    fn parse_request_needs_a_full_header() {
        assert_eq!(parse_request(&[]), (0, 0, 0));
        assert_eq!(parse_request(&[0; 11]), (0, 0, 0));
        let q = [0, 1, 0, 0, 0, 6, 1, 3, 0x01, 0x02, 0x00, 0x04];
        assert_eq!(parse_request(&q), (3, 0x0102, 4));
    }

    #[test]
    fn store_read_survives_a_short_pdu() {
        let w = Fixture::new();
        // Byte count claims 10 bytes; none follow.
        store_read(&w.shared, Function::ReadHolding as u8, 0, &[10]);
        // An empty PDU and a half register.
        store_read(&w.shared, Function::ReadHolding as u8, 0, &[]);
        store_read(&w.shared, Function::ReadHolding as u8, 0, &[2, 0x12]);
        assert_eq!(lock(&w.shared.regs).get(Bank::Holding, 0), 0);
        // The last address does not overflow.
        store_read(&w.shared, Function::ReadHolding as u8, u16::MAX, &[4, 0, 1, 0, 2]);
        assert_eq!(lock(&w.shared.regs).get(Bank::Holding, u16::MAX), 1);
        // Input reads are not stored.
        store_read(&w.shared, Function::ReadInput as u8, 1, &[2, 0, 5]);
        assert_eq!(lock(&w.shared.regs).get(Bank::Holding, 1), 0);
    }
}
