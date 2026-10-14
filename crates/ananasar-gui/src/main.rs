//! Ananasar — a menu-driven Modbus/TCP client/server, consolidating the server
//! (Values list), client and Set-value views into one app around a single
//! in-process register model and embedded server. The GUI is egui/eframe.

// Release builds on Windows are GUI-subsystem apps, so no console window opens.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod clientview;
mod embed;
mod scannerview;
mod filewindow;
mod setvalview;

use ananasar_core::registers::{Bank, InitMode, RegisterFile};
use ananasar_core::log::LogMode;
use ananasar_core::sync::lock;
use clientview::ClientView;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use embed::{ServerHandle, SharedRegs};
use scannerview::ScannerView;
use filewindow::FileWindow;
use setvalview::SetValView;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VISIBLE_ROWS: u16 = 26;
const DEFAULT_PORT: u16 = 15020;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([820.0, 700.0]);
    // Window/taskbar icon — the two-pineapple logo (assets/icon.png).
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Ananasar — Modbus/TCP", options, Box::new(|cc| {
        // Light by default, like the original; View → Theme can switch it.
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        Ok(Box::new(App::new(&cc.egui_ctx)))
    }))
}

struct App {
    regs: SharedRegs,
    server: Arc<ServerHandle>,
    server_stop: Arc<AtomicBool>,
    server_port: u16,
    bank: Bank,
    start_address: u16,
    hex: bool,
    editing: Option<u16>,
    edit_buf: String,
    show_client: bool,
    show_setval: bool,
    show_scanner: bool,
    log_interpretive: bool,
    status_note: String,
    client: ClientView,
    setval: SetValView,
    scanner: ScannerView,
    file: FileWindow,
}

impl App {
    /// Build the app and start its embedded server. Worker threads keep a
    /// clone of `ctx` and repaint only when something changes.
    fn new(ctx: &egui::Context) -> Self {
        let regs: SharedRegs = Arc::new(Mutex::new(RegisterFile::new()));
        let server = ServerHandle::new();
        let stop = Arc::new(AtomicBool::new(false));
        embed::start(ctx.clone(), regs.clone(), server.clone(), DEFAULT_PORT, stop.clone());
        Self {
            regs,
            server,
            server_stop: stop,
            server_port: DEFAULT_PORT,
            bank: Bank::Holding,
            start_address: 0,
            hex: false,
            editing: None,
            edit_buf: String::new(),
            show_client: false,
            show_setval: false,
            show_scanner: false,
            log_interpretive: false,
            status_note: String::new(),
            client: ClientView::new(format!("127.0.0.1:{DEFAULT_PORT}"), ctx.clone()),
            setval: SetValView::default(),
            scanner: ScannerView::new(format!("127.0.0.1:{DEFAULT_PORT}"), ctx.clone()),
            file: FileWindow::default(),
        }
    }

    fn fmt(&self, v: u16) -> String {
        if self.hex { format!("{v:04X}") } else { v.to_string() }
    }
    fn parse(&self, s: &str) -> Option<u16> {
        let s = s.trim();
        if self.hex {
            u16::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        } else {
            s.parse().ok()
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // ---- menu bar ----
        egui::Panel::top("menubar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…").clicked() {
                        self.file.open();
                        ui.close();
                    }
                    if ui.button("Save…").clicked() {
                        self.file.save();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    ui.checkbox(&mut self.hex, "Display as hex");
                    if ui.button("Set register value…").clicked() {
                        self.show_setval = true;
                        ui.close();
                    }
                    ui.menu_button("Initialize holding", |ui| {
                        if ui.button("All → 0").clicked() {
                            lock(&self.regs).initialize(Bank::Holding, InitMode::Zero);
                            ui.close();
                        }
                        if ui.button("Ascending").clicked() {
                            lock(&self.regs).initialize(Bank::Holding, InitMode::Ascending);
                            ui.close();
                        }
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_client, "Client window");
                    ui.checkbox(&mut self.show_setval, "Set value window");
                    ui.checkbox(&mut self.show_scanner, "Scanner");
                    ui.separator();
                    ui.label("Theme");
                    egui::widgets::global_theme_preference_buttons(ui);
                });
                ui.menu_button("Options", |ui| {
                    let listening = self.server.listening.load(Ordering::SeqCst);
                    ui.label(format!("Port {}", self.server_port));
                    if listening {
                        if ui.button("Stop listening").clicked() {
                            self.server_stop.store(true, Ordering::SeqCst);
                            ui.close();
                        }
                    } else if ui.button("Start listening").clicked() {
                        self.server_stop.store(false, Ordering::SeqCst);
                        embed::start(ctx.clone(), self.regs.clone(), self.server.clone(), self.server_port, self.server_stop.clone());
                        ui.close();
                    }
                });
            });
        });

        // ---- status bar ----
        egui::Panel::bottom("statusbar").show(ui, |ui| {
            let st = lock(&self.server.status).clone();
            let n = self.server.clients.load(Ordering::SeqCst);
            ui.horizontal(|ui| {
                ui.label(format!("Server :{} — {st} — {n} client(s)", self.server_port));
                if !self.status_note.is_empty() {
                    ui.separator();
                    ui.label(&self.status_note);
                }
            });
        });

        // ---- the Values list (left) ----
        egui::Panel::left("registers").resizable(false).exact_size(215.0).show(ui, |ui| {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 && ui.ui_contains_pointer() {
                let step = 10i32 * if scroll > 0.0 { 1 } else { -1 };
                self.start_address = (self.start_address as i32 + step).clamp(0, 65535) as u16;
            }
            let editable = self.bank == Bank::Holding;
            let snapshot: Vec<(u16, u16)> = {
                let regs = lock(&self.regs);
                (0..VISIBLE_ROWS)
                    .filter_map(|i| self.start_address.checked_add(i))
                    .map(|addr| (addr, regs.get(self.bank, addr)))
                    .collect()
            };
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::exact(60.0))
                .column(Column::exact(80.0))
                .column(Column::exact(30.0))
                .header(22.0, |mut h| {
                    h.col(|ui| { ui.strong("Register"); });
                    h.col(|ui| { ui.strong("Value"); });
                    h.col(|ui| { ui.strong("R/W"); });
                })
                .body(|mut body| {
                    for (addr, value) in snapshot {
                        body.row(20.0, |mut row| {
                            row.col(|ui| { ui.monospace(format!("{addr}")); });
                            row.col(|ui| {
                                if self.editing == Some(addr) {
                                    let resp = ui.add(egui::TextEdit::singleline(&mut self.edit_buf).desired_width(f32::INFINITY));
                                    resp.request_focus();
                                    if resp.lost_focus() {
                                        if let Some(v) = self.parse(&self.edit_buf.clone()) {
                                            lock(&self.regs).set_holding(addr, v);
                                        }
                                        self.editing = None;
                                    }
                                } else {
                                    let label = ui.add(egui::Label::new(egui::RichText::new(self.fmt(value)).monospace()).sense(egui::Sense::click()));
                                    if editable && label.double_clicked() {
                                        self.editing = Some(addr);
                                        self.edit_buf = self.fmt(value);
                                    }
                                }
                            });
                            row.col(|ui| { ui.monospace("-"); });
                        });
                    }
                });
        });

        // ---- controls and the Server status panel (right) ----
        egui::CentralPanel::default().show(ui, |ui| {
            let log = self.server.log.clone();
            let listening = self.server.listening.load(Ordering::SeqCst);
            let recording = log.is_recording();
            ui.horizontal_top(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Type Shown");
                        ui.radio_value(&mut self.bank, Bank::Input, "Input");
                        ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
                        ui.horizontal(|ui| {
                            ui.label("Start Address");
                            ui.add(egui::DragValue::new(&mut self.start_address).range(0..=65535).speed(1.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("UID");
                            let mut uid = lock(&self.server.cfg).unit_id;
                            if ui.add(egui::DragValue::new(&mut uid).range(0..=255)).changed() {
                                lock(&self.server.cfg).unit_id = uid;
                            }
                        });
                    });
                });
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Client");
                        let text = if self.show_client { "Hide" } else { "Show" };
                        if ui.button(text).clicked() {
                            self.show_client = !self.show_client;
                        }
                        ui.label(if self.client.is_connected() { "Connected" } else { "Disconnected" });
                    });
                });
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Logging");
                        let text = if recording { "Stop" } else { "Start" };
                        if ui.add_enabled(recording || listening, egui::Button::new(text)).clicked() {
                            if recording {
                                log.stop();
                                self.status_note.clear();
                            } else {
                                let mode = if self.log_interpretive { LogMode::Interpretive } else { LogMode::Raw };
                                self.status_note = match log.start(std::path::Path::new("."), &self.server_port.to_string(), mode) {
                                    Ok(path) => format!("Logging to {}", path.display()),
                                    Err(e) => format!("Cannot start log: {e}"),
                                };
                            }
                        }
                        ui.add_enabled(!recording, egui::Checkbox::new(&mut self.log_interpretive, "Interpretive"));
                    });
                });
            });
            ui.add_space(4.0);
            let stats = lock(&self.server.stats).clone();
            ui.group(|ui| {
                ui.label("Server");
                egui::Grid::new("server_stats").num_columns(3).spacing([14.0, 2.0]).show(ui, |ui| {
                    let conn = |i: usize| stats.slots[i].as_ref();
                    stats_row(ui, "Connection IP", |i| conn(i).map(|c| c.ip.clone()));
                    stats_row(ui, "Nagle", |i| conn(i).map(|c| if c.nagle { "Enabled" } else { "Disabled" }.to_owned()));
                    stats_row(ui, "Poll interval", |i| conn(i).and_then(|c| c.poll_interval_ms).map(|ms| format!("{ms} ms")));
                    stats_row(ui, "Data packet size", |i| conn(i).map(|c| c.packet_size.to_string()));
                    // A failed check says why when hovered.
                    ui.monospace("Header check:");
                    for slot in 0..2 {
                        match conn(slot).and_then(|c| c.header_check.as_ref()) {
                            Some(Ok(())) => { ui.monospace("Passed"); }
                            Some(Err(why)) => { ui.monospace("Failed").on_hover_text(why); }
                            None => { ui.monospace("--"); }
                        }
                    }
                    ui.end_row();
                    stats_row(ui, "Transaction ID", |i| conn(i).and_then(|c| c.transaction_id).map(hex16));
                    stats_row(ui, "Protocol ID", |i| conn(i).and_then(|c| c.protocol_id).map(hex16));
                    stats_row(ui, "Message length", |i| conn(i).and_then(|c| c.length).map(hex16));
                    stats_row(ui, "Unit ID", |i| conn(i).and_then(|c| c.unit_id).map(hex8));
                    stats_row(ui, "Function code", |i| conn(i).and_then(|c| c.function).map(hex8));
                    stats_row(ui, "Start address", |i| conn(i).and_then(|c| c.start_address).map(hex16));
                    stats_row(ui, "Word count", |i| conn(i).and_then(|c| c.word_count).map(hex16));
                    stats_row(ui, "Data received", |i| conn(i).map(|c| c.bytes_received.to_string()));
                    stats_row(ui, "Data sent", |i| conn(i).map(|c| c.bytes_sent.to_string()));
                    let flag = |on: bool| if on { "On" } else { "Off" }.to_owned();
                    let single = |ui: &mut egui::Ui, label: &str, value: String| {
                        ui.monospace(format!("{label}:"));
                        ui.monospace(value);
                        ui.end_row();
                    };
                    single(ui, "Local port", self.server.port.load(Ordering::SeqCst).to_string());
                    single(ui, "Logged packets", if recording || log.recordings() > 0 { log.recordings().to_string() } else { "--".to_owned() });
                    single(ui, "Out of Synch", flag(stats.out_of_sync));
                    single(ui, "Incorrect length", flag(stats.incorrect_length));
                    single(ui, "Incorrect PID", flag(stats.incorrect_protocol_id));
                    single(ui, "Return exception", stats.return_exception.map_or_else(|| "Off".to_owned(), |c| format!("Exception {c}")));
                });
            });
        });

        // ---- floating windows (the original's separate windows) ----
        let mut show_client = self.show_client;
        egui::Window::new("Modbus/TCP Client")
            .open(&mut show_client)
            .default_size([520.0, 520.0])
            .show(&ctx, |ui| self.client.ui(ui));
        self.show_client = show_client;

        let mut show_setval = self.show_setval;
        let regs = self.regs.clone();
        egui::Window::new("Set value")
            .open(&mut show_setval)
            .show(&ctx, |ui| self.setval.ui(ui, &regs));
        self.show_setval = show_setval;

        let mut show_scanner = self.show_scanner;
        egui::Window::new("Register Scanner")
            .open(&mut show_scanner)
            .default_size([400.0, 480.0])
            .show(&ctx, |ui| self.scanner.ui(ui));
        self.show_scanner = show_scanner;

        self.file.ui(&ctx, &self.regs, self.bank, &mut self.start_address);
    }
}

/// A row of the Server status panel: the label and one value per client slot
/// (`--` while a slot is free or has not sent anything yet).
fn stats_row(ui: &mut egui::Ui, label: &str, value: impl Fn(usize) -> Option<String>) {
    ui.monospace(format!("{label}:"));
    for slot in 0..2 {
        ui.monospace(value(slot).unwrap_or_else(|| "--".to_owned()));
    }
    ui.end_row();
}

fn hex16(v: u16) -> String {
    let [hi, lo] = v.to_be_bytes();
    format!("{hi:02X} {lo:02X}  ({v})")
}

fn hex8(v: u8) -> String {
    format!("{v:02X}  ({v})")
}
