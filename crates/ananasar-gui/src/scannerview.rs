//! Register scanner window: sweeps an address range with single-register
//! reads over its own connection and tabulates what each address answers.
//! The sweep runs on a worker thread; the UI is only woken when it changes.

use ananasar_core::registers::Bank;
use ananasar_core::scanner::{ScanResult, ScanSummary, Scanner};
use ananasar_core::sync::lock;
use ananasar_net::read_frame;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Minimum gap between repaint requests while a sweep is running.
const REPAINT_GAP: Duration = Duration::from_millis(50);

#[derive(Default)]
struct State {
    running: bool,
    results: Vec<(u16, ScanResult)>,
    summary: ScanSummary,
    total: usize,
    started: Option<Instant>,
    /// Set when the sweep ends; freezes the displayed duration.
    elapsed: Option<Duration>,
    note: String,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
    /// Wakes the UI when the worker changes anything it shows.
    ctx: egui::Context,
}

pub struct ScannerView {
    shared: Arc<Shared>,
    host: String,
    uid: String,
    bank: Bank,
    start: u16,
    end: u16,
    timeout_ms: u64,
    only_ok: bool,
}

impl ScannerView {
    pub fn new(default_host: String, ctx: egui::Context) -> Self {
        Self {
            shared: Arc::new(Shared { state: Mutex::new(State::default()), stop: AtomicBool::new(false), ctx }),
            host: default_host,
            uid: "1".into(),
            bank: Bank::Holding,
            start: 0,
            end: 99,
            timeout_ms: 1000,
            only_ok: false,
        }
    }

    fn start_scan(&self) {
        let Ok(uid) = self.uid.trim().parse::<u8>() else {
            lock(&self.shared.state).note = "UID must be 0-255".into();
            return;
        };
        let total = if self.end >= self.start { usize::from(self.end.saturating_sub(self.start)).saturating_add(1) } else { 0 };
        {
            let mut st = lock(&self.shared.state);
            *st = State { running: true, total, started: Some(Instant::now()), ..State::default() };
        }
        self.shared.stop.store(false, Ordering::SeqCst);
        let job = Job {
            host: self.host.trim().to_owned(),
            uid,
            bank: self.bank,
            start: self.start,
            end: self.end,
            timeout: Duration::from_millis(self.timeout_ms.max(1)),
        };
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let note = run(&shared, &job);
            let mut st = lock(&shared.state);
            st.running = false;
            st.elapsed = st.started.map(|t| t.elapsed());
            st.note = note;
            drop(st);
            shared.ctx.request_repaint();
        });
        self.shared.ctx.request_repaint();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let running = lock(&self.shared.state).running;
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal(|ui| {
                ui.label("Server:");
                ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(150.0));
                ui.label("UID:");
                ui.add(egui::TextEdit::singleline(&mut self.uid).desired_width(32.0));
                ui.label("Timeout (ms):");
                ui.add(egui::DragValue::new(&mut self.timeout_ms).range(1..=60000).speed(10.0));
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
                ui.radio_value(&mut self.bank, Bank::Input, "Input");
                ui.label("Start:");
                ui.add(egui::DragValue::new(&mut self.start).range(0..=65535));
                ui.label("End:");
                ui.add(egui::DragValue::new(&mut self.end).range(0..=65535));
            });
        });
        ui.horizontal(|ui| {
            if running {
                if ui.button("Stop").clicked() {
                    self.shared.stop.store(true, Ordering::SeqCst);
                }
            } else if ui.button("Start").clicked() {
                self.start_scan();
            }
            ui.checkbox(&mut self.only_ok, "Only show answering registers");
        });

        let st = lock(&self.shared.state);
        if st.running {
            let frac = if st.total == 0 { 1.0 } else { st.summary.scanned as f32 / st.total as f32 };
            ui.add(egui::ProgressBar::new(frac).show_percentage());
        }
        ui.separator();

        let rows: Vec<usize> = st
            .results
            .iter()
            .enumerate()
            .filter(|(_, (_, r))| !self.only_ok || matches!(r, ScanResult::Value(_)))
            .map(|(i, _)| i)
            .collect();
        TableBuilder::new(ui)
            .striped(true)
            .max_scroll_height(300.0)
            .column(Column::exact(70.0))
            .column(Column::exact(70.0))
            .column(Column::exact(70.0))
            .column(Column::remainder())
            .header(20.0, |mut h| {
                h.col(|ui| { ui.strong("Address"); });
                h.col(|ui| { ui.strong("Decimal"); });
                h.col(|ui| { ui.strong("Hex"); });
                h.col(|ui| { ui.strong("Result"); });
            })
            .body(|body| {
                body.rows(18.0, rows.len(), |mut row| {
                    let Some(&(addr, res)) = rows.get(row.index()).and_then(|&i| st.results.get(i)) else { return };
                    row.col(|ui| { ui.monospace(addr.to_string()); });
                    match res {
                        ScanResult::Value(v) => {
                            row.col(|ui| { ui.monospace(v.to_string()); });
                            row.col(|ui| { ui.monospace(format!("{v:04X}")); });
                            row.col(|ui| { ui.monospace("ok"); });
                        }
                        ScanResult::Exception(c) => {
                            row.col(|_| {});
                            row.col(|_| {});
                            row.col(|ui| { ui.monospace(format!("exception {c}")); });
                        }
                        ScanResult::NoReply => {
                            row.col(|_| {});
                            row.col(|_| {});
                            row.col(|ui| { ui.monospace("no reply"); });
                        }
                    }
                });
            });

        ui.separator();
        let s = st.summary;
        let dur = st.elapsed.or_else(|| st.started.map(|t| t.elapsed())).unwrap_or_default();
        let mut line = format!(
            "{} registers scanned, {} succeeded, {} failed, total duration {dur:.2?}",
            s.scanned, s.succeeded, s.failed
        );
        if !st.note.is_empty() {
            line = format!("{}: {line}", st.note);
        }
        ui.label(line);
    }
}

struct Job {
    host: String,
    uid: u8,
    bank: Bank,
    start: u16,
    end: u16,
    timeout: Duration,
}

/// Run one sweep; returns a note for the status line (empty on a clean finish).
fn run(shared: &Shared, job: &Job) -> String {
    let stream = job
        .host
        .to_socket_addrs()
        .map_err(|e| e.to_string())
        .and_then(|mut a| a.next().ok_or_else(|| "no address".to_owned()))
        .and_then(|a| TcpStream::connect_timeout(&a, job.timeout.max(Duration::from_secs(1))).map_err(|e| e.to_string()));
    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => return format!("connect to {} failed: {e}", job.host),
    };
    stream.set_read_timeout(Some(job.timeout)).ok();
    let mut scan = Scanner::new(job.uid, job.bank, job.start, job.end);
    let mut last_repaint = Instant::now();
    let mut note = String::new();
    while let Some((addr, query)) = scan.next_query() {
        if shared.stop.load(Ordering::SeqCst) {
            note = "stopped".into();
            break;
        }
        let response = stream.write_all(&query).ok().and_then(|()| read_frame(&mut stream).ok());
        let broken = response.is_none();
        let result = scan.record(addr, response.as_deref());
        {
            let mut st = lock(&shared.state);
            st.results.push((addr, result));
            st.summary = scan.summary();
        }
        if last_repaint.elapsed() >= REPAINT_GAP {
            shared.ctx.request_repaint();
            last_repaint = Instant::now();
        }
        if broken {
            // A timeout leaves the stream out of step with the server; stop.
            note = format!("stopped, no usable reply for address {addr}");
            break;
        }
    }
    note
}
