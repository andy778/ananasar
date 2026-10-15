//! Ananasar — a menu-driven Modbus/TCP client/server, consolidating the server
//! (Values list), client and Set-value views into one app around a single
//! in-process register model and embedded server. The GUI is egui/eframe.

// Release builds on Windows are GUI-subsystem apps, so no console window opens.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod clientview;
#[cfg(windows)]
mod ddeview;
mod dialog;
mod embed;
mod scannerview;
mod filewindow;
mod menu;
mod setvalview;
mod theme;

use ananasar_core::protocol::Exception;
use ananasar_core::registers::{Bank, RegisterFile};
use ananasar_core::log::LogMode;
use ananasar_core::sync::lock;
use clientview::ClientView;
use eframe::egui;
use embed::{ServerHandle, SharedRegs};
use menu::{sc, Action};
use scannerview::ScannerView;
use filewindow::FileWindow;
use setvalview::SetValView;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VISIBLE_ROWS: u16 = 26;
/// The original's port on Windows; elsewhere ports below 1024 need privileges.
#[cfg(windows)]
const DEFAULT_PORT: u16 = 502;
#[cfg(not(windows))]
const DEFAULT_PORT: u16 = 15020;
/// The fixed client area of the main window, below the menu bar.
const CLIENT_W: f32 = 627.0;
/// How far the controls and Server group sit left of the original's positions,
/// since the register list is narrower here.
const SHIFT_X: f32 = 33.0;
/// (x, width) of the Start Address and UID group boxes. Each is wider than its
/// 12 px caption (about 66 and 20 px) plus the caption's 8 px inset, and a gap
/// separates them.
const START_BOX: (f32, f32) = (214.0, 84.0);
const UID_BOX: (f32, f32) = (306.0, 62.0);
const CLIENT_H: f32 = 548.0;
/// Inner size of the client window: its 301 x 522 client area, menu bar and margins.
const CLIENT_WINDOW_W: f32 = 309.0;
const CLIENT_WINDOW_H: f32 = 560.0;

/// The port from `--port N` / `-p N` on the command line, if given.
/// An unusable value exits with a message rather than silently using the default.
fn cli_port() -> Option<u16> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" || a == "-p" {
            match args.next().as_deref().map(parse_port) {
                Some(Ok(p)) => return Some(p),
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
    None
}

/// How many ports are tried in total (the default and the ones after it).
const PORT_SEARCH_ATTEMPTS: u16 = 20;

/// Try `start`, then each following port, until `bind` succeeds. Only
/// "address in use" moves on to the next port; any other error stops the
/// search. Gives up after `attempts` ports or at 65535.
fn first_free_port<T>(start: u16, attempts: u16, mut bind: impl FnMut(u16) -> std::io::Result<T>) -> std::io::Result<(u16, T)> {
    let mut last = std::io::Error::from(std::io::ErrorKind::AddrInUse);
    for port in (start..=u16::MAX).take(usize::from(attempts)) {
        match bind(port) {
            Ok(v) => return Ok((port, v)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Text of the notice shown when the default port was taken at startup.
fn port_changed_text(new: u16, default: u16) -> String {
    format!("Port changed to {new}. Default port {default} is currently used by other instance of Ananas.")
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([CLIENT_W, CLIENT_H + 48.0])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_title(window_title(&local_ip(), cli_port().unwrap_or(DEFAULT_PORT)));
    // Window/taskbar icon — the two-pineapple logo (assets/icon.png).
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Ananasar — Modbus/TCP", options, Box::new(|cc| {
        // Light by default, like the original; View → Theme can switch it.
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        theme::apply(&cc.egui_ctx, true);
        theme::load_monospace_font(&cc.egui_ctx);
        Ok(Box::new(App::new(&cc.egui_ctx)))
    }))
}

struct App {
    regs: SharedRegs,
    server: Arc<ServerHandle>,
    server_stop: Arc<AtomicBool>,
    server_port: u16,
    port_dialog: Option<PortDialog>,
    /// Modal notice shown once at startup when the default port was busy.
    port_notice: Option<String>,
    #[cfg(windows)]
    dde: ddeview::DdeView,
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
    about_icon: Option<egui::TextureHandle>,
    local_ip: Option<std::net::IpAddr>,
    title: String,
    show_client: bool,
    show_setval: bool,
    show_scanner: bool,
    log_interpretive: bool,
    /// An error to show the user in a message box.
    message: Option<String>,
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
        let mut port = cli_port().unwrap_or(DEFAULT_PORT);
        let mut port_notice = None;
        let mut startup_error = None;
        let start = |p: u16| embed::start(ctx.clone(), regs.clone(), server.clone(), p, stop.clone());
        if cli_port().is_some() {
            if let Err(e) = start(port) {
                *lock(&server.status) = format!("bind :{port} failed: {e}");
                startup_error = Some(format!("Cannot listen on port {port}: {e}"));
            }
        } else {
            // Like the original: when the default port is used by another
            // instance, move up to the next free one and tell the user.
            match first_free_port(port, PORT_SEARCH_ATTEMPTS, start) {
                Ok((p, ())) => {
                    if p != port {
                        port_notice = Some(port_changed_text(p, port));
                    }
                    port = p;
                }
                Err(e) => {
                    *lock(&server.status) = format!("bind :{port} failed: {e}");
                    startup_error = Some(format!("Cannot listen on port {port}: {e}"));
                }
            }
        }
        Self {
            regs,
            server,
            server_stop: stop,
            server_port: port,
            port_dialog: None,
            port_notice,
            #[cfg(windows)]
            dde: ddeview::DdeView::default(),
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
            about_icon: None,
            local_ip: local_ip(),
            title: String::new(),
            show_client: false,
            show_setval: false,
            show_scanner: false,
            log_interpretive: true,
            message: startup_error,
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
        let current = self.server_port;
        let closed = dialog::show(ctx, "server-port", "Server port", [223.0, 109.0], 0, &mut |ui| {
            // Fixed 223x109 client area; controls sit at explicit offsets.
            let (area, _) = ui.allocate_exact_size(egui::vec2(223.0, 109.0), egui::Sense::hover());
            let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
            setvalview::group_box(ui, at(8.0, 8.0, 96.0, 57.0), "Port to use");
            let r = ui.put(at(20.0, 28.0, 65.0, 22.0), egui::TextEdit::singleline(&mut dlg.input)).on_hover_text("Select port used by server");
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            let (text, color) = if dlg.error.is_empty() {
                (format!("Port currently used: {current}"), egui::Color32::BLACK)
            } else {
                (dlg.error.clone(), ui.visuals().error_fg_color)
            };
            ui.put(at(8.0, 75.0, 207.0, 28.0), egui::Label::new(egui::RichText::new(text).color(color)).wrap());
            ok |= theme::bevel_button(ui, at(120.0, 8.0, 96.0, 33.0), "port_ok", "OK", true, None).clicked();
            cancel = theme::bevel_button(ui, at(120.0, 47.0, 96.0, 33.0), "port_cancel", "Cancel", true, None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if cancel || closed {
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

    fn port_notice_ui(&mut self, ctx: &egui::Context) {
        let Some(text) = self.port_notice.clone() else { return };
        let mut close = false;
        egui::Window::new("Ananas Modbus/TCP server - client").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(text);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                close = ui.add_sized([75.0, 25.0], egui::Button::new("OK")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
            });
        });
        if close {
            self.port_notice = None;
        }
    }

    /// Options → TCP → Port → Monitor Port: stop the listener, or start it again.
    fn toggle_listening(&mut self, ctx: &egui::Context) {
        if self.server.listening.load(Ordering::SeqCst) {
            self.server_stop.store(true, Ordering::SeqCst);
            return;
        }
        self.server_stop.store(false, Ordering::SeqCst);
        self.status_note.clear();
        if let Err(e) = embed::start(ctx.clone(), self.regs.clone(), self.server.clone(), self.server_port, self.server_stop.clone()) {
            self.message = Some(format!("Cannot listen on port {}: {e}", self.server_port));
        }
    }

    /// The About window: our own logo, then the version and credits, right-aligned like the original's.
    fn about_ui(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let icon = self.about_icon.get_or_insert_with(|| {
            let image = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png"))
                .map(|d| egui::ColorImage::from_rgba_unmultiplied([d.width as usize, d.height as usize], &d.rgba))
                .unwrap_or_else(|_| egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
            ctx.load_texture("about-icon", image, egui::TextureOptions::LINEAR)
        });
        let texture = icon.clone();
        let bits = if cfg!(target_pointer_width = "64") { "64-bit" } else { "32-bit" };
        let lines = [format!("Version: {}", env!("CARGO_PKG_VERSION")), bits.to_owned(), "MIT licence".to_owned()];
        let closed = dialog::show(ctx, "about", "About Ananasar", [296.0, 340.0], 0, &mut |ui| {
            let (area, _) = ui.allocate_exact_size(egui::vec2(296.0, 340.0), egui::Sense::hover());
            let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
            ui.painter().image(texture.id(), at(48.0, 12.0, 200.0, 200.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            for (i, line) in lines.iter().enumerate() {
                let y = 232.0 + 18.0 * f32::from(u8::try_from(i).unwrap_or(0));
                ui.painter().text(at(280.0, y, 0.0, 0.0).min, egui::Align2::RIGHT_CENTER, line, egui::FontId::proportional(12.0), egui::Color32::BLACK);
            }
            ui.put(at(12.0, 290.0, 272.0, 40.0), egui::Label::new(egui::RichText::new("A port of Ananas, the free Modbus/TCP tool by Tuomio, built with REA.").size(11.0)).wrap());
        });
        if closed {
            self.show_about = false;
        }
    }

    /// A message box for errors that have no other place on the fixed-size window.
    fn message_ui(&mut self, ctx: &egui::Context) {
        let Some(text) = self.message.clone() else { return };
        let mut close = false;
        egui::Window::new("Ananasar").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(text);
            ui.add_space(6.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(75.0, 23.0), egui::Sense::hover());
            close = theme::bevel_button(ui, rect, "message_ok", "OK", true, None).clicked();
        });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.message = None;
        }
    }

    /// The "Registers" list: one sunken white list with a header row.
    fn registers_ui(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        const COLS: [f32; 3] = [58.0, 48.0, 34.0];
        const HEADER_H: f32 = 18.0;
        const ROW_H: f32 = 19.0;
        let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
        let painter = ui.painter().clone();
        let black = egui::Color32::BLACK;
        let font = egui::FontId::proportional(12.0);
        painter.text(at(14.0, 8.0, 0.0, 0.0).min, egui::Align2::LEFT_TOP, "Registers", font.clone(), black);
        let outer = at(14.0, 25.0, COLS.iter().sum::<f32>() + 4.0, HEADER_H + ROW_H * f32::from(VISIBLE_ROWS) + 4.0);
        let inner = outer.shrink(2.0);
        painter.rect_filled(inner, 0.0, egui::Color32::WHITE);
        let line = egui::Stroke::new(1.0, theme::GRID_LINE);
        let xs = [inner.left(), inner.left() + COLS[0], inner.left() + COLS[0] + COLS[1], inner.left() + COLS[0] + COLS[1] + COLS[2]];
        let col_x = |i: usize| xs.get(i).copied().unwrap_or(inner.left());

        // Header row, in bold (the text drawn twice, a fraction of a pixel apart).
        for (i, title) in ["Register", "Value", "R/W"].into_iter().enumerate() {
            let galley = painter.layout_no_wrap(title.to_owned(), font.clone(), black);
            let pos = egui::pos2(col_x(i) + 4.0, inner.top() + (HEADER_H - galley.size().y) / 2.0);
            painter.galley(pos, galley.clone(), black);
            painter.galley(theme::shift(pos, 0.6, 0.0), galley, black);
        }
        let list_top = inner.top() + HEADER_H;
        painter.line_segment([egui::pos2(inner.left(), list_top), egui::pos2(inner.right(), list_top)], line);
        for i in 1..3 {
            let x = col_x(i);
            painter.line_segment([egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom())], line);
        }

        // Mouse wheel over the list scrolls by ten registers.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 && ui.rect_contains_pointer(outer) {
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
        for (n, (addr, value)) in snapshot.into_iter().enumerate() {
            let top = list_top + ROW_H * f32::from(u16::try_from(n).unwrap_or(0));
            let row_y = top + ROW_H / 2.0;
            painter.line_segment([egui::pos2(inner.left(), top + ROW_H), egui::pos2(inner.right(), top + ROW_H)], line);
            let text_at = |col: usize| egui::pos2(col_x(col) + 4.0, row_y);
            let reg = if self.addr_hex { format!("{addr:04X}") } else { addr.to_string() };
            painter.text(text_at(0), egui::Align2::LEFT_CENTER, reg, font.clone(), black);
            painter.text(text_at(2), egui::Align2::LEFT_CENTER, "-/-", font.clone(), black);

            let cell = egui::Rect::from_min_max(egui::pos2(col_x(1) + 1.0, top + 1.0), egui::pos2(col_x(2), top + ROW_H));
            if self.editing == Some(addr) {
                let edit = egui::TextEdit::singleline(&mut self.edit_buf).frame(egui::Frame::NONE).margin(egui::Margin::symmetric(3, 0)).vertical_align(egui::Align::Center);
                let resp = ui.put(cell, edit);
                resp.request_focus();
                if resp.lost_focus() {
                    if let Some(v) = self.parse(&self.edit_buf.clone()) {
                        lock(&self.regs).set_holding(addr, v);
                    }
                    self.editing = None;
                }
                continue;
            }
            painter.text(text_at(1), egui::Align2::LEFT_CENTER, self.fmt(value), font.clone(), black);
            let resp = ui.interact(cell, ui.id().with(("reg_cell", n)), egui::Sense::click());
            if resp.clicked() || resp.double_clicked() {
                self.selected = Some(addr);
            }
            if editable && resp.double_clicked() {
                self.editing = Some(addr);
                self.edit_buf = self.fmt(value);
            }
            if self.selected == Some(addr) {
                let r = cell.shrink(1.0);
                let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0, black), 1.0, 1.0));
            }
        }
        theme::sunken_border(&painter, outer);
    }

    /// Type Shown, Start Address, UID, Client and Logging, in one band.
    fn controls_ui(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
        let log = self.server.log.clone();
        let listening = self.server.listening.load(Ordering::SeqCst);
        let recording = log.is_recording();

        setvalview::group_box(ui, at(214.0, 16.0, 153.0, 36.0), "Type Shown");
        if ui.put(at(222.0, 28.0, 66.0, 20.0), egui::RadioButton::new(self.bank == Bank::Input, "Input")).clicked() {
            self.bank = Bank::Input;
        }
        if ui.put(at(295.0, 28.0, 70.0, 20.0), egui::RadioButton::new(self.bank == Bank::Holding, "Holding")).clicked() {
            self.bank = Bank::Holding;
        }

        setvalview::group_box(ui, at(START_BOX.0, 58.0, START_BOX.1, 47.0), "Start Address");
        // Every address is in the list (in hex while registers are shown as hex); it can also be typed.
        theme::combo_u16(ui, at(START_BOX.0 + 4.0, 77.0, START_BOX.1 - 8.0, 21.0), "start_address", &mut self.start_address, 65535, 1, 65536, self.addr_hex);
        setvalview::group_box(ui, at(UID_BOX.0, 58.0, UID_BOX.1, 47.0), "UID");
        let mut uid_wide = u16::from(lock(&self.server.cfg).unit_id);
        if theme::combo_u16(ui, at(UID_BOX.0 + 8.0, 77.0, UID_BOX.1 - 16.0, 21.0), "uid", &mut uid_wide, 255, 1, 256, false) {
            lock(&self.server.cfg).unit_id = u8::try_from(uid_wide).unwrap_or(u8::MAX);
        }

        setvalview::group_box(ui, at(391.0, 16.0, 132.0, 89.0), "Client");
        let text = if self.show_client { "Hide" } else { "Show" };
        let rect = at(406.0, 34.0, 100.0, 38.0);
        if theme::bevel_button(ui, rect, "show_client", text, true, Some(theme::icon_window)).clicked() {
            self.show_client = !self.show_client;
        }
        let state = if self.client.is_connected() { "Connected" } else { "Disconnected" };
        ui.painter().text(at(457.0, 87.0, 0.0, 0.0).min, egui::Align2::CENTER_CENTER, state, egui::FontId::proportional(12.0), egui::Color32::BLACK);

        setvalview::group_box(ui, at(535.0, 16.0, 120.0, 89.0), "Logging");
        let enabled = recording || listening;
        let rect = at(545.0, 34.0, 100.0, 38.0);
        let text = if recording { "Stop" } else { "Start" };
        if theme::bevel_button(ui, rect, "logging", text, enabled, Some(theme::icon_play)).clicked() {
            if recording {
                log.stop();
                self.status_note.clear();
            } else {
                let mode = if self.log_interpretive { LogMode::Interpretive } else { LogMode::Raw };
                match log.start(std::path::Path::new("."), &self.server_port.to_string(), mode) {
                    Ok(path) => self.status_note = format!("Logging to {}", path.display()),
                    Err(e) => self.message = Some(format!("Cannot start log: {e}")),
                }
            }
        }
        ui.add_enabled_ui(!recording, |ui| ui.put(at(552.0, 79.0, 96.0, 20.0), egui::Checkbox::new(&mut self.log_interpretive, "Interpretive")));
    }

    /// The Server group: one value column per connection slot.
    fn server_ui(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        const ROWS_Y: [f32; 22] = [
            139.0, 156.0, 173.0, 197.0, 214.0, 239.0, 256.0, 273.0, 290.0, 307.0, 324.0, 341.0, 363.0, 377.0, 399.0, 421.0, 443.0, 463.0, 479.0, 495.0, 511.0,
            527.0,
        ];
        const LABELS: [&str; 22] = [
            "Connection IP", "Nagle", "Poll interval", "Data packet size", "Header check", "Transaction ID", "Protocol ID", "Message length", "Unit ID",
            "Function code", "Start address", "Word count", "Data received", "Data sent", "Local port", "Logged packets", "Address offset", "Out of Synch",
            "Incorrect length", "Incorrect PID", "Incorrect TI", "Return exception",
        ];
        const COL_X: [f32; 2] = [391.0, 526.0];
        let at = |x: f32, y: f32| setvalview::client_rect(area, x, y, 0.0, 0.0).min;
        let stats = lock(&self.server.stats).clone();
        let log = self.server.log.clone();
        let recording = log.is_recording();
        let font = egui::FontId::monospace(13.0);
        let dark = egui::Color32::BLACK;
        let flag = |on: bool| if on { "On" } else { "Off" }.to_owned();
        let forced = lock(&self.server.cfg).forced_exception.is_some();
        setvalview::group_box(ui, setvalview::client_rect(area, 206.0, 120.0, 448.0, 425.0), "Server");
        let painter = ui.painter().clone();
        let dash = || "--".to_owned();
        for (i, (label, y)) in LABELS.iter().zip(ROWS_Y).enumerate() {
            painter.text(at(215.0, y), egui::Align2::LEFT_CENTER, format!("{label}:"), font.clone(), egui::Color32::BLACK);
            let single = match i {
                14 => Some(self.server.port.load(Ordering::SeqCst).to_string()),
                15 => Some(if recording || log.recordings() > 0 { log.recordings().to_string() } else { dash() }),
                16 | 20 => Some(flag(false)),
                17 => Some(flag(stats.out_of_sync)),
                18 => Some(flag(stats.incorrect_length)),
                19 => Some(flag(stats.incorrect_protocol_id)),
                21 => Some(flag(forced || stats.return_exception.is_some())),
                _ => None,
            };
            if let Some(v) = single {
                let colour = if i == 14 { theme::VALUE_BLUE } else { dark };
                painter.text(at(COL_X[0], y), egui::Align2::LEFT_CENTER, v, font.clone(), colour);
                continue;
            }
            for (slot, x) in COL_X.into_iter().enumerate() {
                let c = stats.slots.get(slot).and_then(Option::as_ref);
                let v = match i {
                    0 => c.map_or_else(dash, |c| c.ip.clone()),
                    1 => c.map_or_else(dash, |c| if c.nagle { "Enabled" } else { "Disabled" }.to_owned()),
                    2 => fmt_ms(c.and_then(|c| c.poll_interval_ms)),
                    3 => c.map_or(0, |c| c.packet_size).to_string(),
                    4 => match c.and_then(|c| c.header_check.as_ref()) {
                        Some(Ok(())) => "Passed".to_owned(),
                        Some(Err(_)) => "Failed".to_owned(),
                        None => dash(),
                    },
                    5 => hex16(c.and_then(|c| c.transaction_id).unwrap_or(0)),
                    6 => hex16_plain(c.and_then(|c| c.protocol_id).unwrap_or(0)),
                    7 => hex16(c.and_then(|c| c.length).unwrap_or(0)),
                    8 => hex8(c.and_then(|c| c.unit_id).unwrap_or(0)),
                    9 => hex8(c.and_then(|c| c.function).unwrap_or(0)),
                    10 => hex16(c.and_then(|c| c.start_address).unwrap_or(0)),
                    11 => hex16(c.and_then(|c| c.word_count).unwrap_or(0)),
                    12 => c.map_or(0, |c| c.bytes_received).to_string(),
                    _ => c.map_or(0, |c| c.bytes_sent).to_string(),
                };
                let colour = if (5..=11).contains(&i) { theme::VALUE_BLUE } else { dark };
                let rect = painter.text(at(x, y), egui::Align2::LEFT_CENTER, v, font.clone(), colour);
                // A failed header check says why when hovered.
                if i == 4 && self.tooltips && let Some(Err(why)) = c.and_then(|c| c.header_check.as_ref()) {
                    ui.interact(rect, ui.id().with(("header_check", slot)), egui::Sense::hover()).on_hover_text(why);
                }
            }
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
        if ctx.input_mut(|i| i.consume_shortcut(&sc::OPEN)) {
            self.file.open();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc::SAVE)) {
            self.file.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc::EXIT)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // The more specific shortcut first, so Ctrl+Alt+H is not taken as Ctrl+H.
        if ctx.input_mut(|i| i.consume_shortcut(&sc::ADDR_HEX)) {
            self.addr_hex = !self.addr_hex;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc::HEX)) {
            self.hex = !self.hex;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc::SET_REGISTER)) {
            self.show_setval = true;
        }
        // Ctrl+Alt shortcuts first, so Ctrl+Alt+S is not taken as Ctrl+S.
        if ctx.input_mut(|i| i.consume_shortcut(&sc::SCANNER)) {
            self.show_scanner = true;
        }
        let mut toggle_listen = ctx.input_mut(|i| i.consume_shortcut(&sc::LISTEN));
        let mut open_port = ctx.input_mut(|i| i.consume_shortcut(&sc::CHANGE_PORT));
        if ctx.input_mut(|i| i.consume_shortcut(&sc::TOOLTIPS)) {
            self.tooltips = !self.tooltips;
        }
        // Value List shortcuts leave Ctrl+C/E/M to a focused text field.
        let mut set_edit: Option<bool> = None;
        let mut copy_list = false;
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&sc::VL_EDIT)) {
                set_edit = Some(true);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&sc::VL_MONITOR)) {
                set_edit = Some(false);
            }
            copy_list = ctx.input_mut(|i| i.consume_shortcut(&sc::VL_COPY));
        }
        let title = window_title(&self.local_ip, self.server_port);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        let menus = {
            let cfg = lock(&self.server.cfg);
            menu::build(&menu::MenuState {
                value_list_edit: self.value_list_edit,
                addr_hex: self.addr_hex,
                hex: self.hex,
                listening: self.server.listening.load(Ordering::SeqCst),
                port: self.server_port,
                tooltips: self.tooltips,
                supported: &cfg.supported,
                forced_exception: cfg.forced_exception,
            })
        };
        let mut chosen = None;
        egui::Panel::top("menubar").show(ui, |ui| {
            egui::MenuBar::new().config(theme::menu_config(self.classic)).ui(ui, |ui| {
                chosen = menu::show(ui, &menus);
            });
        });
        match chosen {
            Some(Action::Open) => self.file.open(),
            Some(Action::SaveAs) => self.file.save(),
            Some(Action::Exit) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Action::About) => self.show_about = true,
            Some(Action::ValueListEdit) => set_edit = Some(true),
            Some(Action::ValueListMonitor) => set_edit = Some(false),
            Some(Action::CopyList) => copy_list = true,
            Some(Action::SetRegister) => self.show_setval = true,
            Some(Action::Initialize(bank, mode)) => lock(&self.regs).initialize(bank, mode),
            Some(Action::AddrHex) => self.addr_hex = !self.addr_hex,
            Some(Action::Hex) => self.hex = !self.hex,
            Some(Action::ToggleFunction(fc)) => {
                let mut cfg = lock(&self.server.cfg);
                if !cfg.supported.remove(&fc) {
                    cfg.supported.insert(fc);
                }
            }
            Some(Action::ForceException(exc)) => {
                let mut cfg = lock(&self.server.cfg);
                cfg.forced_exception = next_forced_exception(cfg.forced_exception, exc);
            }
            Some(Action::Scanner) => self.show_scanner = true,
            Some(Action::MonitorPort) => toggle_listen = true,
            Some(Action::ChangePort) => open_port = true,
            #[cfg(windows)]
            Some(Action::DdeServer) => self.dde.open(),
            Some(Action::Tooltips) => self.tooltips = !self.tooltips,
            None => {}
        }

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
        self.port_notice_ui(&ctx);
        self.about_ui(&ctx);
        #[cfg(windows)]
        self.dde.show(&ctx, &self.regs, &mut self.message);
        self.message_ui(&ctx);

        // ---- status bar: the date and time in a sunken panel, as in the original ----
        egui::Panel::bottom("statusbar").exact_size(24.0).show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(128.0, 19.0), egui::Sense::hover());
            theme::sunken_panel(ui.painter(), rect);
            let now = chrono::Local::now().format("%d/%m/%Y %H.%M.%S").to_string();
            ui.painter().text(theme::shift(rect.left_center(), 4.0, 0.0), egui::Align2::LEFT_CENTER, now, egui::FontId::proportional(12.0), egui::Color32::BLACK);
            // The server state that used to fill the bar is kept as a tooltip.
            let st = lock(&self.server.status).clone();
            let n = self.server.clients.load(Ordering::SeqCst);
            let mut tip = format!("Server :{} \u{2014} {st} \u{2014} {n} client(s)", self.server_port);
            if !self.status_note.is_empty() {
                tip.push('\n');
                tip.push_str(&self.status_note);
            }
            resp.on_hover_text(tip);
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        });

        // ---- the window's fixed-size client area ----
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ui.visuals().panel_fill)).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(egui::vec2(CLIENT_W, CLIENT_H), egui::Sense::hover());
            self.registers_ui(ui, area);
            let shifted = area.translate(egui::vec2(-SHIFT_X, 0.0));
            self.controls_ui(ui, shifted);
            self.server_ui(ui, shifted);
        });

        // ---- floating windows (the original's separate windows) ----
        // The client is a window of its own, like in the original, not a panel inside this one.
        if self.show_client {
            let classic = self.classic;
            let client = &mut self.client;
            let builder = egui::ViewportBuilder::default()
                .with_title("Modbus/TCP Client")
                .with_inner_size([CLIENT_WINDOW_W, CLIENT_WINDOW_H])
                .with_resizable(false)
                .with_maximize_button(false);
            let mut close = false;
            let mut requests = clientview::Requests::default();
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("client"), builder, |ctx, _class| {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.global_style().visuals.panel_fill).inner_margin(egui::Margin::same(4))).show(ctx, |ui| {
                    requests = client.ui(ui, theme::menu_config(classic));
                });
                close = ctx.input(|i| i.viewport().close_requested());
            });
            if close || requests.hide {
                self.show_client = false;
            }
            if requests.scanner {
                self.show_scanner = true;
            }
        }

        let mut show_setval = self.show_setval;
        let regs = self.regs.clone();
        self.setval.show(&ctx, &mut show_setval, &regs, theme::menu_config(self.classic));
        self.show_setval = show_setval;

        let mut show_scanner = self.show_scanner;
        self.scanner.show(&ctx, &mut show_scanner, &self.regs, self.tooltips);
        self.show_scanner = show_scanner;

        if let Some(err) = self.file.ui(&self.regs, self.bank, &mut self.start_address) {
            self.message = Some(err);
        }
    }
}

/// The forced exception after choosing `chosen`: choosing the active one again turns it off.
fn next_forced_exception(current: Option<Exception>, chosen: Exception) -> Option<Exception> {
    (current != Some(chosen)).then_some(chosen)
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

/// A poll interval with its unit; 0 ms while none has been measured.
fn fmt_ms(ms: Option<u64>) -> String {
    format!("{} ms", ms.unwrap_or(0))
}

fn hex16(v: u16) -> String {
    let [hi, lo] = v.to_be_bytes();
    format!("{hi:02X} {lo:02X}  ({v})")
}

/// Hex byte and decimal value; the decimal lines up with the two-byte fields.
fn hex8(v: u8) -> String {
    format!("{v:02X}     ({v})")
}

fn hex16_plain(v: u16) -> String {
    let [hi, lo] = v.to_be_bytes();
    format!("{hi:02X} {lo:02X}")
}

#[cfg(test)]
mod tests {
    use super::{first_free_port, fmt_ms, hex8, hex16, hex16_plain, list_text, next_forced_exception, parse_port, port_changed_text, Exception};
    use crate::menu::{EXCEPTION_CHOICES, FUNCTION_TOGGLES};

    #[test]
    fn list_text_is_tab_separated_and_honours_hex() {
        let rows = [(10, 255), (11, 0)];
        assert_eq!(list_text(&rows, false, false), "10\t255\n11\t0\n");
        assert_eq!(list_text(&rows, true, true), "000A\t00FF\n000B\t0000\n");
    }

    fn in_use() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::AddrInUse)
    }

    #[test]
    fn first_free_port_skips_busy_ports() {
        let got = first_free_port(502, 20, |p| if p < 504 { Err(in_use()) } else { Ok(()) });
        assert_eq!(got.map(|(p, ())| p).ok(), Some(504));
        let got = first_free_port(502, 20, |_| Ok(()));
        assert_eq!(got.map(|(p, ())| p).ok(), Some(502));
    }

    #[test]
    fn first_free_port_gives_up_after_the_bound() {
        let mut tried = Vec::new();
        let got = first_free_port(502, 3, |p| {
            tried.push(p);
            Err::<(), _>(in_use())
        });
        assert_eq!(got.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
        assert_eq!(tried, [502, 503, 504]);
    }

    #[test]
    fn first_free_port_stops_at_65535_and_on_other_errors() {
        let mut tried = Vec::new();
        let _ = first_free_port(65534, 20, |p| {
            tried.push(p);
            Err::<(), _>(in_use())
        });
        assert_eq!(tried, [65534, 65535]);
        let mut n = 0;
        let got = first_free_port(502, 20, |_| {
            n += 1;
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert_eq!(got.err().map(|e| e.kind()), Some(std::io::ErrorKind::PermissionDenied));
        assert_eq!(n, 1);
    }

    #[test]
    fn port_changed_text_names_both_ports() {
        assert_eq!(port_changed_text(503, 502), "Port changed to 503. Default port 502 is currently used by other instance of Ananas.");
    }

    #[test]
    fn packet_fields_show_hex_then_decimal() {
        assert_eq!(hex16(0), "00 00  (0)");
        assert_eq!(hex16(258), "01 02  (258)");
        assert_eq!(hex8(3), "03     (3)");
        assert_eq!(hex16_plain(0), "00 00");
    }

    #[test]
    fn start_address_and_uid_boxes_do_not_overlap() {
        let (sx, sw) = super::START_BOX;
        let (ux, uw) = super::UID_BOX;
        assert!(ux - (sx + sw) >= 6.0, "gap between the boxes");
        assert!(sw >= 66.0 + 16.0 && uw >= 20.0 + 16.0, "wide enough for the captions");
        // The Type Shown box above spans 214..367; both boxes stay inside it.
        assert!(sx >= 214.0 && ux + uw <= 367.0 + 1.0);
    }

    #[test]
    fn poll_interval_always_has_its_unit() {
        assert_eq!(fmt_ms(None), "0 ms");
        assert_eq!(fmt_ms(Some(250)), "250 ms");
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

    #[test]
    fn choosing_the_active_exception_again_turns_it_off() {
        let busy = Exception::SlaveDeviceBusy;
        assert_eq!(next_forced_exception(None, busy), Some(busy));
        assert_eq!(next_forced_exception(Some(busy), busy), None);
        assert_eq!(next_forced_exception(Some(Exception::Acknowledge), busy), Some(busy));
    }

    #[test]
    fn option_menus_match_the_original_entries() {
        let fcs: Vec<u8> = FUNCTION_TOGGLES.iter().map(|&(fc, _)| fc).collect();
        assert_eq!(fcs, [0x03, 0x04, 0x06, 0x07, 0x08, 0x10, 0x16, 0x17]);
        let codes: Vec<u8> = EXCEPTION_CHOICES.iter().map(|&(e, _)| e as u8).collect();
        assert_eq!(codes, [1, 2, 3, 4, 5, 6, 7, 8, 10, 11]);
    }
}
