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
mod theme;

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

/// The port from `--port N` / `-p N` on the command line, else the default.
/// An unusable value exits with a message rather than silently using the default.
fn cli_port() -> u16 {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" || a == "-p" {
            match args.next().as_deref().map(parse_port) {
                Some(Ok(p)) => return p,
                Some(Err(e)) => {
                    eprintln!("{a}: {e}");
                    std::process::exit(2);
                }
                None => {
                    eprintln!("{a} needs a port number");
                    std::process::exit(2);
                }
            }
        }
    }
    DEFAULT_PORT
}

/// Parse a TCP port number (1-65535).
fn parse_port(s: &str) -> Result<u16, String> {
    match s.trim().parse::<u16>() {
        Ok(0) | Err(_) => Err("port must be a number between 1 and 65535".into()),
        Ok(p) => Ok(p),
    }
}

/// The local address that outbound traffic would leave from. A UDP "connect"
/// only selects a route and sends nothing.
fn local_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    Some(sock.local_addr().ok()?.ip())
}

fn window_title(ip: &Option<std::net::IpAddr>, port: u16) -> String {
    match ip {
        Some(ip) => format!("Ananasar - Modbus/TCP server at [{ip}]"),
        None => format!("Ananasar - Modbus/TCP server on port {port}"),
    }
}

/// State of the Options → Port… dialog.
struct PortDialog {
    input: String,
    error: String,
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([820.0, 700.0]).with_title(window_title(&local_ip(), cli_port()));
    // Window/taskbar icon — the two-pineapple logo (assets/icon.png).
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Ananasar — Modbus/TCP", options, Box::new(|cc| {
        // Light by default, like the original; View → Theme can switch it.
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        theme::apply(&cc.egui_ctx, true);
        Ok(Box::new(App::new(&cc.egui_ctx)))
    }))
}

struct App {
    regs: SharedRegs,
    server: Arc<ServerHandle>,
    server_stop: Arc<AtomicBool>,
    server_port: u16,
    port_dialog: Option<PortDialog>,
    bank: Bank,
    start_address: u16,
    tooltips: bool,
    hex: bool,
    selected: Option<u16>,
    addr_hex: bool,
    classic: bool,
    editing: Option<u16>,
    /// Value List mode: false = Monitor (read-only, the default), true = Edit.
    value_list_edit: bool,
    edit_buf: String,
    show_about: bool,
    local_ip: Option<std::net::IpAddr>,
    title: String,
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
        let port = cli_port();
        if let Err(e) = embed::start(ctx.clone(), regs.clone(), server.clone(), port, stop.clone()) {
            *lock(&server.status) = format!("bind :{port} failed: {e}");
        }
        Self {
            regs,
            server,
            server_stop: stop,
            server_port: port,
            port_dialog: None,
            bank: Bank::Holding,
            start_address: 0,
            tooltips: true,
            hex: false,
            selected: None,
            addr_hex: false,
            classic: true,
            editing: None,
            value_list_edit: false,
            edit_buf: String::new(),
            show_about: false,
            local_ip: local_ip(),
            title: String::new(),
            show_client: false,
            show_setval: false,
            show_scanner: false,
            log_interpretive: false,
            status_note: String::new(),
            client: ClientView::new(format!("127.0.0.1:{port}"), ctx.clone()),
            setval: SetValView::default(),
            scanner: ScannerView::new(format!("127.0.0.1:{port}"), ctx.clone()),
            file: FileWindow::default(),
        }
    }

    /// Move the running server to `port`. The new port is bound first, so on
    /// failure the old server keeps running and the error is returned.
    fn change_port(&mut self, ctx: &egui::Context, port: u16) -> Result<(), String> {
        if port == self.server_port && self.server.listening.load(Ordering::SeqCst) {
            return Ok(());
        }
        let stop = Arc::new(AtomicBool::new(false));
        embed::start(ctx.clone(), self.regs.clone(), self.server.clone(), port, stop.clone())
            .map_err(|e| format!("Cannot listen on port {port}: {e}"))?;
        self.server_stop.store(true, Ordering::SeqCst);
        self.server_stop = stop;
        self.server_port = port;
        self.status_note.clear();
        Ok(())
    }

    fn port_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.port_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("Server port").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            // Fixed 223x109 client area; controls sit at explicit offsets.
            let (area, _) = ui.allocate_exact_size(egui::vec2(223.0, 109.0), egui::Sense::hover());
            let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
            setvalview::group_box(ui, at(8.0, 8.0, 96.0, 57.0), "Port to use");
            let r = ui.put(at(20.0, 28.0, 65.0, 22.0), egui::TextEdit::singleline(&mut dlg.input)).on_hover_text("Select port used by server");
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            let (text, color) = if dlg.error.is_empty() {
                (format!("Port currently used: {}", self.server_port), egui::Color32::BLACK)
            } else {
                (dlg.error.clone(), ui.visuals().error_fg_color)
            };
            ui.put(at(8.0, 75.0, 207.0, 28.0), egui::Label::new(egui::RichText::new(text).color(color)).wrap());
            ok |= ui.put(at(120.0, 8.0, 96.0, 33.0), egui::Button::new("OK")).clicked();
            cancel = ui.put(at(120.0, 47.0, 96.0, 33.0), egui::Button::new("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if cancel {
            return;
        }
        if ok {
            match parse_port(&dlg.input).and_then(|p| self.change_port(ctx, p)) {
                Ok(()) => return,
                Err(e) => dlg.error = e,
            }
        }
        self.port_dialog = Some(dlg);
    }

    /// Options → TCP → Port → Monitor Port: stop the listener, or start it again.
    fn toggle_listening(&mut self, ctx: &egui::Context) {
        if self.server.listening.load(Ordering::SeqCst) {
            self.server_stop.store(true, Ordering::SeqCst);
            return;
        }
        self.server_stop.store(false, Ordering::SeqCst);
        self.status_note = match embed::start(ctx.clone(), self.regs.clone(), self.server.clone(), self.server_port, self.server_stop.clone()) {
            Ok(()) => String::new(),
            Err(e) => format!("Cannot listen on port {}: {e}", self.server_port),
        };
    }

    fn about_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About Ananasar").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.heading("Ananasar");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("Modbus/TCP server and client");
            ui.label("Licensed under the MIT licence.");
            ui.separator();
            ui.label("A port of Ananas, the free Modbus/TCP tool by Tuomio, built with REA.");
        });
        self.show_about = open;
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
        let open_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
        let save_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        let exit_sc = egui::KeyboardShortcut::new(egui::Modifiers::ALT, egui::Key::F4);
        if ctx.input_mut(|i| i.consume_shortcut(&open_sc)) {
            self.file.open();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&save_sc)) {
            self.file.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&exit_sc)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let setreg_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::R);
        let hex_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::H);
        let addr_hex_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::H);
        // The more specific shortcut first, so Ctrl+Shift+H is not taken as Ctrl+H.
        if ctx.input_mut(|i| i.consume_shortcut(&addr_hex_sc)) {
            self.addr_hex = !self.addr_hex;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&hex_sc)) {
            self.hex = !self.hex;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&setreg_sc)) {
            self.show_setval = true;
        }
        let item = |ui: &mut egui::Ui, text: &str, sc: &egui::KeyboardShortcut| {
            ui.add(egui::Button::new(text).shortcut_text(ctx.format_shortcut(sc))).clicked()
        };
        let scanner_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::C);
        let offset_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::O);
        let synch_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::S);
        let listen_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::P);
        let port_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
        let tips_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::T);
        // Ctrl+Alt shortcuts first, so Ctrl+Alt+S is not taken as Ctrl+S.
        if ctx.input_mut(|i| i.consume_shortcut(&scanner_sc)) {
            self.show_scanner = true;
        }
        let mut toggle_listen = ctx.input_mut(|i| i.consume_shortcut(&listen_sc));
        let mut open_port = ctx.input_mut(|i| i.consume_shortcut(&port_sc));
        if ctx.input_mut(|i| i.consume_shortcut(&tips_sc)) {
            self.tooltips = !self.tooltips;
        }
        let check = |ui: &mut egui::Ui, on: bool, text: &str, sc: &egui::KeyboardShortcut| {
            let label = if on { format!("\u{2713} {text}") } else { format!("    {text}") };
            ui.add(egui::Button::new(label).shortcut_text(ctx.format_shortcut(sc))).clicked()
        };
        // Value List shortcuts leave Ctrl+C/E/M to a focused text field.
        let vl_edit_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E);
        let vl_monitor_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::M);
        let vl_copy_sc = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::C);
        let mut set_edit: Option<bool> = None;
        let mut copy_list = false;
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&vl_edit_sc)) {
                set_edit = Some(true);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&vl_monitor_sc)) {
                set_edit = Some(false);
            }
            copy_list = ctx.input_mut(|i| i.consume_shortcut(&vl_copy_sc));
        }
        let title = window_title(&self.local_ip, self.server_port);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menubar").show(ui, |ui| {
            egui::MenuBar::new().config(theme::menu_config(self.classic)).ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    ui.set_min_width(190.0);
                    if item(ui, "Open…", &open_sc) {
                        self.file.open();
                        ui.close();
                    }
                    if item(ui, "Save As…", &save_sc) {
                        self.file.save();
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, "Exit", &exit_sc) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.separator();
                    if ui.button("About…").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    ui.set_min_width(190.0);
                    ui.menu_button("Value List", |ui| {
                        ui.set_min_width(190.0);
                        if check(ui, self.value_list_edit, "Edit", &vl_edit_sc) {
                            set_edit = Some(true);
                            ui.close();
                        }
                        if check(ui, !self.value_list_edit, "Monitor", &vl_monitor_sc) {
                            set_edit = Some(false);
                            ui.close();
                        }
                        ui.separator();
                        if item(ui, "Copy", &vl_copy_sc) {
                            copy_list = true;
                            ui.close();
                        }
                    });
                    ui.separator();
                    if item(ui, "Set Register Value", &setreg_sc) {
                        self.show_setval = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Initialize", |ui| {
                        for (name, bank) in [("Input Registers", Bank::Input), ("Holding Registers", Bank::Holding)] {
                            ui.menu_button(name, |ui| {
                                if ui.button("All to '0'").clicked() {
                                    lock(&self.regs).initialize(bank, InitMode::Zero);
                                    ui.close();
                                }
                                if ui.button("Ascending").clicked() {
                                    lock(&self.regs).initialize(bank, InitMode::Ascending);
                                    ui.close();
                                }
                            });
                        }
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.set_min_width(190.0);
                    if check(ui, self.hex, "Values as Hex", &hex_sc) {
                        self.hex = !self.hex;
                    }
                    if check(ui, self.addr_hex, "Registers as Hex", &addr_hex_sc) {
                        self.addr_hex = !self.addr_hex;
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_client, "Client window");
                    ui.checkbox(&mut self.show_setval, "Set value window");
                    ui.checkbox(&mut self.show_scanner, "Scanner");
                    ui.separator();
                    ui.label("Theme");
                    egui::widgets::global_theme_preference_buttons(ui);
                    if ui.checkbox(&mut self.classic, "Classic style (light)").changed() {
                        theme::apply(&ctx, self.classic);
                    }
                });
                ui.menu_button("Options", |ui| {
                    ui.set_min_width(190.0);
                    let listening = self.server.listening.load(Ordering::SeqCst);
                    // Entries of the original with no counterpart in the engine yet are shown greyed out.
                    let off = |ui: &mut egui::Ui, text: &str| {
                        ui.add_enabled(false, egui::Button::new(text));
                    };
                    ui.menu_button("Modbus", |ui| {
                        ui.set_min_width(260.0);
                        ui.add_enabled_ui(false, |ui| {
                            ui.menu_button("Active MB Functions", |_| {});
                        });
                        ui.separator();
                        off(ui, "Return Incorrect TI");
                        off(ui, "Return Incorrect Protocol ID");
                        off(ui, "Return Incorrect Length High Byte");
                        ui.add_enabled_ui(false, |ui| {
                            ui.menu_button("Return Exception", |_| {});
                        });
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Use Address Offset").shortcut_text(ctx.format_shortcut(&offset_sc)));
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Reply Out of Synch").shortcut_text(ctx.format_shortcut(&synch_sc)));
                        ui.separator();
                        if item(ui, "Register Scanner…", &scanner_sc) {
                            self.show_scanner = true;
                            ui.close();
                        }
                    });
                    ui.menu_button("TCP", |ui| {
                        ui.set_min_width(260.0);
                        ui.menu_button("Port", |ui| {
                            ui.set_min_width(260.0);
                            if check(ui, listening, &format!("Monitor Port {}", self.server_port), &listen_sc) {
                                toggle_listen = true;
                                ui.close();
                            }
                            if item(ui, "Change Port…", &port_sc) {
                                open_port = true;
                                ui.close();
                            }
                            ui.separator();
                            off(ui, "Purge Port Usage Blocking(s)");
                        });
                        ui.add_enabled_ui(false, |ui| {
                            ui.menu_button("Disable Nagle", |_| {});
                        });
                        ui.separator();
                        // The server always drops a connection silent for a minute.
                        ui.add_enabled(false, egui::Button::new("\u{2713} Release Connection if Unused >1 min"));
                    });
                    ui.separator();
                    off(ui, "DDE Server…");
                    ui.separator();
                    if check(ui, self.tooltips, "Show Tooltips", &tips_sc) {
                        self.tooltips = !self.tooltips;
                    }
                });
            });
        });

        if let Some(edit) = set_edit {
            self.value_list_edit = edit;
            if !edit {
                self.editing = None;
            }
        }
        if copy_list {
            let rows: Vec<(u16, u16)> = {
                let regs = lock(&self.regs);
                (0..VISIBLE_ROWS)
                    .filter_map(|i| self.start_address.checked_add(i))
                    .map(|addr| (addr, regs.get(self.bank, addr)))
                    .collect()
            };
            ctx.copy_text(list_text(&rows, self.addr_hex, self.hex));
        }
        if toggle_listen {
            self.toggle_listening(&ctx);
        }
        if open_port {
            self.port_dialog = Some(PortDialog { input: self.server_port.to_string(), error: String::new() });
        }

        self.port_dialog_ui(&ctx);
        self.about_ui(&ctx);

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
                let step = if scroll > 0.0 { 10i32 } else { -10 };
                self.start_address = u16::try_from((i32::from(self.start_address).saturating_add(step)).clamp(0, 65535)).unwrap_or(0);
            }
            let editable = self.bank == Bank::Holding && self.value_list_edit;
            let snapshot: Vec<(u16, u16)> = {
                let regs = lock(&self.regs);
                (0..VISIBLE_ROWS)
                    .filter_map(|i| self.start_address.checked_add(i))
                    .map(|addr| (addr, regs.get(self.bank, addr)))
                    .collect()
            };
            let cell = |ui: &mut egui::Ui, selected: bool| {
                if selected {
                    ui.painter().rect_filled(ui.max_rect(), 0.0, theme::HIGHLIGHT);
                }
                ui.painter().rect_stroke(ui.max_rect(), 0.0, theme::grid_stroke(), egui::StrokeKind::Inside);
            };
            TableBuilder::new(ui)
                .striped(false)
                .column(Column::exact(60.0))
                .column(Column::exact(80.0))
                .column(Column::exact(30.0))
                .header(22.0, |mut h| {
                    h.col(|ui| { cell(ui, false); ui.strong("Register"); });
                    h.col(|ui| { cell(ui, false); ui.strong("Value"); });
                    h.col(|ui| { cell(ui, false); ui.strong("R/W"); });
                })
                .body(|mut body| {
                    for (addr, value) in snapshot {
                        body.row(20.0, |mut row| {
                            row.col(|ui| {
                                cell(ui, false);
                                ui.monospace(if self.addr_hex { format!("{addr:04X}") } else { addr.to_string() });
                            });
                            row.col(|ui| {
                                let selected = self.selected == Some(addr) && self.editing != Some(addr);
                                cell(ui, selected);
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
                                    let mut text = egui::RichText::new(self.fmt(value)).monospace();
                                    if selected {
                                        text = text.color(egui::Color32::WHITE);
                                    }
                                    let label = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                                    if label.clicked() || label.double_clicked() {
                                        self.selected = Some(addr);
                                    }
                                    if editable && label.double_clicked() {
                                        self.editing = Some(addr);
                                        self.edit_buf = self.fmt(value);
                                    }
                                }
                            });
                            row.col(|ui| {
                                cell(ui, false);
                                ui.monospace("-/-");
                            });
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
                theme::titled_group(ui, "Type Shown", |ui| {
                    ui.vertical(|ui| {
                        ui.radio_value(&mut self.bank, Bank::Input, "Input");
                        ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
                        ui.horizontal(|ui| {
                            ui.label("Start Address");
                            theme::combo_u16(ui, "start_address", &mut self.start_address, 65535, &[0, 10, 20, 50, 100, 200, 500, 1000, 10000]);
                        });
                        ui.horizontal(|ui| {
                            ui.label("UID");
                            let mut uid = lock(&self.server.cfg).unit_id;
                            let mut uid_wide = u16::from(uid);
                            if theme::combo_u16(ui, "uid", &mut uid_wide, 255, &[0, 1, 2, 3, 4, 5, 247, 255]) {
                                uid = uid_wide as u8;
                                lock(&self.server.cfg).unit_id = uid;
                            }
                        });
                    });
                });
                theme::titled_group(ui, "Client", |ui| {
                    ui.vertical(|ui| {
                        let text = if self.show_client { "Hide" } else { "Show" };
                        if ui.add(egui::Button::new(text).min_size(egui::vec2(72.0, 32.0))).clicked() {
                            self.show_client = !self.show_client;
                        }
                        ui.label(if self.client.is_connected() { "Connected" } else { "Disconnected" });
                    });
                });
                theme::titled_group(ui, "Logging", |ui| {
                    ui.vertical(|ui| {
                        let text = if recording { "Stop" } else { "Start" };
                        if ui.add_enabled(recording || listening, egui::Button::new(text).min_size(egui::vec2(72.0, 32.0))).clicked() {
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
            theme::titled_group(ui, "Server", |ui| {
                egui::Grid::new("server_stats").num_columns(3).spacing([14.0, 2.0]).show(ui, |ui| {
                    let conn = |i: usize| stats.slots.get(i).and_then(Option::as_ref);
                    stats_row(ui, "Connection IP", |i| conn(i).map(|c| c.ip.clone()));
                    stats_row(ui, "Nagle", |i| conn(i).map(|c| if c.nagle { "Enabled" } else { "Disabled" }.to_owned()));
                    stats_row(ui, "Poll interval", |i| conn(i).and_then(|c| c.poll_interval_ms).map(|ms| format!("{ms} ms")));
                    stats_row(ui, "Data packet size", |i| conn(i).map(|c| c.packet_size.to_string()));
                    // A failed check says why when hovered.
                    ui.monospace("Header check:");
                    for slot in 0..2 {
                        match conn(slot).and_then(|c| c.header_check.as_ref()) {
                            Some(Ok(())) => { ui.monospace("Passed"); }
                            Some(Err(why)) => {
                                let failed = ui.monospace("Failed");
                                if self.tooltips {
                                    failed.on_hover_text(why);
                                }
                            }
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
                        ui.label(egui::RichText::new(value).monospace().color(theme::VALUE_BLUE));
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
        self.setval.show(&ctx, &mut show_setval, &regs, theme::menu_config(self.classic));
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
        ui.label(egui::RichText::new(value(slot).unwrap_or_else(|| "--".to_owned())).monospace().color(theme::VALUE_BLUE));
    }
    ui.end_row();
}

/// The register list as tab-separated `address<TAB>value` lines.
fn list_text(rows: &[(u16, u16)], addr_hex: bool, hex: bool) -> String {
    let mut out = String::new();
    for &(addr, v) in rows {
        let a = if addr_hex { format!("{addr:04X}") } else { addr.to_string() };
        let v = if hex { format!("{v:04X}") } else { v.to_string() };
        out.push_str(&format!("{a}\t{v}\n"));
    }
    out
}

fn hex16(v: u16) -> String {
    let [hi, lo] = v.to_be_bytes();
    format!("{hi:02X} {lo:02X}  ({v})")
}

fn hex8(v: u8) -> String {
    format!("{v:02X}  ({v})")
}

#[cfg(test)]
mod tests {
    use super::{list_text, parse_port};

    #[test]
    fn list_text_is_tab_separated_and_honours_hex() {
        let rows = [(10, 255), (11, 0)];
        assert_eq!(list_text(&rows, false, false), "10\t255\n11\t0\n");
        assert_eq!(list_text(&rows, true, true), "000A\t00FF\n000B\t0000\n");
    }

    #[test]
    fn port_must_be_1_to_65535() {
        assert_eq!(parse_port(" 502 "), Ok(502));
        assert_eq!(parse_port("65535"), Ok(65535));
        assert!(parse_port("0").is_err());
        assert!(parse_port("65536").is_err());
        assert!(parse_port("abc").is_err());
        assert!(parse_port("").is_err());
    }
}
