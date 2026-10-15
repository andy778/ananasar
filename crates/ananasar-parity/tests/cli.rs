//! Drives the `ananasar-parity` binary: the cross-run must skip cleanly
//! without a target, `ours` + `diff` must agree with themselves, and `diff`
//! must print one verdict per scenario.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn tool(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ananasar-parity"))
        .args(args)
        .env_remove("ANANAS_ORIGINAL_ADDR")
        .output()
        .unwrap()
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ananasar-parity-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn cross_run_is_skipped_without_a_target() {
    let out = tool(&["cross"]);
    assert!(out.status.success());
    let stdout = text(&out.stdout);
    assert!(stdout.contains("SKIPPED"), "{stdout}");
    assert!(stdout.contains("ANANAS_ORIGINAL_ADDR"), "{stdout}");
}

#[test]
fn cross_run_reads_the_target_from_the_environment_variable() {
    // Nothing listens on port 1: the run must fail loudly, not skip.
    let out = Command::new(env!("CARGO_BIN_EXE_ananasar-parity"))
        .args(["cross", "--timeout-ms", "50"])
        .env("ANANAS_ORIGINAL_ADDR", "127.0.0.1:1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("127.0.0.1:1"));
}

#[test]
fn list_shows_profiles_and_scenarios() {
    let profiles = text(&tool(&["list", "--profiles"]).stdout);
    assert!(profiles.contains("default") && profiles.contains("exception-04") && profiles.contains("fault-ti"));
    let scenarios = text(&tool(&["list"]).stdout);
    assert!(scenarios.contains("mask_write") && scenarios.contains("split_one_byte_at_a_time"));
    assert!(!tool(&["list", "no-such-profile"]).status.success());
}

#[test]
fn diff_prints_one_verdict_per_scenario_and_sets_the_exit_status() {
    let a = scratch("a.rec");
    let b = scratch("b.rec");
    let c = scratch("c.rec");
    std::fs::write(&a, "## one\n> 0001 0000 0002 01 07\n< 0001 0000 0003 01 07 00\n\n## two\n> 0001 0000 0002 01 07\n< none\n").unwrap();
    // Same frames with other transaction ids: still equal.
    std::fs::write(&b, "## one\n> 0009 0000 0002 01 07\n< 0009 0000 0003 01 07 00\n\n## two\n> 0009 0000 0002 01 07\n< none\n").unwrap();
    // Second scenario answered differently.
    std::fs::write(&c, "## one\n> 0001 0000 0002 01 07\n< 0001 0000 0003 01 07 00\n\n## two\n> 0001 0000 0002 01 07\n< 0001 0000 0003 01 87 01\n").unwrap();
    let (a, b, c) = (a.to_str().unwrap(), b.to_str().unwrap(), c.to_str().unwrap());

    let same = tool(&["diff", a, b]);
    assert!(same.status.success(), "{}", text(&same.stdout));
    let stdout = text(&same.stdout);
    assert!(stdout.contains("same  one") && stdout.contains("same  two"), "{stdout}");

    let strict = tool(&["diff", a, b, "--strict-txn"]);
    assert_eq!(strict.status.code(), Some(1), "transaction ids matter with --strict-txn");

    let differs = tool(&["diff", a, c]);
    assert_eq!(differs.status.code(), Some(1));
    let stdout = text(&differs.stdout);
    assert!(stdout.contains("same  one") && stdout.contains("DIFF  two"), "{stdout}");
    assert!(stdout.contains("1 scenario(s) differ") || stdout.contains("2 scenario(s), 1 differ"), "{stdout}");
}

#[test]
fn ours_recording_diffs_clean_against_itself() {
    let path = scratch("ours.rec");
    let p = path.to_str().unwrap();
    let out = tool(&["ours", "--profile", "disable-fc3", "--timeout-ms", "100", "--out", p]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let again = tool(&["diff", p, p, "--strict-txn"]);
    assert!(again.status.success());
    assert!(text(&again.stdout).contains("0 differ"));
}
