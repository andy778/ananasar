//! `ananasar-parity`: run, record and diff Modbus/TCP exchanges.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "developer tool, not yet audited for panics (see #59)"
)]

use ananasar_parity::recording::Recording;
use ananasar_parity::{diff, proxy, runner, scenarios};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
ananasar-parity: compare Modbus/TCP servers at the byte level

USAGE:
  ananasar-parity run <host:port> [--unit N] [--timeout-ms N] [--out FILE]
      Run the fixed scenarios against a server and write a recording
      (stdout without --out). Start the server with all registers zero.
  ananasar-parity proxy <listen host:port> <target host:port> --out FILE
      Forward connections to the target and record the traffic. A connection
      appears in the file when it closes.
  ananasar-parity diff <a.rec> <b.rec>
      Compare two recordings, ignoring transaction ids and timing.
      Exit status 1 if they differ.
  ananasar-parity list
      Print the scenario names.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("proxy") => cmd_proxy(&args[1..]),
        Some("diff") => cmd_diff(&args[1..]),
        Some("list") => {
            for s in scenarios::all(1) {
                println!("{}", s.name);
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(USAGE.to_owned()),
    };
    result.unwrap_or_else(|msg| {
        eprintln!("{msg}");
        ExitCode::from(2)
    })
}

/// `--flag value` pairs in command-line order.
type Given = Vec<(String, String)>;

/// Split `args` into positionals and `--flag value` pairs.
fn parse(args: &[String], flags: &[&str]) -> Result<(Vec<String>, Given), String> {
    let mut positional = Vec::new();
    let mut given = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            if !flags.contains(&name) {
                return Err(format!("unknown option --{name}\n\n{USAGE}"));
            }
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            given.push((name.to_owned(), value.clone()));
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

fn cmd_run(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["unit", "timeout-ms", "out"])?;
    let [addr] = pos.as_slice() else { return Err(USAGE.to_owned()) };
    let unit = number(&given, "unit", 1u8)?;
    let timeout = Duration::from_millis(number(&given, "timeout-ms", 500u64)?);
    let rec = runner::run(addr.as_str(), unit, timeout).map_err(|e| format!("cannot run against {addr}: {e}"))?;
    let mut text = format!("# ananasar-parity run against {addr}, unit {unit}\n\n");
    text.push_str(&rec.to_text());
    match flag(&given, "out") {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_proxy(args: &[String]) -> Result<ExitCode, String> {
    let (pos, given) = parse(args, &["out"])?;
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
    let [a, b] = args else { return Err(USAGE.to_owned()) };
    let differences = diff::diff(&load(a)?, &load(b)?);
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
