//! Client view (`TFRMCLIENT`): poll-list editor + trigger modes wired to the
//! real [`PollEngine`], rendered as a floating window. Keeps its own display
//! register file (the original's client shares the register model; here it
//! holds the values read back from the remote server it is pointed at).

use ananasar_net::read_frame;
use ananasar_core::client::{AddressBase, ManualCommand, PollEngine, PollList};
use ananasar_core::protocol::{self, Function};
use ananasar_core::registers::RegisterFile;
use ananasar_core::sync::lock;
use eframe::egui;
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
enum Cmd {
    Connect { addr: String, uid: u8 },
    Disconnect,
    SetMode(Mode),
    SetLists { rh: Vec<u16>, ri: Vec<u16>, w: Vec<u16> },
    SetMax(u16),
    SetInterval(u64),
    SetBase(AddressBase),
    SetZeroTi(bool),
    Manual(ManualCommand),
    Quit,
}

struct Shared {
    log: Mutex<VecDeque<String>>,
    regs: Mutex<RegisterFile>,
    connected: AtomicBool,
    status: Mutex<String>,
    /// Wakes the UI when the worker changes anything it shows.
    ctx: egui::Context,
}

impl Shared {
    fn set_status(&self, s: impl Into<String>) {
        *lock(&self.status) = s.into();
        self.ctx.request_repaint();
    }

    fn log_line(&self, s: impl Into<String>) {
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

pub struct ClientView {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    host: String,
    uid: String,
    read_holding: Vec<u16>,
    read_input: Vec<u16>,
    write: Vec<u16>,
    addr_input: String,
    mode: Mode,
    max_per_query: u16,
    interval_ms: u64,
    base_one: bool,
    zero_ti: bool,
    man_addr: String,
    man_value: String,
    man_and: String,
    man_or: String,
    range: RangeDialog,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListKind {
    ReadHolding,
    ReadInput,
    Write,
}

/// State of the From/To dialog that adds or removes a whole address range.
struct RangeDialog {
    open: bool,
    from: String,
    to: String,
    list: ListKind,
    remove: bool,
}

impl ClientView {
    pub fn new(default_host: String, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            log: Mutex::new(VecDeque::new()),
            regs: Mutex::new(RegisterFile::new()),
            connected: AtomicBool::new(false),
            status: Mutex::new("IP: --  Port: --".into()),
            ctx,
        });
        let (tx, rx) = mpsc::channel();
        spawn_worker(shared.clone(), rx);
        Self {
            shared,
            tx,
            host: default_host,
            uid: "1".into(),
            read_holding: vec![],
            read_input: vec![],
            write: vec![],
            addr_input: String::new(),
            mode: Mode::Pause,
            max_per_query: 125,
            interval_ms: 1000,
            base_one: false,
            zero_ti: false,
            man_addr: "0".into(),
            man_value: "0".into(),
            man_and: "FFFF".into(),
            man_or: "0000".into(),
            range: RangeDialog { open: false, from: "0".into(), to: "0".into(), list: ListKind::ReadHolding, remove: false },
        }
    }

    fn push_lists(&self) {
        let _ = self.tx.send(Cmd::SetLists {
            rh: self.read_holding.clone(),
            ri: self.read_input.clone(),
            w: self.write.clone(),
        });
    }

    fn range_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.range.open;
        let mut apply = false;
        egui::Window::new("Add / remove range").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let r = &mut self.range;
            ui.horizontal(|ui| {
                ui.label("From:");
                ui.add(egui::TextEdit::singleline(&mut r.from).desired_width(60.0));
                ui.label("To:");
                ui.add(egui::TextEdit::singleline(&mut r.to).desired_width(60.0));
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut r.list, ListKind::ReadHolding, "Read H");
                ui.radio_value(&mut r.list, ListKind::ReadInput, "Read I");
                ui.radio_value(&mut r.list, ListKind::Write, "Write");
            });
            ui.checkbox(&mut r.remove, "Remove");
            let bounds = parse_range(&r.from, &r.to);
            if ui.add_enabled(bounds.is_some(), egui::Button::new(if r.remove { "Remove" } else { "Add" })).clicked() {
                apply = true;
            }
        });
        self.range.open = open;
        if apply && let Some((from, to)) = parse_range(&self.range.from, &self.range.to) {
            let list = match self.range.list {
                ListKind::ReadHolding => &mut self.read_holding,
                ListKind::ReadInput => &mut self.read_input,
                ListKind::Write => &mut self.write,
            };
            if self.range.remove {
                remove_range(list, from, to);
            } else {
                add_range(list, from, to);
            }
            self.push_lists();
        }
    }

    /// True while the client is connected to a server.
    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let connected = self.shared.connected.load(Ordering::SeqCst);
        ui.horizontal(|ui| {
            ui.label("Server:");
            ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(150.0));
            ui.label("UID:");
            ui.add(egui::TextEdit::singleline(&mut self.uid).desired_width(32.0));
            if !connected {
                if ui.button("Connect").clicked() {
                    let uid = self.uid.trim().parse().unwrap_or(1);
                    let _ = self.tx.send(Cmd::Connect { addr: self.host.clone(), uid });
                    self.push_lists();
                    let _ = self.tx.send(Cmd::SetMode(self.mode));
                }
            } else if ui.button("Disconnect").clicked() {
                let _ = self.tx.send(Cmd::Disconnect);
            }
            ui.label(lock(&self.shared.status).clone());
        });

        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.label("Trigger:");
                let before = self.mode;
                ui.radio_value(&mut self.mode, Mode::Cycle, "Cycle");
                ui.radio_value(&mut self.mode, Mode::WaitAfter, "Wait");
                ui.radio_value(&mut self.mode, Mode::Immediate, "Imm");
                ui.radio_value(&mut self.mode, Mode::Pause, "Pause");
                if self.mode != before {
                    let _ = self.tx.send(Cmd::SetMode(self.mode));
                }
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Max/q:");
                    if ui.add(egui::DragValue::new(&mut self.max_per_query).range(1..=125)).changed() {
                        let _ = self.tx.send(Cmd::SetMax(self.max_per_query));
                    }
                    ui.label("ms:");
                    if ui.add(egui::DragValue::new(&mut self.interval_ms).range(0..=60000).speed(10.0)).changed() {
                        let _ = self.tx.send(Cmd::SetInterval(self.interval_ms));
                    }
                });
                if ui.checkbox(&mut self.base_one, "1-based").changed() {
                    let _ = self.tx.send(Cmd::SetBase(if self.base_one { AddressBase::One } else { AddressBase::Zero }));
                }
                if ui.checkbox(&mut self.zero_ti, "TI always 00 00").changed() {
                    let _ = self.tx.send(Cmd::SetZeroTi(self.zero_ti));
                }
            });
        });

        ui.horizontal(|ui| {
            ui.label("Addr:");
            ui.add(egui::TextEdit::singleline(&mut self.addr_input).desired_width(60.0));
            let a = self.addr_input.trim().parse::<u16>().ok();
            if ui.button("+Read H").clicked()
                && let Some(a) = a { add_sorted(&mut self.read_holding, a); self.push_lists(); }
            if ui.button("+Read I").clicked()
                && let Some(a) = a { add_sorted(&mut self.read_input, a); self.push_lists(); }
            if ui.button("+Write").clicked()
                && let Some(a) = a { add_sorted(&mut self.write, a); self.push_lists(); }
        });
        ui.horizontal(|ui| {
            let mut changed = false;
            changed |= list_chips(ui, "Read H", &mut self.read_holding);
            changed |= list_chips(ui, "Read I", &mut self.read_input);
            changed |= list_chips(ui, "Write", &mut self.write);
            if changed { self.push_lists(); }
        });
        ui.horizontal(|ui| {
            if ui.button("Range…").clicked() {
                self.range.open = true;
            }
            let mut cleared = false;
            for (label, list) in [("Clear Read H", &mut self.read_holding), ("Clear Read I", &mut self.read_input), ("Clear Write", &mut self.write)] {
                if ui.add_enabled(!list.is_empty(), egui::Button::new(label)).clicked() {
                    list.clear();
                    cleared = true;
                }
            }
            if cleared { self.push_lists(); }
        });
        self.range_dialog(ui.ctx());

        ui.horizontal(|ui| {
            ui.label("Manual @");
            ui.add(egui::TextEdit::singleline(&mut self.man_addr).desired_width(50.0));
            ui.label("val(hex):");
            ui.add(egui::TextEdit::singleline(&mut self.man_value).desired_width(50.0));
            let addr = self.man_addr.trim().parse::<u16>().ok();
            let val = u16::from_str_radix(self.man_value.trim(), 16).ok();
            if ui.add_enabled(connected, egui::Button::new("FC6")).clicked()
                && let (Some(a), Some(v)) = (addr, val) {
                    let _ = self.tx.send(Cmd::Manual(ManualCommand::WriteSingle { addr: a, value: v }));
                }
            if ui.add_enabled(connected, egui::Button::new("FC7")).clicked() {
                let _ = self.tx.send(Cmd::Manual(ManualCommand::ReadExceptionStatus));
            }
            let and = u16::from_str_radix(self.man_and.trim(), 16).ok();
            let or = u16::from_str_radix(self.man_or.trim(), 16).ok();
            ui.label("AND:");
            ui.add(egui::TextEdit::singleline(&mut self.man_and).desired_width(50.0));
            ui.label("OR:");
            ui.add(egui::TextEdit::singleline(&mut self.man_or).desired_width(50.0));
            if ui.add_enabled(connected, egui::Button::new("FC22")).clicked()
                && let (Some(a), Some(am), Some(om)) = (addr, and, or) {
                    let _ = self.tx.send(Cmd::Manual(ManualCommand::MaskWrite { addr: a, and_mask: am, or_mask: om }));
                }
        });

        ui.separator();
        ui.strong("Event log");
        egui::ScrollArea::vertical().stick_to_bottom(true).max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
            let log = lock(&self.shared.log);
            for line in log.iter() {
                ui.monospace(line);
            }
        });
    }
}

fn add_sorted(v: &mut Vec<u16>, a: u16) {
    if !v.contains(&a) {
        v.push(a);
        v.sort_unstable();
    }
}

/// Parses the dialog's From/To fields; `None` unless both are addresses with `from <= to`.
fn parse_range(from: &str, to: &str) -> Option<(u16, u16)> {
    let (from, to) = (from.trim().parse::<u16>().ok()?, to.trim().parse::<u16>().ok()?);
    (from <= to).then_some((from, to))
}

/// Adds every address in `from..=to`, keeping the list sorted and free of duplicates.
fn add_range(v: &mut Vec<u16>, from: u16, to: u16) {
    v.extend(from..=to);
    v.sort_unstable();
    v.dedup();
}

/// Removes every address in `from..=to`.
fn remove_range(v: &mut Vec<u16>, from: u16, to: u16) {
    v.retain(|a| !(from..=to).contains(a));
}

fn list_chips(ui: &mut egui::Ui, title: &str, list: &mut Vec<u16>) -> bool {
    let mut removed = None;
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.label(format!("{title} ({})", list.len()));
            ui.horizontal_wrapped(|ui| {
                for (i, a) in list.iter().enumerate() {
                    if ui.small_button(format!("{a} ✕")).clicked() {
                        removed = Some(i);
                    }
                }
            });
        });
    });
    if let Some(i) = removed {
        list.remove(i);
        true
    } else {
        false
    }
}

fn spawn_worker(shared: Arc<Shared>, rx: Receiver<Cmd>) {
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
                    Cmd::Connect { addr, uid } => {
                        let zero_ti = engine.force_zero_txn;
                        engine = PollEngine::new(uid);
                        engine.force_zero_txn = zero_ti;
                        match TcpStream::connect(&addr) {
                            Ok(s) => {
                                s.set_read_timeout(Some(Duration::from_secs(3))).ok();
                                let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or(addr.clone());
                                stream = Some(s);
                                shared.connected.store(true, Ordering::SeqCst);
                                shared.set_status(format!("Connected: {peer}"));
                                shared.log_line(format!("connected to {peer}"));
                                next_poll = Instant::now();
                            }
                            Err(e) => shared.log_line(format!("connect failed: {e}")),
                        }
                    }
                    Cmd::Disconnect => {
                        stream = None;
                        shared.connected.store(false, Ordering::SeqCst);
                        shared.set_status("IP: --  Port: --");
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
                    Cmd::SetMax(m) => {
                        engine.max_read = m.min(125);
                        engine.max_write = m.min(123);
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
                shared.set_status("IP: --  Port: --");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_range_merges_sorted_and_deduped() {
        let mut v = vec![2, 10];
        add_range(&mut v, 1, 4);
        assert_eq!(v, [1, 2, 3, 4, 10]);
        add_range(&mut v, 4, 4);
        assert_eq!(v, [1, 2, 3, 4, 10]);
    }

    #[test]
    fn add_range_reaches_the_top_address() {
        let mut v = vec![];
        add_range(&mut v, 65534, 65535);
        assert_eq!(v, [65534, 65535]);
    }

    #[test]
    fn remove_range_keeps_the_rest() {
        let mut v = vec![1, 2, 3, 4, 10];
        remove_range(&mut v, 2, 4);
        assert_eq!(v, [1, 10]);
        remove_range(&mut v, 20, 30);
        assert_eq!(v, [1, 10]);
    }

    #[test]
    fn parse_range_rejects_bad_input() {
        assert_eq!(parse_range(" 3", "7 "), Some((3, 7)));
        assert_eq!(parse_range("5", "5"), Some((5, 5)));
        assert_eq!(parse_range("7", "3"), None);
        assert_eq!(parse_range("x", "3"), None);
        assert_eq!(parse_range("0", "65536"), None);
    }
}
