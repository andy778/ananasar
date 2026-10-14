//! Ananasar — a menu-driven Modbus/TCP client/server, consolidating the server
//! (Values list), client and Set-value views into one app around a single
//! in-process register model and embedded server. The GUI is egui/eframe.

mod clientview;
mod embed;
mod scannerview;
mod filewindow;
mod logview;
mod setvalview;

use ananasar_core::registers::{Bank, InitMode, RegisterFile};
use ananasar_core::server::ServerConfig;
use ananasar_core::sync::lock;
use clientview::ClientView;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use embed::{ServerHandle, SharedRegs};
use scannerview::ScannerView;
use filewindow::FileWindow;
use logview::LogView;
use setvalview::SetValView;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VISIBLE_ROWS: u16 = 25;
const DEFAULT_PORT: u16 = 15020;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([560.0, 700.0]);
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
    show_log: bool,
    client: ClientView,
    setval: SetValView,
    scanner: ScannerView,
    file: FileWindow,
    logview: LogView,
}

impl App {
    /// Build the app and start its embedded server. Worker threads keep a
    /// clone of `ctx` and repaint only when something changes.
    fn new(ctx: &egui::Context) -> Self {
        let regs: SharedRegs = Arc::new(Mutex::new(RegisterFile::new()));
        let server = ServerHandle::new();
        let stop = Arc::new(AtomicBool::new(false));
        embed::start(ctx.clone(), regs.clone(), server.clone(), ServerConfig::default(), DEFAULT_PORT, stop.clone());
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
            show_log: false,
            client: ClientView::new(format!("127.0.0.1:{DEFAULT_PORT}"), ctx.clone()),
            setval: SetValView::default(),
            scanner: ScannerView::new(format!("127.0.0.1:{DEFAULT_PORT}"), ctx.clone()),
            file: FileWindow::default(),
            logview: LogView::default(),
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
                    ui.checkbox(&mut self.show_log, "Data logging");
                    ui.separator();
                    ui.label("Theme");
                    egui::widgets::global_theme_preference_buttons(ui);
                });
                ui.menu_button("Server", |ui| {
                    let listening = self.server.listening.load(Ordering::SeqCst);
                    ui.label(format!("Port {}", self.server_port));
                    if listening {
                        if ui.button("Stop listening").clicked() {
                            self.server_stop.store(true, Ordering::SeqCst);
                            ui.close();
                        }
                    } else if ui.button("Start listening").clicked() {
                        self.server_stop.store(false, Ordering::SeqCst);
                        embed::start(ctx.clone(), self.regs.clone(), self.server.clone(), ServerConfig::default(), self.server_port, self.server_stop.clone());
                        ui.close();
                    }
                });
            });
        });

        // ---- status / register controls ----
        egui::Panel::top("controls").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.label("Registers:");
                    ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
                    ui.radio_value(&mut self.bank, Bank::Input, "Input");
                });
                ui.group(|ui| {
                    ui.label("Start address:");
                    ui.add(egui::DragValue::new(&mut self.start_address).range(0..=65535).speed(1.0));
                });
                ui.checkbox(&mut self.hex, "Hex");
            });
            ui.horizontal(|ui| {
                let st = lock(&self.server.status).clone();
                let n = self.server.clients.load(Ordering::SeqCst);
                ui.label(format!("Server :{} — {st} — {n} client(s)", self.server_port));
            });
            ui.add_space(4.0);
        });

        // ---- main Values list ----
        egui::CentralPanel::default().show(ui, |ui| {
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
                .column(Column::exact(120.0))
                .column(Column::remainder())
                .column(Column::exact(60.0))
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
                            row.col(|ui| { ui.monospace("—"); });
                        });
                    }
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

        let mut show_log = self.show_log;
        let (log, port, listening) = (
            self.server.log.clone(),
            self.server.port.load(Ordering::SeqCst),
            self.server.listening.load(Ordering::SeqCst),
        );
        egui::Window::new("Data logging")
            .open(&mut show_log)
            .show(&ctx, |ui| self.logview.ui(ui, &log, port, listening));
        self.show_log = show_log;
        self.file.ui(&ctx, &self.regs, self.bank, &mut self.start_address);
    }
}
