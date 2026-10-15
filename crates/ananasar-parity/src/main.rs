//! `ananasar-parity`: run, record and diff Modbus/TCP exchanges.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "developer tool, not yet audited for panics (see #59)"
)]

use ananasar_parity::diff::Options;
use ananasar_parity::recording::Recording;
use ananasar_parity::scenarios::{self, DEFAULT_PROFILE, PROFILES};
use ananasar_parity::{diff, ours, proxy, runner};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// Environment variable naming the original server for the cross-run.
const ORIGINAL_ENV: &str = "ANANAS_ORIGINAL_ADDR";

const USAGE: &str = "\
ananasar-parity: compare Modbus/TCP servers at the byte level

USAGE:
  ananasar-parity run <host:port> [--profile P] [--only NAME,NAME] [--unit N] [--timeout-ms N] [--out FILE]
      Run a profile's scenarios (default: default) against a server and write a
      recording (stdout without --out). Start the server with all registers
      zero and the options the profile lists (see `list --profiles`).
  ananasar-parity cross [--addr host:port] [--profile P] [--only NAME,NAME] [--unit N] [--timeout-ms N]
                        [--strict-txn] [--record DIR]
      Run a profile against the ORIGINAL server (--addr, or the environment
      variable ANANAS_ORIGINAL_ADDR), run the same profile against Ananasar
      in-process and print one verdict per scenario. --record DIR also writes
      the original's frames to DIR/<profile>.rec. Without an address the
      command reports that it is skipped and exits with status 0.
  ananasar-parity ours [--profile P] [--only NAME,NAME] [--timeout-ms N] [--out FILE]
      Start Ananasar's server in-process, set up like the profile, and record
      the profile's scenarios against it (to diff with a golden or a run).
  ananasar-parity proxy <listen host:port> <target host:port> --out FILE
      Forward connections to the target and record the traffic. A connection
      appears in the file when it closes.
  ananasar-parity diff <a.rec> <b.rec> [--strict-txn]
      Compare two recordings scenario by scenario and print a verdict for
      each. Transaction ids and timing are ignored unless --strict-txn is
      given. Exit status 1 if they differ.
  ananasar-parity list [P | --profiles]
      Print the scenarios of a profile (default: default) or the profiles.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("cross") => cmd_cross(&args[1..]),
        Some("ours") => cmd_ours(&args[1..]),
        Some("proxy") => cmd_proxy(&args[1..]),
        Some("diff") => cmd_diff(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    result.unwrap_or_else(|msg| {
        eprintln!("{msg}");
        ExitCode::from(2)
    })
}

/// `--flag value` pairs in command-line order.
type Given = Vec<(String, String)>;

/// Split `args` into positionals and flags. `valued` flags take a value,
/// `switches` do not (they are stored with the value `"1"`).
fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Result<(Vec<String>, Given), String> {
    let mut positional = Vec::new();
    let mut given = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            if switches.contains(&name) {
                given.push((name.to_owned(), "1".to_owned()));
            } else if valued.contains(&name) {
                let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
                given.push((name.to_owned(), value.clone()));
            } else {
                return Err(format!("unknown option --{name}\n\n{USAGE}"));
            }
        } else {
            positional.push(a.clone());
        }
    }
    Ok((positional, given))
}

fn flag<'a>(given: &'a [(String, String)], name: &str) -> Option<&'a str> {
    given.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn number<T: std::str::FromStr>(given: &[(String, String)], name: &str, default: T) -> Result<T, String> {
    flag(given, name).map_or(Ok(default), |v| v.parse().map_err(|_| format!("--{name}: not a number: {v}")))
}

fn profile_name(given: &[(String, String)]) -> Result<&str, String> {
    let name = flag(given, "profile").unwrap_or(DEFAULT_PROFILE);
    if PROFILES.iter().any(|p| p.name == name) {
        Ok(name)
    } else {
        Err(format!("unknown profile {name}; `ananasar-parity list --profiles` shows them"))
    }
}

fn header(addr: &str, profile: &str, unit: u8) -> String {
    format!("# ananasar-parity run against {addr}, profile {profile}, unit {unit}\n\n")
}

fn cmd_run(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["profile", "only", "unit", "timeout-ms", "out"], &[])?;
    let [addr] = pos.as_slice() else { return Err(USAGE.to_owned()) };
    let profile = profile_name(&given)?;
    let unit = number(&given, "unit", 1u8)?;
    let timeout = Duration::from_millis(number(&given, "timeout-ms", 500u64)?);
    let rec = runner::run_profile(addr.as_str(), profile, &only(&given), unit, timeout)
        .map_err(|e| format!("cannot run against {addr}: {e}"))?;
    let mut text = header(addr, profile, unit);
    text.push_str(&rec.to_text());
    match flag(&given, "out") {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_cross(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["addr", "profile", "only", "unit", "timeout-ms", "record"], &["strict-txn"])?;
    if !pos.is_empty() {
        return Err(USAGE.to_owned());
    }
    let from_env = std::env::var(ORIGINAL_ENV).ok().filter(|v| !v.trim().is_empty());
    let Some(addr) = flag(&given, "addr").map(str::to_owned).or(from_env) else {
        println!(
            "SKIPPED: no original server given. Pass --addr host:port or set {ORIGINAL_ENV} \
             (see docs/parity-testing.md). Nothing was run."
        );
        return Ok(ExitCode::SUCCESS);
    };
    let profile = profile_name(&given)?;
    let unit = number(&given, "unit", 1u8)?;
    let timeout = Duration::from_millis(number(&given, "timeout-ms", 500u64)?);
    let opts = Options { strict_txn: flag(&given, "strict-txn").is_some() || profile == "fault-ti" };
    let setup = PROFILES.iter().find(|p| p.name == profile).map_or("", |p| p.setup);
    println!("profile {profile}: the original must have all registers zero and this setup: {setup}");
    println!("running {} scenarios against the original at {addr} ...", scenarios::for_profile(profile, unit).len());

    let original = runner::run_profile(addr.as_str(), profile, &only(&given), unit, timeout)
        .map_err(|e| format!("cannot run against {addr}: {e}"))?;
    if let Some(dir) = flag(&given, "record") {
        let path = Path::new(dir).join(format!("{profile}.rec"));
        let text = format!("{}{}", header("the original", profile, unit), original.to_text());
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("recorded the original's frames to {}", path.display());
    }
    let Some(cfg) = ours::server_config(profile) else {
        println!("Ananasar has no counterpart for this option yet; nothing to compare against.");
        return Ok(ExitCode::SUCCESS);
    };
    let server = ours::start(cfg).map_err(|e| format!("cannot start Ananasar's server: {e}"))?;
    let mine = runner::run_profile(server.addr, profile, &only(&given), unit, timeout).map_err(|e| e.to_string())?;
    Ok(print_verdicts(&original, &mine, opts, "original", "ananasar"))
}

fn print_verdicts(a: &Recording, b: &Recording, opts: Options, a_name: &str, b_name: &str) -> ExitCode {
    let verdicts = diff::verdicts(a, b, opts);
    for v in &verdicts {
        if v.matches() {
            println!("same  {}", v.name);
        } else {
            println!("DIFF  {}", v.name);
            for d in &v.differences {
                let d = d.replace("\n  a:", &format!("\n    {a_name}:")).replace("\n  b:", &format!("\n    {b_name}:"));
                println!("      {d}");
            }
        }
    }
    let different = verdicts.iter().filter(|v| !v.matches()).count();
    println!("\n{} scenario(s), {different} differ", verdicts.len());
    if different == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

fn cmd_proxy(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["out"], &[])?;
    let [listen, target] = pos.as_slice() else { return Err(USAGE.to_owned()) };
    let out = flag(&given, "out").ok_or("--out FILE is required")?;
    let listener = TcpListener::bind(listen.as_str()).map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    eprintln!("proxying {listen} -> {target}, recording to {out} (Ctrl+C to stop)");
    proxy::serve(&listener, target.clone(), &PathBuf::from(out)).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

fn load(path: &str) -> Result<Recording, String> {
    let text = std::fs::read_to_string(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    Recording::parse(&text).map_err(|e| format!("{path}: {e}"))
}

fn cmd_diff(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &[], &["strict-txn"])?;
    let [a, b] = pos.as_slice() else { return Err(USAGE.to_owned()) };
    let opts = Options { strict_txn: flag(&given, "strict-txn").is_some() };
    let (ra, rb) = (load(a)?, load(b)?);
    // Recordings with the same section names (scenario runs) are compared per
    // scenario; anything else (proxied sessions) is paired by position.
    let names = |r: &Recording| r.sections.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    if names(&ra) == names(&rb) {
        return Ok(print_verdicts(&ra, &rb, opts, "a", "b"));
    }
    let differences = diff::diff_with(&ra, &rb, opts);
    if differences.is_empty() {
        println!("recordings match");
        return Ok(ExitCode::SUCCESS);
    }
    for d in &differences {
        println!("{d}");
    }
    println!("\n{} difference(s)", differences.len());
    Ok(ExitCode::from(1))
}

fn cmd_list(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &[], &["profiles"])?;
    if flag(&given, "profiles").is_some() {
        for p in PROFILES {
            println!("{:<20} {}", p.name, p.setup);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let profile = pos.first().map_or(DEFAULT_PROFILE, String::as_str);
    let list = scenarios::for_profile(profile, 1);
    if list.is_empty() {
        return Err(format!("unknown profile {profile}; use `list --profiles`"));
    }
    for s in list {
        println!("{:<40} {}", s.name, s.about);
    }
    Ok(ExitCode::SUCCESS)
}

/// The `--only a,b` scenario names, empty for all.
fn only(given: &[(String, String)]) -> Vec<String> {
    flag(given, "only").map_or_else(Vec::new, |v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
}

fn cmd_ours(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["profile", "only", "timeout-ms", "out"], &[])?;
    if !pos.is_empty() {
        return Err(USAGE.to_owned());
    }
    let profile = profile_name(&given)?;
    let timeout = Duration::from_millis(number(&given, "timeout-ms", 300u64)?);
    let cfg = ours::server_config(profile).unwrap_or_else(|| {
        eprintln!("note: Ananasar has no counterpart for profile {profile}; recording its default behaviour");
        ananasar_core::server::ServerConfig::default()
    });
    let server = ours::start(cfg).map_err(|e| e.to_string())?;
    let rec = runner::run_profile(server.addr, profile, &only(&given), 1, timeout).map_err(|e| e.to_string())?;
    let text = format!("{}{}", header("ananasar (in-process)", profile, 1), rec.to_text());
    match flag(&given, "out") {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}
