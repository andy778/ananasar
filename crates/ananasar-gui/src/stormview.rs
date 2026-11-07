//! Storm test window (Tools > Storm Test…): the GUI for the `ananasar-storm`
//! library, the same engine as `ananasar storm`. The original program has no
//! such feature, so this window is an Ananasar addition.
//!
//! The test runs on its own threads (one per connection, plus the one that
//! drives them); the window only reads a snapshot once a second, so it stays
//! responsive and Stop works at once. The safety rules are the CLI's: the
//! authorisation box is required for exactly what `--i-am-authorised` is
//! required for, because both ask [`Config::authorisation_reasons`], and the
//! library checks the rules again before it sends anything.
#![allow(clippy::arithmetic_side_effects, reason = "egui geometry on f32 vectors; the integer maths below saturates")]

use crate::{dialog, theme};
use ananasar_core::sync::lock;
use ananasar_net::{parse_endpoint, split_host_port};
use ananasar_storm::config::MIX_FUNCTIONS;
use ananasar_storm::{Config, FaultKind, Histogram, Report, SAFETY_NOTICE, Tick, fresh_seed, run_with};
use eframe::egui::{self, Color32, Vec2};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Client size of the window, in points.
const SIZE: [f32; 2] = [700.0, 800.0];
/// Per-second bars kept in the timeline plot.
const PLOT_SECONDS: usize = 120;

const SCOPE_NOTICE: &str = "Storm traffic can slow, confuse or crash a device. The defaults are modest (4 connections x 10 requests/s for 10 s, reads only). \
     This works at the Modbus/TCP level; it does not replace the link-level broadcast, multicast and unicast storm tests used for classification.";

/// The text boxes of the window. Everything is kept as typed and only parsed
/// when a run starts, so a half-typed number never changes anything.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub target: String,
    pub unit_id: String,
    pub connections: String,
    pub rate: String,
    pub ramp: bool,
    pub ramp_to: String,
    /// Blank: the ramp lasts the whole run, as with `--ramp-secs` left out.
    pub ramp_secs: String,
    pub duration: String,
    pub timeout_ms: String,
    /// Blank: a seed from the clock, shown and recorded in the report.
    pub seed: String,
    /// Weights for FC3, FC4, FC6, FC16, FC22 and FC23.
    pub weights: [String; 6],
    pub addr_start: String,
    pub addr_end: String,
    pub qty_min: String,
    pub qty_max: String,
    /// Percent per fault kind, in [`FaultKind::ALL`] order; blank is 0.
    pub faults: [String; 7],
    pub coalesce_count: String,
    pub split_delay_ms: String,
    pub fault_wait_ms: String,
    pub churn: bool,
    pub churn_interval_ms: String,
    pub churn_request: bool,
    pub half_open: String,
    /// "I am authorised to test this equipment": the CLI's `--i-am-authorised`.
    pub authorised: bool,
}

impl Default for Form {
    /// The CLI defaults. There is no target: nothing is sent until one is typed.
    fn default() -> Self {
        let c = Config::new("", 0);
        let s = |v: &dyn ToString| v.to_string();
        Self {
            target: String::new(),
            unit_id: s(&c.unit_id),
            connections: s(&c.connections),
            rate: s(&c.rate),
            ramp: false,
            ramp_to: "100".to_owned(),
            ramp_secs: String::new(),
            duration: s(&c.duration.as_secs()),
            timeout_ms: s(&c.reply_timeout.as_millis()),
            seed: String::new(),
            weights: c.mix.weights.map(|w| w.to_string()),
            addr_start: s(&c.mix.addr_start),
            addr_end: s(&c.mix.addr_end),
            qty_min: s(&c.mix.qty_min),
            qty_max: s(&c.mix.qty_max),
            faults: FaultKind::ALL.map(|_| "0".to_owned()),
            coalesce_count: s(&c.coalesce_count),
            split_delay_ms: s(&c.split_delay.as_millis()),
            fault_wait_ms: s(&c.fault_wait.as_millis()),
            churn: false,
            churn_interval_ms: s(&c.churn_interval.as_millis()),
            churn_request: false,
            half_open: s(&c.half_open),
            authorised: false,
        }
    }
}

fn num<T: FromStr>(name: &str, text: &str) -> Result<T, String> {
    text.trim().parse().map_err(|_| format!("{name} is not a valid number"))
}

/// A number where a blank box means 0.
fn num_or_zero<T: FromStr + Default>(name: &str, text: &str) -> Result<T, String> {
    if text.trim().is_empty() { Ok(T::default()) } else { num(name, text) }
}

fn secs(name: &str, v: f64) -> Result<Duration, String> {
    Duration::try_from_secs_f64(v).map_err(|_| format!("{name} must be a non-negative number of seconds"))
}

impl Form {
    /// The run the boxes describe, with the seed filled in. The same checks
    /// as the CLI's argument handling; [`start_check`] adds the safety rules.
    pub fn to_config(&self) -> Result<Config, String> {
        let (host, port) = split_host_port(&self.target);
        let port = port.ok_or("the target must be host:port, for example 192.168.1.50:502")?;
        let (host, port) = parse_endpoint(host, port)?;
        let mut c = Config::new(&host, port);
        c.unit_id = num("unit id", &self.unit_id)?;
        c.connections = num("connections", &self.connections)?;
        c.rate = num("rate", &self.rate)?;
        c.duration = secs("duration", num("duration", &self.duration)?)?;
        if self.ramp {
            c.ramp_to = Some(num("ramp rate", &self.ramp_to)?);
            c.ramp_duration = if self.ramp_secs.trim().is_empty() { c.duration } else { secs("ramp duration", num("ramp duration", &self.ramp_secs)?)? };
        }
        c.reply_timeout = Duration::from_millis(num("reply timeout", &self.timeout_ms)?);
        c.seed = if self.seed.trim().is_empty() { fresh_seed() } else { num("seed", &self.seed)? };
        c.mix.weights = [0; 6];
        for (&fc, text) in MIX_FUNCTIONS.iter().zip(&self.weights) {
            c.mix.set_weight(fc, num_or_zero(&format!("weight of FC{fc}"), text)?)?;
        }
        c.mix.addr_start = num("address start", &self.addr_start)?;
        c.mix.addr_end = num("address end", &self.addr_end)?;
        c.mix.qty_min = num("quantity min", &self.qty_min)?;
        c.mix.qty_max = num("quantity max", &self.qty_max)?;
        for (kind, text) in FaultKind::ALL.into_iter().zip(&self.faults) {
            c.faults.set(kind, num_or_zero(&format!("{} fault", kind.name()), text)?);
        }
        c.coalesce_count = num("coalesce count", &self.coalesce_count)?;
        c.split_delay = Duration::from_millis(num("split delay", &self.split_delay_ms)?);
        c.fault_wait = Duration::from_millis(num("fault wait", &self.fault_wait_ms)?);
        c.churn = self.churn;
        c.churn_interval = Duration::from_millis(num("churn interval", &self.churn_interval_ms)?);
        c.churn_request = self.churn && self.churn_request;
        c.half_open = num_or_zero("half-open connections", &self.half_open)?;
        c.authorised = self.authorised;
        Ok(c)
    }
}

/// Whether `cfg` may start: valid, and authorised when the rules say it must be.
/// The same rules the library enforces in [`Config::check`]; this only words
/// the refusal for the box in the window instead of the CLI flag.
pub fn start_check(cfg: &Config) -> Result<(), String> {
    cfg.validate()?;
    let why = cfg.authorisation_reasons();
    if !why.is_empty() && !cfg.authorised {
        return Err(format!(
            "This run needs \"I am authorised to test this equipment\" ticked: {}. It can disturb or disable the target.",
            why.join(", ")
        ));
    }
    Ok(())
}

/// Counters summed over the per-second ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub sent: u64,
    pub replies: u64,
    pub exceptions: u64,
    pub timeouts: u64,
    pub errors: u64,
    pub mismatches: u64,
    pub faults: u64,
    pub cycles: u64,
}

impl Totals {
    pub fn of(ticks: &[Tick]) -> Self {
        ticks.iter().fold(Self::default(), |a, t| Self {
            sent: a.sent.saturating_add(t.sent),
            replies: a.replies.saturating_add(t.replies),
            exceptions: a.exceptions.saturating_add(t.exceptions),
            timeouts: a.timeouts.saturating_add(t.timeouts),
            errors: a.errors.saturating_add(t.errors),
            mismatches: a.mismatches.saturating_add(t.mismatches),
            faults: a.faults.saturating_add(t.faults),
            cycles: a.cycles.saturating_add(t.cycles),
        })
    }
}

/// Latency figures in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latency {
    pub count: u64,
    pub mean: Option<u64>,
    pub p50: Option<u64>,
    pub p95: Option<u64>,
    pub p99: Option<u64>,
    pub max: Option<u64>,
}

impl Latency {
    pub fn of(h: &Histogram) -> Self {
        Self { count: h.count(), mean: h.mean(), p50: h.percentile(50.0), p95: h.percentile(95.0), p99: h.percentile(99.0), max: h.max() }
    }
}

fn ms(us: Option<u64>) -> String {
    us.map_or_else(|| "-".to_owned(), |u| format!("{:.2} ms", u as f64 / 1000.0))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Running,
    Finished,
}

#[derive(Default)]
struct RunState {
    phase: Phase,
    started: Option<Instant>,
    cfg: Option<Config>,
    ticks: Vec<Tick>,
    latency: Latency,
    report: Option<Report>,
    error: Option<String>,
}

struct Shared {
    state: Mutex<RunState>,
    stop: AtomicBool,
    ctx: egui::Context,
}

/// Marks the run finished when the worker thread ends, however it ends.
struct Finish(Arc<Shared>);

impl Drop for Finish {
    fn drop(&mut self) {
        {
            let mut st = lock(&self.0.state);
            if st.report.is_none() && st.error.is_none() {
                st.error = Some("the test thread ended without a result".to_owned());
            }
            st.phase = Phase::Finished;
        }
        self.0.ctx.request_repaint();
    }
}

pub struct StormView {
    pub form: Form,
    shared: Arc<Shared>,
    /// Folder the report files are written to.
    out_dir: String,
    /// What the last Save said (the files written, or the error).
    save_note: String,
    /// Why the last Start did nothing.
    refusal: Option<String>,
}

impl StormView {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            form: Form::default(),
            shared: Arc::new(Shared { state: Mutex::new(RunState::default()), stop: AtomicBool::new(false), ctx }),
            out_dir: String::new(),
            save_note: String::new(),
            refusal: None,
        }
    }

    fn phase(&self) -> Phase {
        lock(&self.shared.state).phase
    }

    /// Whether a test is sending right now.
    pub fn running(&self) -> bool {
        self.phase() == Phase::Running
    }

    /// Ask a running test to stop; its workers end within one reply timeout.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    /// Start the test the form describes, unless the rules refuse it.
    pub fn start(&mut self) {
        self.refusal = None;
        if self.running() {
            return;
        }
        let cfg = match self.form.to_config().and_then(|c| start_check(&c).map(|()| c)) {
            Ok(c) => c,
            Err(e) => {
                self.refusal = Some(e);
                return;
            }
        };
        // The box is a decision for one run, as the flag is for one command.
        self.form.authorised = false;
        self.save_note.clear();
        self.shared.stop.store(false, Ordering::Relaxed);
        *lock(&self.shared.state) = RunState { phase: Phase::Running, started: Some(Instant::now()), cfg: Some(cfg.clone()), ..RunState::default() };
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let _finish = Finish(shared.clone());
            let result = run_with(&cfg, &shared.stop, &mut |p| {
                {
                    let mut st = lock(&shared.state);
                    st.ticks.push(p.tick.clone());
                    st.latency = Latency::of(p.latency);
                }
                shared.ctx.request_repaint();
            });
            let mut st = lock(&shared.state);
            match result {
                Ok(report) => {
                    st.latency = Latency::of(&report.stats.latency);
                    st.report = Some(report);
                }
                Err(e) => st.error = Some(e),
            }
        });
    }

    /// The content of the window; returns true when Close was pressed.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let running = self.running();
            ui.colored_label(Color32::from_rgb(0x9A, 0x1F, 0x1F), format!("{SAFETY_NOTICE} {SCOPE_NOTICE}"));
            ui.add_space(6.0);
            ui.add_enabled_ui(!running, |ui| self.settings(ui));
            ui.add_space(6.0);
            self.controls(ui, running, &mut close);
            ui.separator();
            self.progress(ui);
            self.report_section(ui);
        });
        close
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let f = &mut self.form;
        ui.horizontal(|ui| {
            field(ui, "Target (host:port):", egui::TextEdit::singleline(&mut f.target).desired_width(220.0).hint_text("192.168.1.50:502"));
            field(ui, "Unit id:", egui::TextEdit::singleline(&mut f.unit_id).desired_width(40.0));
        });
        egui::CollapsingHeader::new("Load").default_open(true).show(ui, |ui| {
            egui::Grid::new("storm_load").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                text_row(ui, "Connections:", &mut f.connections, "Duration (s):", &mut f.duration);
                text_row(ui, "Rate per connection (/s, 0 = unlimited):", &mut f.rate, "Reply timeout (ms):", &mut f.timeout_ms);
                let ramp_to = ui.checkbox(&mut f.ramp, "Ramp to (/s):");
                ui.add_enabled(f.ramp, egui::TextEdit::singleline(&mut f.ramp_to).desired_width(70.0)).labelled_by(ramp_to.id);
                field_enabled(ui, f.ramp, "Ramp time (s, blank = whole run):", egui::TextEdit::singleline(&mut f.ramp_secs).desired_width(70.0));
                ui.end_row();
                field(ui, "Seed (blank = from the clock):", egui::TextEdit::singleline(&mut f.seed).desired_width(120.0));
                ui.end_row();
            });
        });
        egui::CollapsingHeader::new("Request mix").default_open(true).show(ui, |ui| {
            ui.label("Weights per function code. Writes (FC6, FC16, FC22, FC23) change data on the target and are off unless given a weight.");
            ui.horizontal(|ui| {
                for (fc, text) in MIX_FUNCTIONS.iter().zip(&mut f.weights) {
                    field(ui, format!("FC{fc}:"), egui::TextEdit::singleline(text).desired_width(36.0));
                }
            });
            egui::Grid::new("storm_mix").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                text_row(ui, "Address start:", &mut f.addr_start, "Address end:", &mut f.addr_end);
                text_row(ui, "Quantity min:", &mut f.qty_min, "Quantity max:", &mut f.qty_max);
            });
        });
        egui::CollapsingHeader::new("Fault injection (needs authorisation)").show(ui, |ui| {
            ui.label("Percent of requests that get each fault; the total must not exceed 100.");
            egui::Grid::new("storm_faults").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                for (n, (kind, text)) in FaultKind::ALL.into_iter().zip(&mut f.faults).enumerate() {
                    field(ui, format!("{}:", fault_caption(kind)), egui::TextEdit::singleline(text).desired_width(50.0));
                    if n % 2 == 1 {
                        ui.end_row();
                    }
                }
                ui.end_row();
                text_row(ui, "Coalesce count (2-16):", &mut f.coalesce_count, "Split delay (ms):", &mut f.split_delay_ms);
                field(ui, "Fault wait (ms):", egui::TextEdit::singleline(&mut f.fault_wait_ms).desired_width(70.0));
                ui.end_row();
            });
        });
        egui::CollapsingHeader::new("Connection churn and half-open").show(ui, |ui| {
            ui.checkbox(&mut f.churn, "Churn: each connection thread connects and disconnects repeatedly");
            ui.add_enabled_ui(f.churn, |ui| {
                ui.horizontal(|ui| {
                    field(ui, "Pause between cycles (ms, 0 = none, needs authorisation):", egui::TextEdit::singleline(&mut f.churn_interval_ms).desired_width(60.0));
                });
                ui.checkbox(&mut f.churn_request, "Send one request on each connection before closing it");
            });
            ui.horizontal(|ui| {
                field(ui, "Extra idle connections held open:", egui::TextEdit::singleline(&mut f.half_open).desired_width(60.0));
            });
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui, running: bool, close: &mut bool) {
        let parsed = self.form.to_config();
        let can_start = !running && parsed.as_ref().is_ok_and(|c| start_check(c).is_ok());
        ui.add_enabled_ui(!running, |ui| {
            ui.checkbox(&mut self.form.authorised, "I am authorised to test this equipment");
        });
        match &parsed {
            Ok(c) => {
                let why = c.authorisation_reasons();
                if !why.is_empty() {
                    ui.colored_label(Color32::from_rgb(0x9A, 0x1F, 0x1F), format!("Needs the box above: {}.", why.join(", ")));
                }
            }
            Err(e) => {
                ui.weak(format!("Not ready: {e}"));
            }
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(can_start, egui::Button::new("Start")).clicked() {
                self.start();
            }
            let stopping = self.shared.stop.load(Ordering::Relaxed);
            if ui.add_enabled(running && !stopping, egui::Button::new("Stop")).clicked() {
                self.stop();
            }
            if ui.button("Close!").clicked() {
                *close = true;
            }
            if running && stopping {
                ui.label("Stopping…");
            }
        });
        if let Some(r) = &self.refusal {
            ui.colored_label(Color32::from_rgb(0x9A, 0x1F, 0x1F), r);
        }
    }

    fn progress(&self, ui: &mut egui::Ui) {
        let st = lock(&self.shared.state);
        let Some(cfg) = &st.cfg else {
            ui.label("No test has run yet.");
            return;
        };
        let elapsed = match (&st.report, st.started) {
            (Some(r), _) => r.elapsed,
            (None, Some(t)) => t.elapsed(),
            (None, None) => Duration::ZERO,
        };
        let fraction = (elapsed.as_secs_f64() / cfg.duration.as_secs_f64()).clamp(0.0, 1.0);
        let state = match (st.phase, &st.error) {
            (Phase::Running, _) => "running",
            (_, Some(_)) => "failed",
            _ if self.shared.stop.load(Ordering::Relaxed) => "stopped",
            _ => "finished",
        };
        ui.add(egui::ProgressBar::new(fraction as f32).text(format!(
            "{state}: {:.0} s of {:.0} s, {}:{} , seed {}",
            elapsed.as_secs_f64(),
            cfg.duration.as_secs_f64(),
            cfg.host,
            cfg.port,
            cfg.seed
        )));
        if let Some(e) = &st.error {
            ui.colored_label(Color32::from_rgb(0x9A, 0x1F, 0x1F), e);
        }
        let t = Totals::of(&st.ticks);
        let l = st.latency;
        let counters = [
            ("Requests sent", t.sent),
            ("Replies", t.replies),
            ("Exceptions", t.exceptions),
            ("Timeouts", t.timeouts),
            ("Drops (resets, I/O and connect errors)", t.errors),
            ("Mismatched replies", t.mismatches),
            ("Faults injected", t.faults),
            ("Churn cycles", t.cycles),
        ];
        egui::Grid::new("storm_counters").num_columns(4).spacing([16.0, 2.0]).show(ui, |ui| {
            for pair in counters.chunks(2) {
                for (name, value) in pair {
                    ui.label(format!("{name}:"));
                    ui.monospace(value.to_string());
                }
                ui.end_row();
            }
        });
        ui.label(format!(
            "Latency over {} replies: mean {}, median {}, 95th {}, 99th {}, max {}",
            l.count,
            ms(l.mean),
            ms(l.p50),
            ms(l.p95),
            ms(l.p99),
            ms(l.max)
        ));
        ui.add_space(4.0);
        plot(ui, &st.ticks);
        if st.phase == Phase::Running {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
    }

    fn report_section(&mut self, ui: &mut egui::Ui) {
        let (text, have_report) = {
            let st = lock(&self.shared.state);
            (st.report.as_ref().map(Report::text), st.report.is_some())
        };
        let Some(mut text) = text else { return };
        ui.separator();
        ui.horizontal(|ui| {
            field(ui, "Save the report to:", egui::TextEdit::singleline(&mut self.out_dir).desired_width(320.0));
            if ui.button("Browse…").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                self.out_dir = dir.display().to_string();
            }
            if ui.add_enabled(have_report && !self.out_dir.trim().is_empty(), egui::Button::new("Save report")).clicked() {
                self.save();
            }
        });
        if !self.save_note.is_empty() {
            ui.label(&self.save_note);
        }
        egui::ScrollArea::vertical().id_salt("storm_report").max_height(220.0).show(ui, |ui| {
            let report = ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).interactive(false));
            report.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Storm report"));
        });
    }

    /// Write the JSON, CSV and timeline files, named as the CLI names them.
    fn save(&mut self) {
        let report = lock(&self.shared.state).report.clone();
        let Some(report) = report else { return };
        let dir = PathBuf::from(self.out_dir.trim());
        self.save_note = match report.write_to(&dir) {
            Ok(files) => format!("Wrote {}", files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")),
            Err(e) => format!("Cannot write the report to {}: {e}", dir.display()),
        };
    }

    /// The window as a viewport of its own. Closing it stops a running test.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        if !*open {
            return;
        }
        let mut close = false;
        let closed = dialog::show(ctx, "storm-test", "Storm Test", SIZE, 8, &mut |ui| close = self.ui(ui));
        if closed || close {
            *open = false;
            self.stop();
        }
    }
}

fn fault_caption(kind: FaultKind) -> &'static str {
    match kind {
        FaultKind::ShortHeader => "Short header",
        FaultKind::BadProtocol => "Bad protocol id",
        FaultKind::BadLength => "Bad length",
        FaultKind::TruncatedPdu => "Truncated PDU",
        FaultKind::OversizedPdu => "Oversized PDU",
        FaultKind::Coalesced => "Coalesced requests",
        FaultKind::Split => "Split request",
    }
}

/// A label and the text box it names; the link gives the box its accessible name.
fn field(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>, edit: egui::TextEdit<'_>) {
    field_enabled(ui, true, label, edit);
}

fn field_enabled(ui: &mut egui::Ui, enabled: bool, label: impl Into<egui::WidgetText>, edit: egui::TextEdit<'_>) {
    let label = ui.label(label);
    ui.add_enabled(enabled, edit).labelled_by(label.id);
}

fn text_row(ui: &mut egui::Ui, l1: &str, t1: &mut String, l2: &str, t2: &mut String) {
    field(ui, l1, egui::TextEdit::singleline(t1).desired_width(70.0));
    field(ui, l2, egui::TextEdit::singleline(t2).desired_width(70.0));
    ui.end_row();
}

/// Requests per second as bars, the share that timed out, dropped or did not
/// match in red. The last [`PLOT_SECONDS`] seconds are shown.
fn plot(ui: &mut egui::Ui, ticks: &[Tick]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(200.0), 90.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    theme::sunken_border(&painter, rect);
    let shown = ticks.get(ticks.len().saturating_sub(PLOT_SECONDS)..).unwrap_or(&[]);
    let peak = shown.iter().map(|t| t.sent).max().unwrap_or(0).max(1);
    let inner = rect.shrink(3.0);
    let slot = inner.width() / PLOT_SECONDS as f32;
    for (n, t) in shown.iter().enumerate() {
        let x = inner.left() + slot * n as f32;
        let h = |v: u64| inner.height() * (v as f32 / peak as f32);
        let bad = t.timeouts.saturating_add(t.errors).saturating_add(t.mismatches).min(t.sent);
        let top = inner.bottom() - h(t.sent);
        let split = inner.bottom() - h(bad);
        let width = (slot - 1.0).max(1.0);
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + width, split)), 0.0, Color32::from_rgb(0x3A, 0x6E, 0xA5));
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, split), egui::pos2(x + width, inner.bottom())), 0.0, Color32::from_rgb(0xC0, 0x30, 0x30));
    }
    painter.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        format!("requests sent per second, peak {peak} (red: timeouts, drops, mismatches)"),
        egui::FontId::proportional(11.0),
        Color32::from_gray(0x50),
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, reason = "test code: a panic is a failed test")]
mod tests {
    use super::*;

    fn form() -> Form {
        Form { target: "127.0.0.1:502".to_owned(), ..Form::default() }
    }

    fn config(f: &Form) -> Config {
        f.to_config().unwrap()
    }

    #[test]
    fn defaults_are_the_cli_defaults_and_pass_without_authorisation() {
        let c = config(&form());
        let cli = Config::new("127.0.0.1", 502);
        assert_eq!((c.connections, c.rate, c.duration, c.reply_timeout), (4, 10.0, Duration::from_secs(10), Duration::from_secs(1)));
        assert_eq!(c.mix, cli.mix);
        assert!(!c.mix.writes());
        assert!(c.authorisation_reasons().is_empty());
        assert_eq!(start_check(&c), Ok(()));
        assert_eq!(c.check(), Ok(()));
        let cfg = Config { seed: c.seed, ..cli };
        assert_eq!(c, cfg, "the boxes start out as Config::new");
    }

    #[test]
    fn nothing_starts_without_a_valid_target() {
        for bad in ["", "plc.local", "plc.local:0", "plc.local:70000", ":502", "host:x", "a b:502"] {
            let f = Form { target: bad.to_owned(), ..Form::default() };
            assert!(f.to_config().is_err(), "{bad:?}");
        }
        let c = Form { target: " [::1]:1502 ".to_owned(), ..Form::default() }.to_config().unwrap();
        assert_eq!((c.host.as_str(), c.port), ("::1", 1502));
    }

    #[test]
    fn the_gate_asks_for_the_same_things_as_the_cli_flag() {
        type Change = Box<dyn Fn(&mut Form)>;
        let risky: [(&str, Change); 7] = [
            ("rate over 1000/s in total", Box::new(|f| f.rate = "300".to_owned())),
            ("unlimited rate", Box::new(|f| f.rate = "0".to_owned())),
            ("more than 64 connections", Box::new(|f| f.connections = "65".to_owned())),
            ("connections plus half-open", Box::new(|f| {
                f.connections = "60".to_owned();
                f.half_open = "5".to_owned();
            })),
            ("fault injection", Box::new(|f| f.faults[2] = "1".to_owned())),
            ("churn without a pause", Box::new(|f| {
                f.churn = true;
                f.churn_interval_ms = "0".to_owned();
            })),
            ("a ramp above 1000/s", Box::new(|f| {
                f.rate = "1".to_owned();
                f.ramp = true;
                f.ramp_to = "2000".to_owned();
            })),
        ];
        for (what, change) in &risky {
            let mut f = form();
            change(&mut f);
            let refused = start_check(&config(&f)).unwrap_err();
            assert!(refused.contains("authorised"), "{what}: {refused}");
            f.authorised = true;
            assert_eq!(start_check(&config(&f)), Ok(()), "{what}");
        }
        // Churn above 1000 connects/s: 64 threads every 50 ms is 1280/s.
        let mut f = form();
        f.churn = true;
        f.connections = "64".to_owned();
        f.churn_interval_ms = "50".to_owned();
        assert!(start_check(&config(&f)).is_err());
        f.churn_interval_ms = "100".to_owned();
        assert_eq!(start_check(&config(&f)), Ok(()));
        // Exactly at the limits stays allowed, as in the CLI.
        let f = Form { connections: "64".to_owned(), rate: "15.625".to_owned(), ..form() };
        assert_eq!(start_check(&config(&f)), Ok(()));
    }

    #[test]
    fn the_gate_and_the_library_agree() {
        // Whatever the window lets through, the library check lets through too, and the reverse.
        for rate in ["0", "5", "250", "251", "1000"] {
            for authorised in [false, true] {
                let f = Form { rate: rate.to_owned(), authorised, ..form() };
                let c = config(&f);
                assert_eq!(start_check(&c).is_ok(), c.check().is_ok(), "rate {rate}, authorised {authorised}");
            }
        }
    }

    #[test]
    fn writes_are_off_unless_given_a_weight() {
        let mut f = form();
        assert!(!config(&f).mix.writes());
        f.weights[3] = "5".to_owned(); // FC16
        let c = config(&f);
        assert!(c.mix.writes());
        assert_eq!(c.mix.weights, [70, 30, 0, 5, 0, 0]);
        f.weights[0] = String::new();
        f.weights[1] = String::new();
        assert_eq!(config(&f).mix.weights, [0, 0, 0, 5, 0, 0], "a blank weight is 0");
    }

    #[test]
    fn bad_numbers_are_named() {
        let bad = |change: fn(&mut Form)| {
            let mut f = form();
            change(&mut f);
            f.to_config().and_then(|c| start_check(&c).map(|()| c)).unwrap_err()
        };
        assert!(bad(|f| f.connections = "four".to_owned()).contains("connections"));
        assert!(bad(|f| f.connections = "0".to_owned()).contains("connections"));
        assert!(bad(|f| f.rate = "-1".to_owned()).contains("rate"));
        assert!(bad(|f| f.rate = "nan".to_owned()).contains("rate"));
        assert!(bad(|f| f.duration = "0".to_owned()).contains("duration"));
        assert!(bad(|f| f.duration = "-3".to_owned()).contains("duration"));
        assert!(bad(|f| f.addr_start = "70000".to_owned()).contains("address start"));
        assert!(bad(|f| f.addr_start = "200".to_owned()).contains("address"));
        assert!(bad(|f| f.qty_max = "126".to_owned()).contains("quantities"));
        assert!(bad(|f| f.weights = Default::default()).contains("mix"));
        assert!(bad(|f| f.faults[6] = "120".to_owned()).contains("percentage"));
        assert!(bad(|f| f.faults[6] = "x".to_owned()).contains("split fault"));
        assert!(bad(|f| {
            f.faults[1] = "60".to_owned();
            f.faults[2] = "60".to_owned();
            f.authorised = true;
        })
        .contains("100"));
    }

    #[test]
    fn a_ramp_defaults_to_the_whole_run() {
        let mut f = form();
        f.ramp = true;
        f.duration = "30".to_owned();
        let c = config(&f);
        assert_eq!((c.ramp_to, c.ramp_duration), (Some(100.0), Duration::from_secs(30)));
        f.ramp_secs = "5".to_owned();
        assert_eq!(config(&f).ramp_duration, Duration::from_secs(5));
        f.ramp = false;
        assert_eq!(config(&f).ramp_to, None);
    }

    #[test]
    fn a_blank_seed_is_filled_in_and_a_typed_one_is_kept() {
        assert_ne!(config(&form()).seed, 0);
        assert_eq!(config(&Form { seed: "77".to_owned(), ..form() }).seed, 77);
    }

    #[test]
    fn the_window_run_is_reproducible_with_the_cli() {
        let mut f = Form { seed: "9".to_owned(), authorised: true, ..form() };
        f.faults[6] = "5".to_owned();
        f.churn = true;
        let args = config(&f).reproduce_args();
        for want in ["storm 127.0.0.1:502", "--seed 9", "--fault-split 5", "--churn", "--i-am-authorised"] {
            assert!(args.contains(want), "{args} lacks {want}");
        }
    }

    #[test]
    fn totals_add_up_the_ticks() {
        let tick = |sent, timeouts| Tick {
            index: 1,
            unix_ms: 0,
            elapsed_s: 1.0,
            sent,
            replies: sent - timeouts,
            exceptions: 1,
            timeouts,
            errors: 2,
            mismatches: 0,
            faults: 3,
            cycles: 0,
            mean_latency_us: None,
        };
        let t = Totals::of(&[tick(10, 1), tick(20, 4)]);
        assert_eq!((t.sent, t.replies, t.timeouts, t.exceptions, t.errors, t.faults), (30, 25, 5, 2, 4, 6));
        assert_eq!(Totals::of(&[]), Totals::default());
    }

    #[test]
    fn latency_figures_come_from_the_histogram() {
        assert_eq!(Latency::of(&Histogram::default()), Latency::default());
        let mut h = Histogram::default();
        for v in 1..=50 {
            h.record(v);
        }
        let l = Latency::of(&h);
        assert_eq!((l.count, l.p50, l.max), (50, Some(25), Some(50)));
        assert_eq!(ms(Some(1500)), "1.50 ms");
        assert_eq!(ms(None), "-");
    }

    /// Our own server on a free loopback port, stopped on drop.
    struct TestServer {
        port: u16,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn start() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let stop = Arc::new(AtomicBool::new(false));
            let server = ananasar_net::Server {
                regs: Arc::new(Mutex::new(ananasar_core::registers::RegisterFile::new())),
                cfg: Arc::new(Mutex::new(ananasar_core::server::ServerConfig::default())),
                clients: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                stop: stop.clone(),
                notify: Arc::new(|_| {}),
                log: None,
                stats: Arc::default(),
            };
            let thread = std::thread::spawn(move || server.run(&listener));
            Self { port, stop, thread: Some(thread) }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                t.join().ok();
            }
        }
    }

    fn harness(form: Form) -> egui_kittest::Harness<'static, StormView> {
        let mut view = StormView::new(egui::Context::default());
        view.form = form;
        egui_kittest::Harness::builder().with_size(egui::vec2(SIZE[0], SIZE[1])).build_ui_state(
            |ui, view: &mut StormView| {
                view.ui(ui);
            },
            view,
        )
    }

    fn wait_until_idle(h: &mut egui_kittest::Harness<'_, StormView>, limit: Duration) {
        let deadline = Instant::now() + limit;
        while h.state().running() {
            h.step();
            assert!(Instant::now() < deadline, "the test did not finish in {limit:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
        h.run();
    }

    #[test]
    fn the_window_runs_a_short_storm_against_our_server_and_saves_the_report() {
        use egui_kittest::kittest::Queryable;
        let server = TestServer::start();
        let mut h = harness(Form {
            target: format!("127.0.0.1:{}", server.port),
            connections: "2".to_owned(),
            rate: "50".to_owned(),
            duration: "2".to_owned(),
            seed: "5".to_owned(),
            ..Form::default()
        });
        h.run();
        h.get_by_label("Start").click();
        h.step();
        assert!(h.state().running());
        wait_until_idle(&mut h, Duration::from_secs(20));

        // The final partial second is not always a tick, so the run's own totals decide.
        let (stats, ticks, latency, error) = {
            let st = lock(&h.state().shared.state);
            (st.report.as_ref().map(|r| r.stats.clone()), st.ticks.len(), st.latency, st.error.clone())
        };
        assert_eq!(error, None);
        let stats = stats.expect("a finished run has a report");
        assert!(ticks >= 1);
        assert!(stats.sent >= 100, "{stats:?}");
        assert_eq!(stats.sent, stats.ok, "our server answers every read: {stats:?}");
        assert_eq!(stats.sent, stats.accounted(), "{stats:?}");
        assert_eq!((stats.timeouts, stats.resets, stats.io_errors, stats.mismatch_total(), stats.fault_total()), (0, 0, 0, 0, 0));
        assert!(latency.count > 0 && latency.p95.is_some());
        assert!(!h.state().form.authorised, "the box is for one run");
        h.get_by_label("Save report");

        let dir = std::env::temp_dir().join(format!("ananasar-storm-gui-{}", std::process::id()));
        h.state_mut().out_dir = dir.display().to_string();
        h.state_mut().save();
        let note = h.state().save_note.clone();
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        std::fs::remove_dir_all(&dir).ok();
        assert!(note.starts_with("Wrote "), "{note}");
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().any(|n| n.ends_with(".json")) && names.iter().any(|n| n.ends_with(".timeline.csv")), "{names:?}");
    }

    #[test]
    fn stop_ends_a_long_run_quickly() {
        use egui_kittest::kittest::Queryable;
        let server = TestServer::start();
        let mut h = harness(Form {
            target: format!("127.0.0.1:{}", server.port),
            connections: "1".to_owned(),
            duration: "60".to_owned(),
            ..Form::default()
        });
        h.run();
        h.get_by_label("Start").click();
        h.step();
        assert!(h.state().running());
        h.get_by_label("Stop").click();
        wait_until_idle(&mut h, Duration::from_secs(5));
        assert!(!h.state().running());
        let st = lock(&h.state().shared.state);
        assert!(st.report.as_ref().is_some_and(|r| r.elapsed < Duration::from_secs(5)));
    }

    #[test]
    fn a_risky_run_is_refused_until_the_box_is_ticked() {
        use egui_kittest::kittest::Queryable;
        let server = TestServer::start();
        let mut form = Form { target: format!("127.0.0.1:{}", server.port), duration: "1".to_owned(), ..Form::default() };
        form.faults[6] = "100".to_owned();
        let mut h = harness(form);
        h.run();
        // The button is disabled, and starting by other means is refused too.
        h.state_mut().start();
        assert!(!h.state().running());
        assert!(h.state().refusal.as_ref().is_some_and(|r| r.contains("authorised")), "{:?}", h.state().refusal);
        assert!(lock(&h.state().shared.state).cfg.is_none(), "nothing may be sent");
        h.run();
        h.get_by_label("I am authorised to test this equipment").click();
        h.run();
        assert!(h.state().form.authorised);
        h.get_by_label("Start").click();
        h.step();
        wait_until_idle(&mut h, Duration::from_secs(20));
        let st = lock(&h.state().shared.state);
        assert!(st.error.is_none(), "{:?}", st.error);
        let report = st.report.as_ref().expect("a finished run has a report");
        assert!(report.stats.fault_total() > 0, "{:?}", report.stats);
    }

    #[test]
    fn nothing_is_sent_to_a_target_that_was_not_typed() {
        let mut view = StormView::new(egui::Context::default());
        view.start();
        assert!(!view.running() && view.refusal.is_some());
        assert!(lock(&view.shared.state).cfg.is_none());
    }
}
