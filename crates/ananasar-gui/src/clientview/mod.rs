//! Client view (`TFRMCLIENT`): poll-list editor + trigger modes wired to the
//! real [`ananasar_core::client::PollEngine`], rendered as a floating window. Like the original's
//! client it shares the register file of the GUI's own server: holding reads
//! land in the main window's list, and writes send the values held there.
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

use crate::embed::SharedRegs;
use eframe::egui;
use widgets::Canvas;
use connection::ConnDialog;
use lists::RangeDialog;
use polling::{DEFAULT_WRITE, MAX_READ};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use worker::{spawn_worker, Cmd, Shared};

pub use worker::Mode;

/// What the client window asks of the application around it.
#[derive(Default)]
pub struct Requests {
    /// Edit > Hide Window.
    pub hide: bool,
    /// Modbus > Register Scanner.
    pub scanner: bool,
}

pub struct ClientView {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    host: String,
    port: String,
    keepalive: bool,
    uid: u16,
    /// Which read list the Read Input/Holding register group edits.
    read_input_selected: bool,
    read_addr: u16,
    write_addr: u16,
    read_holding: Vec<u16>,
    read_input: Vec<u16>,
    write: Vec<u16>,
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
    man_open: bool,
    man_mask: bool,
    conn: ConnDialog,
    range: RangeDialog,
}

impl ClientView {
    pub fn new(default_host: String, regs: SharedRegs, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            log: Mutex::new(VecDeque::new()),
            regs,
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
            uid: 1,
            read_input_selected: false,
            read_addr: 0,
            write_addr: 0,
            read_holding: vec![],
            read_input: vec![],
            write: vec![],
            mode: Mode::Pause,
            max_read: MAX_READ,
            max_write: DEFAULT_WRITE,
            interval_ms: 1000,
            base_one: false,
            zero_ti: false,
            man_addr: "0".into(),
            man_value: "0".into(),
            man_and: "FFFF".into(),
            man_or: "0000".into(),
            man_use_fc16: false,
            man_open: false,
            man_mask: false,
            conn: ConnDialog::new(),
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

    /// The window contents: menu bar, then a fixed-size client area laid out
    /// at absolute positions.
    pub fn ui(&mut self, ui: &mut egui::Ui, menu: egui::containers::menu::MenuConfig) -> Requests {
        let connected = self.shared.connected.load(Ordering::SeqCst);
        let mut requests = self.menu_ui(ui, menu, connected);
        requests.scanner |= self.modbus_shortcuts(ui.ctx(), connected);
        let c = Canvas::new(ui, 301.0, 522.0);
        self.connection_ui(ui, &c, connected);
        self.lists_ui(ui, &c);
        self.polling_ui(ui, &c);
        self.log_ui(ui, &c, (6.0, 412.0, 288.0, 106.0));
        self.manual_ui(ui, connected);
        requests
    }

    /// The Edit and Modbus menus: the actions that have no control of their own.
    fn menu_ui(&mut self, ui: &mut egui::Ui, menu: egui::containers::menu::MenuConfig, connected: bool) -> Requests {
        let mut requests = Requests::default();
        egui::MenuBar::new().config(menu).ui(ui, |ui| {
            ui.menu_button("Edit", |ui| {
                requests.hide = self.edit_menu(ui);
            });
            ui.menu_button("Modbus", |ui| {
                requests.scanner = self.modbus_menu(ui, connected);
            });
        });
        requests
    }
}
