//! Poll worker thread: owns the socket and the [`PollEngine`], driven by [`Cmd`]s from the UI.

use super::connection::partner_text;
use super::polling::{clamp_limit, MAX_READ, MAX_WRITE};
use ananasar_core::client::{AddressBase, ManualCommand, PollEngine, PollList};
use ananasar_core::protocol::{self, Function};
use ananasar_core::registers::RegisterFile;
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
    pub(super) regs: Mutex<RegisterFile>,
    pub(super) connected: AtomicBool,
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
            next_poll = Instant::now() + wait;
        }
    });
}

fn poll_once(stream: &mut TcpStream, query: &[u8], shared: &Shared) -> std::io::Result<()> {
    let (func, start, count) = parse_request(query);
    stream.write_all(query)?;
    let resp = read_frame(stream)?;
    if let Ok((hdr, f, pdu)) = protocol::frame(&resp) {
        if f & 0x80 != 0 {
            shared.log_line(format!("TI {}: exception FC 0x{:02X} code {}", hdr.transaction_id, f & 0x7F, pdu.first().copied().unwrap_or(0)));
        } else if matches!(Function::from_u8(f), Some(Function::ReadHolding) | Some(Function::ReadInput)) {
            store_read(shared, func, start, pdu);
            let shown: Vec<String> = pdu.get(1..).unwrap_or(&[]).chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]]).to_string()).collect();
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
            shared.log_line(format!("TI {}: exception FC 0x{:02X} code {}", hdr.transaction_id, f & 0x7F, pdu.first().copied().unwrap_or(0)));
        } else {
            shared.log_line(format!("TI {}: manual FC 0x{f:02X} ok", hdr.transaction_id));
        }
    }
}

fn store_read(shared: &Shared, func: u8, start: u16, pdu: &[u8]) {
    if func != Function::ReadHolding as u8 || pdu.is_empty() {
        return; // only mirror holding reads into the display regs
    }
    let mut regs = lock(&shared.regs);
    for (i, c) in pdu[1..].chunks(2).enumerate() {
        if c.len() < 2 { break; }
        if let Some(addr) = start.checked_add(i as u16) {
            regs.set_holding(addr, u16::from_be_bytes([c[0], c[1]]));
        }
    }
}

fn parse_request(query: &[u8]) -> (u8, u16, u16) {
    if query.len() >= 12 {
        (query[7], u16::from_be_bytes([query[8], query[9]]), u16::from_be_bytes([query[10], query[11]]))
    } else {
        (0, 0, 0)
    }
}
