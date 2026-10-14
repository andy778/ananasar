//! Client view (`TFRMCLIENT`): poll-list editor + trigger modes wired to the
//! real [`PollEngine`], rendered as a floating window. Keeps its own display
//! register file (the original's client shares the register model; here it
//! holds the values read back from the remote server it is pointed at).

use crate::net::read_frame;
use ananasar_core::sync::lock;
use ananasar_core::client::{AddressBase, ManualCommand, PollEngine, PollList};
use ananasar_core::protocol::{self, Function};
use ananasar_core::registers::RegisterFile;
use eframe::egui;
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    Manual(ManualCommand),
    Quit,
}

struct Shared {
    log: Mutex<VecDeque<String>>,
    regs: Mutex<RegisterFile>,
    connected: AtomicBool,
    status: Mutex<String>,
}

impl Shared {
    fn log_line(&self, s: impl Into<String>) {
        let mut log = lock(&self.log);
        log.push_back(s.into());
        while log.len() > 500 {
            log.pop_front();
        }
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
    man_addr: String,
    man_value: String,
    man_and: String,
    man_or: String,
}

impl ClientView {
    pub fn new(default_host: String) -> Self {
        let shared = Arc::new(Shared {
            log: Mutex::new(VecDeque::new()),
            regs: Mutex::new(RegisterFile::new()),
            connected: AtomicBool::new(false),
            status: Mutex::new("IP: --  Port: --".into()),
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
            man_addr: "0".into(),
            man_value: "0".into(),
            man_and: "FFFF".into(),
            man_or: "0000".into(),
        }
    }

    fn push_lists(&self) {
        let _ = self.tx.send(Cmd::SetLists {
            rh: self.read_holding.clone(),
            ri: self.read_input.clone(),
            w: self.write.clone(),
        });
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
        loop {
            loop {
                match rx.try_recv() {
                    Ok(cmd) => match cmd {
                        Cmd::Quit => return,
                        Cmd::Connect { addr, uid } => {
                            engine = PollEngine::new(uid);
                            match TcpStream::connect(&addr) {
                                Ok(s) => {
                                    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
                                    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or(addr.clone());
                                    stream = Some(s);
                                    shared.connected.store(true, Ordering::SeqCst);
                                    *lock(&shared.status) = format!("Connected: {peer}");
                                    shared.log_line(format!("connected to {peer}"));
                                }
                                Err(e) => shared.log_line(format!("connect failed: {e}")),
                            }
                        }
                        Cmd::Disconnect => {
                            stream = None;
                            shared.connected.store(false, Ordering::SeqCst);
                            *lock(&shared.status) = "IP: --  Port: --".into();
                            shared.log_line("disconnected");
                        }
                        Cmd::SetMode(m) => { mode = m; shared.log_line(format!("mode: {m:?}")); }
                        Cmd::SetLists { rh, ri, w } => {
                            engine.read_holding = PollList::new(rh);
                            engine.read_input = PollList::new(ri);
                            engine.write = PollList::new(w);
                        }
                        Cmd::SetMax(m) => { engine.max_read = m.min(125); engine.max_write = m.min(123); }
                        Cmd::SetInterval(ms) => interval = Duration::from_millis(ms),
                        Cmd::SetBase(b) => engine.base = b,
                        Cmd::Manual(cmd) => {
                            if let Some(s) = stream.as_mut() {
                                let frame = engine.build_manual(cmd);
                                if s.write_all(&frame).is_ok()
                                    && let Ok(resp) = read_frame(s) {
                                        log_resp(&shared, &resp);
                                    }
                            }
                        }
                    },
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            let active = matches!(mode, Mode::Cycle | Mode::WaitAfter | Mode::Immediate);
            if active && stream.is_some() && !engine.is_idle() {
                let query = { let regs = lock(&shared.regs); engine.next_query(&regs) };
                if let Some(q) = query {
                    let s = stream.as_mut().unwrap();
                    if poll_once(s, &q, &shared).is_err() {
                        stream = None;
                        shared.connected.store(false, Ordering::SeqCst);
                        *lock(&shared.status) = "IP: --  Port: --".into();
                        shared.log_line("connection lost");
                    }
                }
                match mode {
                    Mode::Immediate => std::thread::sleep(Duration::from_millis(1)),
                    _ => std::thread::sleep(interval),
                }
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
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
