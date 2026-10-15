//! The end-of-run report: a text summary and JSON, CSV and timeline files.

use crate::config::{Config, MIX_FUNCTIONS};
use crate::fault::FaultKind;
use crate::request::MismatchKind;
use crate::stats::Stats;
use ananasar_core::log::format_timestamp;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Counters for one progress interval (about one second).
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// Interval number, from 1.
    pub index: u64,
    /// Wall clock at the end of the interval, Unix milliseconds.
    pub unix_ms: u64,
    /// Seconds since the run started.
    pub elapsed_s: f64,
    /// Valid requests sent in the interval.
    pub sent: u64,
    /// Replies (ok or exception) received.
    pub replies: u64,
    /// Exception replies among them.
    pub exceptions: u64,
    /// Timeouts.
    pub timeouts: u64,
    /// Resets, I/O errors and failed connects.
    pub errors: u64,
    /// Mismatched replies.
    pub mismatches: u64,
    /// Injected faults.
    pub faults: u64,
    /// Churn cycles.
    pub cycles: u64,
    /// Mean reply latency in the interval, microseconds.
    pub mean_latency_us: Option<u64>,
}

impl Tick {
    /// The one-line progress text.
    pub fn line(&self) -> String {
        let mean = self.mean_latency_us.map_or_else(|| "-".to_owned(), |u| format!("{:.2} ms", us_to_ms(u)));
        format!(
            "[{:>4.0}s] sent {:>6}  replies {:>6}  exc {:>4}  timeout {:>4}  err {:>4}  mismatch {:>3}  fault {:>4}  cycles {:>5}  mean {mean}",
            self.elapsed_s, self.sent, self.replies, self.exceptions, self.timeouts, self.errors, self.mismatches, self.faults, self.cycles
        )
    }
}

fn us_to_ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

/// A finished run.
#[derive(Clone, Debug)]
pub struct Report {
    /// The parameters used.
    pub config: Config,
    /// Tool version.
    pub version: &'static str,
    /// Start time, Unix milliseconds.
    pub started_unix_ms: u64,
    /// Measured run time.
    pub elapsed: Duration,
    /// Merged counters.
    pub stats: Stats,
    /// Per-second timeline.
    pub timeline: Vec<Tick>,
}

#[derive(Clone, Debug)]
enum Value {
    Int(u64),
    Float(f64),
    Text(String),
    Bool(bool),
    Null,
}

type Items = Vec<(String, Value)>;

fn int(items: &mut Items, k: &str, v: u64) {
    items.push((k.to_owned(), Value::Int(v)));
}

fn opt(items: &mut Items, k: &str, v: Option<u64>) {
    items.push((k.to_owned(), v.map_or(Value::Null, Value::Int)));
}

fn text(items: &mut Items, k: &str, v: impl Into<String>) {
    items.push((k.to_owned(), Value::Text(v.into())));
}

fn float(items: &mut Items, k: &str, v: f64) {
    items.push((k.to_owned(), Value::Float(v)));
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Value {
    fn json(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Float(f) if f.is_finite() => format!("{f}"),
            Self::Float(_) | Self::Null => "null".to_owned(),
            Self::Text(s) => json_string(s),
            Self::Bool(b) => b.to_string(),
        }
    }

    fn csv(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Float(f) if f.is_finite() => format!("{f}"),
            Self::Float(_) | Self::Null => String::new(),
            Self::Text(s) if s.contains([',', '"', '\n']) => format!("\"{}\"", s.replace('"', "\"\"")),
            Self::Text(s) => s.clone(),
            Self::Bool(b) => b.to_string(),
        }
    }
}

fn round3(f: f64) -> f64 {
    (f * 1000.0).round() / 1000.0
}

/// Render `items` as a JSON object; a dotted key `a.b` becomes `"a": {"b": ..}`.
fn json_object(items: &[(String, Value)], indent: usize) -> String {
    let pad = "  ".repeat(indent.saturating_add(1));
    let mut groups: Vec<(String, Vec<(String, Value)>)> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (k, v) in items {
        match k.split_once('.') {
            Some((head, rest)) => match groups.iter_mut().find(|(g, _)| g == head) {
                Some((_, inner)) => inner.push((rest.to_owned(), v.clone())),
                None => {
                    groups.push((head.to_owned(), vec![(rest.to_owned(), v.clone())]));
                    lines.push(format!("\u{0}{head}"));
                }
            },
            None => lines.push(format!("{pad}{}: {}", json_string(k), v.json())),
        }
    }
    let rendered: Vec<String> = lines
        .into_iter()
        .map(|l| match l.strip_prefix('\u{0}') {
            Some(head) => {
                let inner = groups.iter().find(|(g, _)| g == head).map(|(_, i)| i.as_slice()).unwrap_or_default();
                format!("{pad}{}: {}", json_string(head), json_object(inner, indent.saturating_add(1)))
            }
            None => l,
        })
        .collect();
    format!("{{\n{}\n{}}}", rendered.join(",\n"), "  ".repeat(indent))
}

impl Report {
    /// Seconds of measured run time.
    pub fn elapsed_s(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    /// Requests per second actually sent, over all connections.
    pub fn achieved_rate(&self) -> f64 {
        let s = self.elapsed_s();
        if s > 0.0 { self.stats.sent as f64 / s } else { 0.0 }
    }

    /// The start time as `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn started_utc(&self) -> String {
        let stamp = format_timestamp(self.started_unix_ms / 1000);
        match stamp.split_once(' ') {
            Some((date, time)) => format!("{date}T{}Z", time.replace('-', ":")),
            None => stamp,
        }
    }

    fn file_stem(&self) -> String {
        format!("ananasar-stress-{}", format_timestamp(self.started_unix_ms / 1000).replace(' ', "_"))
    }

    fn parameters(&self) -> Items {
        let c = &self.config;
        let mut p = Items::new();
        text(&mut p, "tool", "ananasar-stress");
        text(&mut p, "version", self.version);
        text(&mut p, "host", c.host.as_str());
        int(&mut p, "port", u64::from(c.port));
        int(&mut p, "unit_id", u64::from(c.unit_id));
        int(&mut p, "connections", c.connections as u64);
        float(&mut p, "rate_per_connection", c.rate);
        if let Some(end) = c.ramp_to {
            float(&mut p, "ramp_to_rate_per_connection", end);
            float(&mut p, "ramp_seconds", c.ramp_duration.as_secs_f64());
        }
        float(&mut p, "duration_seconds", c.duration.as_secs_f64());
        int(&mut p, "reply_timeout_ms", c.reply_timeout.as_millis() as u64);
        int(&mut p, "seed", c.seed);
        for (fc, w) in MIX_FUNCTIONS.iter().zip(&c.mix.weights) {
            int(&mut p, &format!("mix_weight.fc{fc}"), u64::from(*w));
        }
        int(&mut p, "addr_start", u64::from(c.mix.addr_start));
        int(&mut p, "addr_end", u64::from(c.mix.addr_end));
        int(&mut p, "qty_min", u64::from(c.mix.qty_min));
        int(&mut p, "qty_max", u64::from(c.mix.qty_max));
        for kind in FaultKind::ALL {
            float(&mut p, &format!("fault_percent.{}", kind.name()), c.faults.get(kind));
        }
        int(&mut p, "split_delay_ms", c.split_delay.as_millis() as u64);
        int(&mut p, "coalesce_count", c.coalesce_count as u64);
        int(&mut p, "fault_wait_ms", c.fault_wait.as_millis() as u64);
        p.push(("churn".to_owned(), Value::Bool(c.churn)));
        int(&mut p, "churn_interval_ms", c.churn_interval.as_millis() as u64);
        p.push(("churn_request".to_owned(), Value::Bool(c.churn_request)));
        int(&mut p, "half_open", c.half_open as u64);
        p.push(("authorised_flag_given".to_owned(), Value::Bool(c.authorised)));
        text(&mut p, "reproduce", format!("ananasar {}", c.reproduce_args()));
        p
    }

    fn results(&self) -> Items {
        let s = &self.stats;
        let mut r = Items::new();
        text(&mut r, "started_utc", self.started_utc());
        int(&mut r, "started_unix_ms", self.started_unix_ms);
        float(&mut r, "elapsed_seconds", round3(self.elapsed_s()));
        int(&mut r, "requests_sent", s.sent);
        float(&mut r, "achieved_rate_total", round3(self.achieved_rate()));
        int(&mut r, "ok", s.ok);
        int(&mut r, "exceptions_total", s.exception_total());
        for (code, n) in &s.exceptions {
            int(&mut r, &format!("exceptions.code_{code}"), *n);
        }
        int(&mut r, "timeouts", s.timeouts);
        int(&mut r, "resets", s.resets);
        int(&mut r, "io_errors", s.io_errors);
        int(&mut r, "connect_errors", s.connect_errors);
        int(&mut r, "connects", s.connects);
        int(&mut r, "reconnects", s.reconnects);
        int(&mut r, "mismatches_total", s.mismatch_total());
        for kind in MismatchKind::ALL {
            int(&mut r, &format!("mismatches.{}", kind.name()), s.mismatches.get(&kind).copied().unwrap_or(0));
        }
        int(&mut r, "requests_accounted", s.accounted());
        int(&mut r, "faults_total", s.fault_total());
        for kind in FaultKind::ALL {
            int(&mut r, &format!("faults.{}", kind.name()), s.faults.get(&kind).copied().unwrap_or(0));
        }
        int(&mut r, "fault_replied", s.fault_replied);
        int(&mut r, "fault_connection_closed", s.fault_closed);
        int(&mut r, "fault_ignored", s.fault_silent);
        int(&mut r, "churn_cycles", s.churn_cycles);
        int(&mut r, "half_open_held", s.half_open_held);
        int(&mut r, "half_open_failed", s.half_open_failed);
        int(&mut r, "worker_panics", s.worker_panics);
        Self::latency_items(&mut r, "latency_us", &s.latency);
        Self::latency_items(&mut r, "connect_latency_us", &s.connect_latency);
        r
    }

    fn latency_items(items: &mut Items, name: &str, h: &crate::hist::Histogram) {
        int(items, &format!("{name}.count"), h.count());
        opt(items, &format!("{name}.min"), h.min());
        opt(items, &format!("{name}.mean"), h.mean());
        opt(items, &format!("{name}.p50"), h.percentile(50.0));
        opt(items, &format!("{name}.p95"), h.percentile(95.0));
        opt(items, &format!("{name}.p99"), h.percentile(99.0));
        opt(items, &format!("{name}.max"), h.max());
    }

    /// The summary for the terminal.
    pub fn text(&self) -> String {
        let (c, s) = (&self.config, &self.stats);
        let mut o = String::new();
        let _ = writeln!(o, "Ananasar stress summary (ananasar-stress {})", self.version);
        let _ = writeln!(o, "  target        {}:{} (unit {})", c.host, c.port, c.unit_id);
        let _ = writeln!(
            o,
            "  run           {} connections, {:.1} s measured, started {}, seed {}",
            c.connections,
            self.elapsed_s(),
            self.started_utc(),
            c.seed
        );
        let _ = writeln!(o, "  requests      {} sent, {:.1}/s over all connections", s.sent, self.achieved_rate());
        let exc: Vec<String> = s.exceptions.iter().map(|(code, n)| format!("code {code}: {n}")).collect();
        let exc = if exc.is_empty() { String::new() } else { format!(" ({})", exc.join(", ")) };
        let _ = writeln!(o, "  ok replies    {}", s.ok);
        let _ = writeln!(o, "  exceptions    {}{exc}", s.exception_total());
        let _ = writeln!(o, "  timeouts      {}", s.timeouts);
        let _ = writeln!(
            o,
            "  connections   {} opened, {} reconnects, {} connect errors, {} resets, {} other I/O errors",
            s.connects, s.reconnects, s.connect_errors, s.resets, s.io_errors
        );
        let mism: Vec<String> = MismatchKind::ALL
            .iter()
            .filter_map(|k| s.mismatches.get(k).map(|n| format!("{}: {n}", k.name())))
            .collect();
        let mism = if mism.is_empty() { String::new() } else { format!(" ({})", mism.join(", ")) };
        let _ = writeln!(o, "  mismatches    {}{mism}", s.mismatch_total());
        if s.fault_total() > 0 {
            let kinds: Vec<String> = FaultKind::ALL
                .iter()
                .filter_map(|k| s.faults.get(k).map(|n| format!("{}: {n}", k.name())))
                .collect();
            let _ = writeln!(o, "  faults        {} injected ({})", s.fault_total(), kinds.join(", "));
            let _ = writeln!(
                o,
                "                malformed frames: {} answered, {} closed the connection, {} ignored",
                s.fault_replied, s.fault_closed, s.fault_silent
            );
        }
        if c.churn {
            let _ = writeln!(o, "  churn         {} connect/disconnect cycles", s.churn_cycles);
        }
        if c.half_open > 0 {
            let _ = writeln!(o, "  half-open     {} idle connections held, {} failed", s.half_open_held, s.half_open_failed);
        }
        if s.worker_panics > 0 {
            let _ = writeln!(o, "  WARNING       {} worker thread(s) panicked", s.worker_panics);
        }
        let fmt = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |u| format!("{:.3}", us_to_ms(u)));
        let h = &s.latency;
        let _ = writeln!(
            o,
            "  latency (ms)  min {}  mean {}  p50 {}  p95 {}  p99 {}  max {}  (n={})",
            fmt(h.min()),
            fmt(h.mean()),
            fmt(h.percentile(50.0)),
            fmt(h.percentile(95.0)),
            fmt(h.percentile(99.0)),
            fmt(h.max()),
            h.count()
        );
        if c.churn {
            let h = &s.connect_latency;
            let _ = writeln!(
                o,
                "  connect (ms)  min {}  mean {}  p50 {}  p95 {}  p99 {}  max {}  (n={})",
                fmt(h.min()),
                fmt(h.mean()),
                fmt(h.percentile(50.0)),
                fmt(h.percentile(95.0)),
                fmt(h.percentile(99.0)),
                fmt(h.max()),
                h.count()
            );
        }
        let _ = writeln!(o, "  reproduce     ananasar {}", c.reproduce_args());
        o
    }

    /// The full report as JSON.
    pub fn json(&self) -> String {
        let ticks: Vec<String> = self
            .timeline
            .iter()
            .map(|t| {
                let mut items = Items::new();
                int(&mut items, "second", t.index);
                int(&mut items, "unix_ms", t.unix_ms);
                float(&mut items, "elapsed_s", round3(t.elapsed_s));
                int(&mut items, "sent", t.sent);
                int(&mut items, "replies", t.replies);
                int(&mut items, "exceptions", t.exceptions);
                int(&mut items, "timeouts", t.timeouts);
                int(&mut items, "errors", t.errors);
                int(&mut items, "mismatches", t.mismatches);
                int(&mut items, "faults", t.faults);
                int(&mut items, "churn_cycles", t.cycles);
                opt(&mut items, "mean_latency_us", t.mean_latency_us);
                format!("    {}", json_object(&items, 2).replace('\n', "").replace("  ", ""))
            })
            .collect();
        format!(
            "{{\n  \"parameters\": {},\n  \"results\": {},\n  \"timeline\": [\n{}\n  ]\n}}\n",
            json_object(&self.parameters(), 1),
            json_object(&self.results(), 1),
            ticks.join(",\n")
        )
    }

    /// The parameters and results as `section,key,value` rows.
    pub fn csv(&self) -> String {
        let mut o = String::from("section,key,value\n");
        for (section, items) in [("parameter", self.parameters()), ("result", self.results())] {
            for (k, v) in items {
                let _ = writeln!(o, "{section},{k},{}", v.csv());
            }
        }
        o
    }

    /// One row per progress interval, with wall-clock timestamps, for lining
    /// the run up against an external event such as a link-level flood.
    pub fn timeline_csv(&self) -> String {
        let mut o = String::from("second,unix_ms,elapsed_s,sent,replies,exceptions,timeouts,errors,mismatches,faults,churn_cycles,mean_latency_us\n");
        for t in &self.timeline {
            let _ = writeln!(
                o,
                "{},{},{:.3},{},{},{},{},{},{},{},{},{}",
                t.index,
                t.unix_ms,
                t.elapsed_s,
                t.sent,
                t.replies,
                t.exceptions,
                t.timeouts,
                t.errors,
                t.mismatches,
                t.faults,
                t.cycles,
                t.mean_latency_us.map_or_else(String::new, |u| u.to_string())
            );
        }
        o
    }

    /// Write the JSON, CSV and timeline files into `dir` (created if needed)
    /// under a timestamped name. Returns the paths written.
    pub fn write_to(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let base = self.file_stem();
        // Two runs in the same second get -2, -3, ... instead of overwriting.
        let stem = (1..100)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
            .find(|s| ["json", "csv", "timeline.csv"].iter().all(|ext| !dir.join(format!("{s}.{ext}")).exists()))
            .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "too many report files with this timestamp"))?;
        let files = [("json", self.json()), ("csv", self.csv()), ("timeline.csv", self.timeline_csv())];
        let mut written = Vec::new();
        for (ext, body) in files {
            let path = dir.join(format!("{stem}.{ext}"));
            OpenOptions::new().write(true).create_new(true).open(&path)?.write_all(body.as_bytes())?;
            written.push(path);
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        let mut config = Config::new("10.0.0.5", 502);
        config.seed = 99;
        let mut stats = Stats { sent: 10, ok: 8, timeouts: 1, ..Stats::default() };
        stats.exceptions.insert(2, 1);
        stats.latency.record(1500);
        stats.latency.record(2500);
        Report {
            config,
            version: "1.2.3",
            started_unix_ms: 1_700_000_000_000,
            elapsed: Duration::from_secs(2),
            stats,
            timeline: vec![Tick {
                index: 1,
                unix_ms: 1_700_000_001_000,
                elapsed_s: 1.0,
                sent: 5,
                replies: 4,
                exceptions: 0,
                timeouts: 1,
                errors: 0,
                mismatches: 0,
                faults: 0,
                cycles: 0,
                mean_latency_us: Some(2000),
            }],
        }
    }

    #[test]
    fn text_summary_has_the_key_numbers() {
        let t = report().text();
        for want in ["10 sent", "5.0/s", "ok replies    8", "code 2: 1", "timeouts      1", "seed 99", "p99", "ananasar stress 10.0.0.5:502"] {
            assert!(t.contains(want), "{t}\nlacks {want}");
        }
    }

    #[test]
    fn json_is_well_formed_enough_and_complete() {
        let j = report().json();
        for want in [
            "\"seed\": 99",
            "\"version\": \"1.2.3\"",
            "\"requests_sent\": 10",
            "\"mix_weight\": {",
            "\"code_2\": 1",
            "\"p50\":",
            "\"started_utc\": \"2023-11-14T22:13:20Z\"",
            "\"timeline\": [",
            "\"mean_latency_us\": 2000",
            "\"reproduce\": \"ananasar stress 10.0.0.5:502",
        ] {
            assert!(j.contains(want), "{j}\nlacks {want}");
        }
        assert_eq!(j.matches('{').count(), j.matches('}').count());
        assert_eq!(j.matches('[').count(), j.matches(']').count());
        assert!(!j.contains("NaN"));
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
    }

    #[test]
    fn csv_has_parameters_and_results() {
        let c = report().csv();
        assert!(c.starts_with("section,key,value\n"));
        for want in ["parameter,seed,99", "parameter,host,10.0.0.5", "result,requests_sent,10", "result,exceptions.code_2,1", "result,latency_us.max,2500"] {
            assert!(c.contains(want), "{c}\nlacks {want}");
        }
        // The reproduce line contains no comma, so it is not quoted; a text with one is.
        assert_eq!(Value::Text("a,b".into()).csv(), "\"a,b\"");
    }

    #[test]
    fn timeline_csv_has_a_row_per_tick() {
        let t = report().timeline_csv();
        assert_eq!(t.lines().count(), 2);
        assert!(t.contains("1,1700000001000,1.000,5,4,0,1,0,0,0,0,2000"));
    }

    #[test]
    fn writes_three_timestamped_files_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("ananasar-stress-test-{}", std::process::id()));
        let r = report();
        let first = r.write_to(&dir).unwrap();
        let second = r.write_to(&dir).unwrap();
        assert_eq!(first.len(), 3);
        assert!(first[0].to_string_lossy().contains("ananasar-stress-2023-11-14_22-13-20"));
        assert!(second[0].to_string_lossy().ends_with("-2.json"));
        assert!(first.iter().chain(&second).all(|p| p.exists()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
