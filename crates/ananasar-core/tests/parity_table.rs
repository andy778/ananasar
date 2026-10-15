//! Checks `docs/parity.toml` against the source tree: shipped entries must
//! name tests, and every named test must exist as a `#[test]` fn.
//!
//! The table uses a tiny TOML subset (`[[feature]]` tables with string,
//! integer and one-line string-array values) so the core crate stays
//! dependency-free.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Default)]
struct Feature {
    id: String,
    status: String,
    tests: Vec<String>,
    issue: Option<u32>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_strings(value: &str) -> Vec<String> {
    let inner = value.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse(text: &str) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[feature]]" {
            out.push(Feature::default());
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("parity.toml line {}: expected key = value", n + 1));
        let f = out
            .last_mut()
            .unwrap_or_else(|| panic!("parity.toml line {}: key before [[feature]]", n + 1));
        let (key, value) = (key.trim(), value.trim());
        match key {
            "id" => f.id = value.trim_matches('"').to_string(),
            "status" => f.status = value.trim_matches('"').to_string(),
            "tests" => f.tests = parse_strings(value),
            "issue" => f.issue = Some(value.parse().expect("issue must be a number")),
            "area" | "feature" | "note" => {}
            other => panic!("parity.toml line {}: unknown key {other}", n + 1),
        }
    }
    out
}

/// True when `path` has `#[test]` directly above `fn name(`.
fn has_test(path: &str, name: &str) -> bool {
    let Ok(src) = fs::read_to_string(root().join(path)) else {
        return false;
    };
    let needle = format!("fn {name}(");
    let lines: Vec<&str> = src.lines().map(str::trim).collect();
    lines.iter().enumerate().any(|(i, l)| {
        l.contains(&needle)
            && lines[..i]
                .iter()
                .rev()
                .take_while(|p| p.starts_with("#[") || p.starts_with("///"))
                .any(|p| *p == "#[test]")
    })
}

#[test]
fn parity_table_matches_the_tests() {
    let text = fs::read_to_string(root().join("docs/parity.toml")).expect("docs/parity.toml");
    let features = parse(&text);
    assert!(!features.is_empty(), "no features in the table");

    let mut ids = HashSet::new();
    let mut problems = Vec::new();
    for f in &features {
        if f.id.is_empty() || !ids.insert(f.id.clone()) {
            problems.push(format!("missing or duplicate id {:?}", f.id));
        }
        match f.status.as_str() {
            "shipped" => {
                if f.tests.is_empty() {
                    problems.push(format!("{}: shipped but names no test", f.id));
                }
            }
            "partial" | "missing" => {
                if f.issue.is_none() {
                    problems.push(format!("{}: {} needs an issue", f.id, f.status));
                }
            }
            other => problems.push(format!("{}: unknown status {other:?}", f.id)),
        }
        for t in &f.tests {
            match t.split_once("::") {
                Some((path, name)) if has_test(path, name) => {}
                _ => problems.push(format!("{}: test {t} not found", f.id)),
            }
        }
    }
    assert!(
        problems.is_empty(),
        "parity table problems:\n{}",
        problems.join("\n")
    );
}
