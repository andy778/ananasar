//! Runs Ananasar's real server (over loopback) through every scenario of
//! every profile and compares the frames with the committed recordings of the
//! original Ananas in `golden/`. No original is needed: the goldens are plain
//! frames. Any difference fails the test unless the golden file lists it as an
//! intentional deviation with a reason (see `ananasar_parity::golden`).
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_parity::diff::Options;
use ananasar_parity::golden::{self, Golden};
use ananasar_parity::scenarios::{PROFILES, for_profile};
use ananasar_parity::{ours, runner};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn load(profile: &str) -> Golden {
    let path = golden_dir().join(format!("{profile}.rec"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    golden::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn every_profile_has_a_golden_file_and_nothing_else_is_there() {
    let mut on_disk: Vec<String> = std::fs::read_dir(golden_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rec"))
        .map(|n| n.trim_end_matches(".rec").to_owned())
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = PROFILES.iter().map(|p| p.name.to_owned()).collect();
    expected.sort();
    assert_eq!(on_disk, expected, "golden files and PROFILES must match one to one");
}

#[test]
fn goldens_cover_exactly_the_catalog() {
    for p in PROFILES {
        let g = load(p.name);
        let recorded: Vec<&str> = g.recording.sections.iter().map(|s| s.name.as_str()).collect();
        let catalog: Vec<&str> = for_profile(p.name, 1).iter().map(|s| s.name).collect();
        assert_eq!(recorded, catalog, "{}: re-record the golden after changing the catalog", p.name);
        for note in &g.notes {
            assert!(recorded.contains(&note.section.as_str()), "{}: note for unknown section {}", p.name, note.section);
        }
    }
}

#[test]
fn deviations_and_quirks_carry_a_reason() {
    for p in PROFILES {
        let g = load(p.name);
        for n in &g.notes {
            assert!(n.text.split_whitespace().count() >= 3, "{}: [{}] note needs a real reason", p.name, n.section);
        }
        if let Some(d) = &g.default_deviation {
            assert!(d.split_whitespace().count() >= 3, "{}: default-deviation needs a real reason", p.name);
        }
    }
}

#[test]
fn ananasar_matches_the_original_except_listed_deviations() {
    let handles: Vec<_> = PROFILES
        .iter()
        .map(|p| {
            let name = p.name;
            std::thread::spawn(move || {
                let golden = load(name);
                // Profiles for options Ananasar lacks run against the default setup;
                // their golden files say so with a default-deviation.
                let cfg = ours::server_config(name).unwrap_or_default();
                let server = ours::start(cfg).unwrap();
                let recorded = runner::run_profile(server.addr, name, &[], 1, Duration::from_millis(250)).unwrap();
                let report = golden::compare(&golden, &recorded, Options { strict_txn: true });
                (name, report)
            })
        })
        .collect();
    let mut failures = String::new();
    for h in handles {
        let (name, report) = h.join().unwrap();
        if !report.is_clean() {
            failures.push_str(&format!("== profile {name}\n{}\n", report.failures()));
        }
    }
    assert!(failures.is_empty(), "Ananasar differs from the original:\n{failures}");
}

#[test]
fn unimplemented_options_are_declared_in_their_golden() {
    for p in PROFILES {
        if ours::server_config(p.name).is_none() {
            let g = load(p.name);
            assert!(g.default_deviation.is_some(), "{}: Ananasar lacks this option, so the golden must say why it differs", p.name);
        }
    }
}

