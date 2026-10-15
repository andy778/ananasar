//! Register scanner window: sweeps an address range with single-register
//! reads over its own connection and lists what each address answers.
//! The sweep runs on a worker thread; the UI is only woken when it changes.
//!
//! The window is a fixed-size form (client area 522x416) laid out at absolute
//! positions, as in the original.

use crate::embed::SharedRegs;
use ananasar_core::registers::Bank;
use ananasar_core::scanner::{ScanResult, ScanSummary, Scanner};
use ananasar_core::sync::lock;
use ananasar_net::{parse_endpoint, read_frame};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::io::Write;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Minimum gap between repaint requests while a sweep is running.
const REPAINT_GAP: Duration = Duration::from_millis(50);
/// How long Connect waits for the server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Client area of the form, in points.
const CLIENT: Vec2 = Vec2::new(522.0, 416.0);

#[derive(Default)]
struct State {
    running: bool,
    /// Result lines, already formatted (and filtered) for the list box.
    lines: Vec<String>,
    summary: ScanSummary,
    total: usize,
    started: Option<Instant>,
    /// Set when the sweep ends; freezes the displayed duration.
    elapsed: Option<Duration>,
    note: String,
}

/// The scanner's own connection to the server.
#[derive(Default)]
struct Conn {
    stream: Option<TcpStream>,
    peer: Option<SocketAddr>,
    connecting: bool,
}

struct Shared {
    state: Mutex<State>,
    conn: Mutex<Conn>,
    stop: AtomicBool,
    /// Wakes the UI when the worker changes anything it shows.
    ctx: egui::Context,
}

pub struct ScannerView {
    shared: Arc<Shared>,
    host: String,
    port: String,
    show_connect: bool,
    uid: u8,
    bank: Bank,
    from: u32,
    to: u32,
    timeout_ms: u32,
    offset: bool,
    failed_only: bool,
    copy_to_regs: bool,
    hscroll: bool,
}

impl ScannerView {
    /// `default_host` is `host:port` of the embedded server.
    pub fn new(default_host: String, ctx: egui::Context) -> Self {
        let (host, port) = default_host
            .rsplit_once(':')
            .map_or((default_host.clone(), String::new()), |(h, p)| (h.to_owned(), p.to_owned()));
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                conn: Mutex::new(Conn::default()),
                stop: AtomicBool::new(false),
                ctx,
            }),
            host,
            port,
            show_connect: false,
            uid: 1,
            bank: Bank::Holding,
            from: 1,
            to: 1,
            timeout_ms: 100,
            offset: false,
            failed_only: false,
            copy_to_regs: false,
            hscroll: false,
        }
    }

    fn connect(&self) {
        let (host, port) = match parse_endpoint(&self.host, &self.port) {
            Ok(hp) => hp,
            Err(e) => {
                lock(&self.shared.state).note = format!("Cannot connect: {e}");
                return;
            }
        };
        {
            let mut conn = lock(&self.shared.conn);
            if conn.connecting || conn.stream.is_some() {
                return;
            }
            conn.connecting = true;
        }
        lock(&self.shared.state).note = format!("Connecting to {host}:{port}...");
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map_err(|e| e.to_string())
                .and_then(|mut a| a.next().ok_or_else(|| "no address".to_owned()))
                .and_then(|a| TcpStream::connect_timeout(&a, CONNECT_TIMEOUT).map(|s| (a, s)).map_err(|e| e.to_string()));
            let note = {
                let mut conn = lock(&shared.conn);
                conn.connecting = false;
                match result {
                    Ok((addr, stream)) => {
                        stream.set_nodelay(true).ok();
                        conn.stream = Some(stream);
                        conn.peer = Some(addr);
                        String::new()
                    }
                    Err(e) => format!("Connect to {host}:{port} failed: {e}"),
                }
            };
            lock(&shared.state).note = note;
            shared.ctx.request_repaint();
        });
    }

    fn disconnect(&self) {
        drop_connection(&self.shared);
        lock(&self.shared.state).note.clear();
    }

    fn start_scan(&self, regs: &SharedRegs) {
        let Some((start, end)) = wire_range(self.from, self.to, self.offset) else {
            lock(&self.shared.state).note = "Invalid range".into();
            return;
        };
        let stream = lock(&self.shared.conn).stream.as_ref().and_then(|s| s.try_clone().ok());
        let Some(stream) = stream else {
            lock(&self.shared.state).note = "Not connected".into();
            return;
        };
        let total = usize::from(end.saturating_sub(start)).saturating_add(1);
        {
            let mut st = lock(&self.shared.state);
            *st = State { running: true, total, started: Some(Instant::now()), ..State::default() };
        }
        self.shared.stop.store(false, Ordering::SeqCst);
        let job = Job {
            stream,
            uid: self.uid,
            bank: self.bank,
            start,
            end,
            timeout: Duration::from_millis(u64::from(self.timeout_ms.max(1))),
            offset: self.offset,
            failed_only: self.failed_only,
            regs: self.copy_to_regs.then(|| regs.clone()),
        };
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let note = run(&shared, job);
            let mut st = lock(&shared.state);
            st.running = false;
            st.elapsed = st.started.map(|t| t.elapsed());
            st.note = note;
            drop(st);
            shared.ctx.request_repaint();
        });
        self.shared.ctx.request_repaint();
    }

    /// Show the scanner as a fixed-size window. `open` is cleared by the
    /// title-bar close button or the "Close!" menu entry.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, regs: &SharedRegs, tooltips: bool) {
        if !*open {
            self.connect_window(ctx);
            return;
        }
        let closed = crate::dialog::show(ctx, "register-scanner", "Register Scanner", [CLIENT.x, CLIENT.y + 26.0], 0, &mut |ui| {
                let mut close = false;
                egui::MenuBar::new().ui(ui, |ui| {
                    if ui.button("Close!").clicked() {
                        close = true;
                    }
                });
                self.form(ui, regs, tooltips);
                if close {
                    *open = false;
                }
            });
        if closed {
            *open = false;
        }
        self.connect_window(ctx);
    }

    /// The "Connect..." parameter window.
    fn connect_window(&mut self, ctx: &egui::Context) {
        if !self.show_connect {
            return;
        }
        let mut open = true;
        let mut connect_now = false;
        egui::Window::new("Connect to server").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("scanner_connect").num_columns(2).show(ui, |ui| {
                ui.label("IP:");
                ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(140.0));
                ui.end_row();
                ui.label("Port:");
                ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(60.0));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Connect").clicked() {
                    connect_now = true;
                }
                if ui.button("Cancel").clicked() {
                    self.show_connect = false;
                }
            });
        });
        if connect_now {
            self.connect();
            self.show_connect = false;
        }
        if !open {
            self.show_connect = false;
        }
    }

    #[allow(clippy::arithmetic_side_effects, reason = "egui geometry on f32 vectors")]
    fn form(&mut self, ui: &mut egui::Ui, regs: &SharedRegs, tooltips: bool) {
        let (rect, _) = ui.allocate_exact_size(CLIENT, egui::Sense::hover());
        let origin = rect.min;
        let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(origin + Vec2::new(x, y), Vec2::new(w, h));
        let tip = |r: egui::Response, text: &str| -> egui::Response {
            if tooltips { r.on_hover_text(text).on_disabled_hover_text(text) } else { r }
        };

        let (running, lines, summary, total, started, elapsed, note) = {
            let st = lock(&self.shared.state);
            (st.running, st.lines.clone(), st.summary, st.total, st.started, st.elapsed, st.note.clone())
        };
        let (peer, connecting) = {
            let conn = lock(&self.shared.conn);
            (conn.peer.filter(|_| conn.stream.is_some()), conn.connecting)
        };
        let connected = peer.is_some();

        // ---- Connection ----
        group(ui, at(8.0, 8.0, 272.0, 97.0), "Connection");
        let r = place(ui, at(20.0, 28.0, 100.0, 25.0), |ui| ui.add_enabled(!connecting && !connected && !running, egui::Button::new("Connect...")));
        if tip(r, "Open server connection parameter window").clicked() {
            self.show_connect = true;
        }
        let r = place(ui, at(20.0, 58.0, 100.0, 25.0), |ui| ui.add_enabled(connected && !running, egui::Button::new("Disconnect")));
        if tip(r, "Disconnect client from server").clicked() {
            self.disconnect();
        }
        let r = place(ui, at(20.0, 86.0, 252.0, 16.0), |ui| ui.label(peer_text(peer)));
        tip(r, "Connection partner's IP and remote port used");

        // ---- Register type and UID ----
        group(ui, at(8.0, 112.0, 272.0, 57.0), "Register type and UID");
        let r = place(ui, at(18.0, 134.0, 64.0, 20.0), |ui| ui.radio_value(&mut self.bank, Bank::Input, "Input"));
        tip(r, "Use Input registers");
        let r = place(ui, at(86.0, 134.0, 72.0, 20.0), |ui| ui.radio_value(&mut self.bank, Bank::Holding, "Holding"));
        tip(r, "Use Holding registers");
        place(ui, at(168.0, 134.0, 32.0, 20.0), |ui| ui.label("UID:"));
        let r = place(ui, at(200.0, 132.0, 70.0, 22.0), |ui| {
            egui::ComboBox::from_id_salt("scanner_uid")
                .width(60.0)
                .selected_text(self.uid.to_string())
                .show_ui(ui, |ui| {
                    for id in 0..=u8::MAX {
                        ui.selectable_value(&mut self.uid, id, id.to_string());
                    }
                })
                .response
        });
        tip(r, "UID (MB slave address) used for connection");

        // ---- Range ----
        group(ui, at(8.0, 175.0, 272.0, 90.0), "Range");
        let idle = !running;
        let spin_row = |ui: &mut egui::Ui, y: f32, label: &str, value: &mut u32, max: u32, hint: &str| {
            place(ui, at(18.0, y + 2.0, 70.0, 18.0), |ui| ui.label(label));
            let r = place(ui, at(92.0, y, 56.0, 21.0), |ui| {
                ui.add_enabled(idle, egui::DragValue::new(value).range(0..=max).speed(1.0))
            });
            tip(r, hint);
            match spinner(ui, at(150.0, y, 14.0, 21.0), idle) {
                Some(true) => *value = value.saturating_add(1).min(max),
                Some(false) => *value = value.saturating_sub(1),
                None => {}
            }
        };
        spin_row(ui, 191.0, "From:", &mut self.from, 65535, "Starting address of polling range");
        spin_row(ui, 211.0, "To:", &mut self.to, 65535, "End address of polling range");
        spin_row(ui, 233.0, "Timeout [ms]:", &mut self.timeout_ms, 60000, "Maximum reply wait time");
        let frac = progress_fraction(summary.scanned, total);
        place(ui, at(170.0, 238.0, 95.0, 13.0), |ui| {
            ui.add(egui::ProgressBar::new(frac).desired_height(13.0).desired_width(95.0))
        });

        // ---- Misc ----
        group(ui, at(8.0, 271.0, 145.0, 121.0), "Misc");
        let r = place(ui, at(16.0, 292.0, 134.0, 20.0), |ui| {
            ui.add_enabled(idle, egui::Checkbox::new(&mut self.offset, "Use address offset"))
        });
        tip(r, "Address offset (-1)");
        let r = place(ui, at(16.0, 320.0, 134.0, 20.0), |ui| {
            ui.add_enabled(idle, egui::Checkbox::new(&mut self.failed_only, "List failed polls only"))
        });
        tip(r, "Only failed polls are added to Results-list");
        let r = place(ui, at(16.0, 348.0, 134.0, 20.0), |ui| {
            ui.add_enabled(idle, egui::Checkbox::new(&mut self.copy_to_regs, "Copy value to register"))
        });
        tip(r, "If poll succeed, copy value to corresponding register");

        // ---- Start / Stop ----
        let r = place(ui, at(186.0, 288.0, 79.0, 25.0), |ui| ui.add_enabled(connected && !running, egui::Button::new("Start")));
        if tip(r, "Start scan").clicked() {
            self.start_scan(regs);
        }
        let r = place(ui, at(186.0, 343.0, 79.0, 25.0), |ui| ui.add_enabled(running, egui::Button::new("Stop")));
        if tip(r, "Interrupt scan").clicked() {
            self.shared.stop.store(true, Ordering::SeqCst);
        }

        // ---- Results ----
        group(ui, at(286.0, 9.0, 230.0, 383.0), "Results");
        let list_rect = at(292.0, 28.0, 218.0, 358.0);
        ui.painter().rect_filled(list_rect, 0.0, Color32::WHITE);
        ui.painter().rect_stroke(list_rect, 0.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        let list_resp = ui.interact(list_rect, ui.id().with("scanner_list"), egui::Sense::click());
        if list_resp.double_clicked() {
            self.hscroll = !self.hscroll;
        }
        let list_resp = tip(list_resp, "Double-click for horizontal scrollbar");
        let mut clear = false;
        let mut copy = false;
        list_resp.context_menu(|ui| {
            if ui.button("Copy to Clipboard").clicked() {
                copy = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Clear").clicked() {
                clear = true;
                ui.close();
            }
        });
        if copy {
            ui.ctx().copy_text(clipboard_text(&lines));
        }
        if clear {
            lock(&self.shared.state).lines.clear();
        }
        let hscroll = self.hscroll;
        place_raw(ui, list_rect.shrink(2.0), |ui| {
            egui::ScrollArea::new([hscroll, true]).auto_shrink([false, false]).show_rows(ui, 16.0, lines.len(), |ui, range| {
                for line in lines.get(range).unwrap_or_default() {
                    let text = egui::RichText::new(line).monospace();
                    ui.add(egui::Label::new(text).wrap_mode(if hscroll { egui::TextWrapMode::Extend } else { egui::TextWrapMode::Truncate }));
                }
            });
        });

        // ---- Status bar ----
        let bar = at(0.0, 392.0, CLIENT.x, 24.0);
        ui.painter().line_segment([bar.left_top(), bar.right_top()], ui.visuals().widgets.noninteractive.bg_stroke);
        let status = status_text(summary, started, elapsed, &note, running);
        ui.painter().text(
            bar.left_center() + Vec2::new(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            status,
            egui::FontId::proportional(13.0),
            ui.visuals().text_color(),
        );
    }
}

/// Drop the scanner's connection (closing the socket).
fn drop_connection(shared: &Shared) {
    let mut conn = lock(&shared.conn);
    if let Some(s) = conn.stream.take() {
        s.shutdown(std::net::Shutdown::Both).ok();
    }
    conn.peer = None;
}

/// Run `add` with its widgets confined to `rect`, sized to fill it.
fn place<R>(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    place_raw(ui, rect, |ui| {
        ui.spacing_mut().interact_size = rect.size();
        ui.spacing_mut().combo_width = rect.width();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add).inner
    })
}

/// Like [`place`] but leaves the child's spacing alone.
fn place_raw<R>(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner
}

/// An etched group box with its caption on the top edge.
#[allow(clippy::arithmetic_side_effects, reason = "egui geometry on f32 vectors")]
fn group(ui: &mut egui::Ui, rect: Rect, title: &str) {
    let painter = ui.painter();
    let frame = Rect::from_min_max(rect.min + Vec2::new(0.0, 7.0), rect.max);
    painter.rect_stroke(frame, 0.0, egui::Stroke::new(1.0, Color32::from_rgb(0xA0, 0xA0, 0xA0)), egui::StrokeKind::Inside);
    painter.rect_stroke(frame.shrink(1.0), 0.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
    let galley = painter.layout_no_wrap(title.to_owned(), egui::FontId::proportional(13.0), ui.visuals().text_color());
    let pos = rect.min + Vec2::new(8.0, 0.0);
    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(2.0, 0.0)), 0.0, ui.visuals().window_fill);
    painter.galley(pos, galley, Color32::BLACK);
}

/// Up/down arrow pair filling `rect`. `Some(true)` = up clicked, `Some(false)` = down.
#[allow(clippy::arithmetic_side_effects, reason = "egui geometry on f32 vectors")]
fn spinner(ui: &mut egui::Ui, rect: Rect, enabled: bool) -> Option<bool> {
    let half = rect.height() / 2.0;
    let mut out = None;
    for (up, r) in [
        (true, Rect::from_min_size(rect.min, Vec2::new(rect.width(), half))),
        (false, Rect::from_min_size(rect.min + Vec2::new(0.0, half), Vec2::new(rect.width(), half))),
    ] {
        let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
        let resp = ui.interact(r, ui.id().with(("spin", rect.min.x as i32, rect.min.y as i32, up)), sense);
        let fill = if resp.is_pointer_button_down_on() {
            ui.visuals().widgets.active.bg_fill
        } else if resp.hovered() && enabled {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        let stroke = ui.visuals().widgets.inactive.bg_stroke;
        ui.painter().rect_filled(r, 0.0, fill);
        ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        let c = r.center();
        let s = 2.5;
        let pts = if up {
            vec![Pos2::new(c.x - s, c.y + 1.0), Pos2::new(c.x + s, c.y + 1.0), Pos2::new(c.x, c.y - 2.0)]
        } else {
            vec![Pos2::new(c.x - s, c.y - 1.0), Pos2::new(c.x + s, c.y - 1.0), Pos2::new(c.x, c.y + 2.0)]
        };
        let color = if enabled { Color32::BLACK } else { Color32::GRAY };
        ui.painter().add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
        if resp.clicked() {
            out = Some(up);
        }
    }
    out
}

struct Job {
    stream: TcpStream,
    uid: u8,
    bank: Bank,
    /// Wire addresses (offset already applied).
    start: u16,
    end: u16,
    timeout: Duration,
    offset: bool,
    failed_only: bool,
    regs: Option<SharedRegs>,
}

/// Run one sweep; returns a note for the status line (empty on a clean finish).
fn run(shared: &Shared, job: Job) -> String {
    let mut stream = job.stream;
    stream.set_read_timeout(Some(job.timeout)).ok();
    let mut scan = Scanner::new(job.uid, job.bank, job.start, job.end);
    let mut last_repaint = Instant::now();
    let mut note = String::new();
    while let Some((addr, query)) = scan.next_query() {
        if shared.stop.load(Ordering::SeqCst) {
            note = "Stopped".into();
            break;
        }
        let response = stream.write_all(&query).ok().and_then(|()| read_frame(&mut stream).ok());
        let broken = response.is_none();
        let result = scan.record(addr, response.as_deref());
        if let (ScanResult::Value(v), Some(regs)) = (result, &job.regs) {
            lock(regs).set(job.bank, addr, v);
        }
        {
            let mut st = lock(&shared.state);
            if list_it(result, job.failed_only) {
                st.lines.push(format_line(display_addr(addr, job.offset), result));
            }
            st.summary = scan.summary();
        }
        if last_repaint.elapsed() >= REPAINT_GAP {
            shared.ctx.request_repaint();
            last_repaint = Instant::now();
        }
        if broken {
            // A timeout leaves the stream out of step with the server; the
            // connection cannot be reused.
            drop_connection(shared);
            note = format!("Stopped, no usable reply for address {}; connection closed", display_addr(addr, job.offset));
            break;
        }
    }
    note
}

/// The wire range for the addresses typed in From/To. With the address offset
/// the typed numbers are one-based, so each is sent minus one. `None` for an
/// empty or unrepresentable range.
fn wire_range(from: u32, to: u32, offset: bool) -> Option<(u16, u16)> {
    let shift = u32::from(offset);
    let start = u16::try_from(from.checked_sub(shift)?).ok()?;
    let end = u16::try_from(to.checked_sub(shift)?).ok()?;
    (start <= end).then_some((start, end))
}

/// The address shown for wire address `addr`.
fn display_addr(addr: u16, offset: bool) -> u32 {
    u32::from(addr).saturating_add(u32::from(offset))
}

/// Whether a poll result goes into the list.
fn list_it(result: ScanResult, failed_only: bool) -> bool {
    !failed_only || !matches!(result, ScanResult::Value(_))
}

fn format_line(addr: u32, result: ScanResult) -> String {
    match result {
        ScanResult::Value(v) => format!("{addr:>5}  {v:>5}  {v:04X}"),
        ScanResult::Exception(c) => format!("{addr:>5}  exception {c}"),
        ScanResult::NoReply => format!("{addr:>5}  no reply"),
    }
}

fn clipboard_text(lines: &[String]) -> String {
    let mut out = lines.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

fn progress_fraction(scanned: usize, total: usize) -> f32 {
    if total == 0 { 0.0 } else { (scanned as f32 / total as f32).clamp(0.0, 1.0) }
}

fn peer_text(peer: Option<SocketAddr>) -> String {
    match peer {
        Some(a) => format!("IP: {}  Port: {}", a.ip(), a.port()),
        None => "IP: --  Port: --".into(),
    }
}

fn status_text(s: ScanSummary, started: Option<Instant>, elapsed: Option<Duration>, note: &str, running: bool) -> String {
    let mut line = if started.is_some() || s.scanned > 0 {
        let dur = elapsed.or_else(|| started.map(|t| t.elapsed())).unwrap_or_default();
        let what = if running { "scanning" } else { "done" };
        format!("{} registers scanned, {} succeeded, {} failed, total duration {dur:.2?} ({what})", s.scanned, s.succeeded, s.failed)
    } else {
        String::new()
    };
    if !note.is_empty() {
        line = if line.is_empty() { note.to_owned() } else { format!("{note}: {line}") };
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_without_offset_is_sent_as_typed() {
        assert_eq!(wire_range(0, 99, false), Some((0, 99)));
        assert_eq!(wire_range(5, 5, false), Some((5, 5)));
    }

    #[test]
    fn range_with_offset_is_one_based() {
        assert_eq!(wire_range(1, 100, true), Some((0, 99)));
        assert_eq!(wire_range(0, 10, true), None);
    }

    #[test]
    fn reversed_or_oversized_range_is_rejected() {
        assert_eq!(wire_range(10, 9, false), None);
        assert_eq!(wire_range(0, 70000, false), None);
    }

    #[test]
    fn display_address_adds_the_offset_back() {
        assert_eq!(display_addr(0, true), 1);
        assert_eq!(display_addr(65535, true), 65536);
        assert_eq!(display_addr(7, false), 7);
    }

    #[test]
    fn failed_only_hides_answers() {
        assert!(list_it(ScanResult::Value(1), false));
        assert!(!list_it(ScanResult::Value(1), true));
        assert!(list_it(ScanResult::Exception(2), true));
        assert!(list_it(ScanResult::NoReply, true));
    }

    #[test]
    fn lines_show_value_in_decimal_and_hex() {
        assert_eq!(format_line(3, ScanResult::Value(255)), "    3    255  00FF");
        assert_eq!(format_line(3, ScanResult::Exception(2)), "    3  exception 2");
        assert_eq!(format_line(3, ScanResult::NoReply), "    3  no reply");
    }

    #[test]
    fn clipboard_text_is_crlf_lines() {
        assert_eq!(clipboard_text(&[]), "");
        assert_eq!(clipboard_text(&["a".into(), "b".into()]), "a\r\nb\r\n");
    }

    #[test]
    fn progress_is_clamped_and_empty_safe() {
        assert!(progress_fraction(0, 0).abs() < f32::EPSILON);
        assert!((progress_fraction(5, 10) - 0.5).abs() < f32::EPSILON);
        assert!((progress_fraction(20, 10) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn partner_text_shows_dashes_until_connected() {
        assert_eq!(peer_text(None), "IP: --  Port: --");
        assert_eq!(peer_text(Some("10.0.0.2:502".parse().unwrap())), "IP: 10.0.0.2  Port: 502");
    }
}
