//! Client view (`TFRMCLIENT`): poll-list editor + trigger modes wired to the
//! real [`ananasar_core::client::PollEngine`], rendered as a floating window. Keeps its own display
//! register file (the original's client shares the register model; here it
//! holds the values read back from the remote server it is pointed at).
//!
//! The window is composed from one module per section; each adds its
//! `*_ui` method to [`ClientView`].

mod connection;
mod lists;
mod log;
mod manual;
mod polling;
mod widgets;
mod worker;

use ananasar_core::registers::RegisterFile;
use eframe::egui;
use lists::RangeDialog;
use polling::{MAX_READ, MAX_WRITE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use worker::{spawn_worker, Cmd, Shared};

pub use worker::Mode;

pub struct ClientView {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    host: String,
    port: String,
    keepalive: bool,
    uid: String,
    read_holding: Vec<u16>,
    read_input: Vec<u16>,
    write: Vec<u16>,
    addr_input: String,
    mode: Mode,
    max_read: u16,
    max_write: u16,
    interval_ms: u64,
    base_one: bool,
    zero_ti: bool,
    man_addr: String,
    man_value: String,
    man_and: String,
    man_or: String,
    man_use_fc16: bool,
    range: RangeDialog,
}

impl ClientView {
    pub fn new(default_host: String, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            log: Mutex::new(VecDeque::new()),
            regs: Mutex::new(RegisterFile::new()),
            connected: AtomicBool::new(false),
            status: Mutex::new(String::new()),
            partner: Mutex::new(None),
            ctx,
        });
        let (tx, rx) = mpsc::channel();
        spawn_worker(shared.clone(), rx);
        let (host, port) = ananasar_net::split_host_port(&default_host);
        let (host, port) = (host.to_string(), port.unwrap_or("502").to_string());
        Self {
            shared,
            tx,
            host,
            port,
            keepalive: false,
            uid: "1".into(),
            read_holding: vec![],
            read_input: vec![],
            write: vec![],
            addr_input: String::new(),
            mode: Mode::Pause,
            max_read: MAX_READ,
            max_write: MAX_WRITE,
            interval_ms: 1000,
            base_one: false,
            zero_ti: false,
            man_addr: "0".into(),
            man_value: "0".into(),
            man_and: "FFFF".into(),
            man_or: "0000".into(),
            man_use_fc16: false,
            range: RangeDialog::new(),
        }
    }

    fn push_lists(&self) {
        let _ = self.tx.send(Cmd::SetLists {
            rh: self.read_holding.clone(),
            ri: self.read_input.clone(),
            w: self.write.clone(),
        });
    }

    /// True while the client is connected to a server.
    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let connected = self.shared.connected.load(Ordering::SeqCst);
        self.connection_ui(ui, connected);
        self.polling_ui(ui);
        self.lists_ui(ui);
        self.manual_ui(ui, connected);
        ui.separator();
        self.log_ui(ui);
    }
}
