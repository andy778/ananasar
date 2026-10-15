//! `ananasar-mcp`: MCP server on stdin/stdout. Protocol messages go to stdout
//! only; everything else (diagnostics) goes to stderr.

use ananasar_mcp::rpc::{INVALID_REQUEST, handle_line};
use ananasar_mcp::{Config, Mcp};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::net::IpAddr;
use std::process::ExitCode;

/// Longest accepted message line. Longer lines are skipped with an error.
const MAX_LINE: usize = 1024 * 1024;

const HELP: &str = "\
ananasar-mcp: Model Context Protocol server for Modbus/TCP (stdio transport)

USAGE:
    ananasar-mcp [--allow-write | --read-only] [--bind <IP>]

OPTIONS:
        --allow-write   Let the assistant write to remote Modbus devices (FC6, FC16, FC22).
                        Without it the server is read-only.
        --read-only     Refuse remote writes (the default; accepted for clarity).
        --bind <IP>     Address the simulated server listens on [default: 127.0.0.1]
    -h, --help          Print this help
    -V, --version       Print the version
";

enum Parsed {
    Run(Config),
    Exit(ExitCode),
}

fn parse_args(args: impl Iterator<Item = String>) -> Parsed {
    let mut cfg = Config::default();
    let (mut allow, mut read_only) = (false, false);
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--allow-write" => allow = true,
            "--read-only" => read_only = true,
            "--bind" => match args.next().map(|v| v.parse::<IpAddr>()) {
                Some(Ok(ip)) => cfg.bind = ip,
                _ => {
                    eprintln!("ananasar-mcp: --bind needs an IP address");
                    return Parsed::Exit(ExitCode::from(2));
                }
            },
            "-h" | "--help" => {
                print!("{HELP}");
                return Parsed::Exit(ExitCode::SUCCESS);
            }
            "-V" | "--version" => {
                println!("ananasar-mcp {}", env!("CARGO_PKG_VERSION"));
                return Parsed::Exit(ExitCode::SUCCESS);
            }
            other => {
                eprintln!("ananasar-mcp: unknown argument '{other}' (see --help)");
                return Parsed::Exit(ExitCode::from(2));
            }
        }
    }
    if allow && read_only {
        eprintln!("ananasar-mcp: --allow-write and --read-only contradict each other");
        return Parsed::Exit(ExitCode::from(2));
    }
    cfg.allow_write = allow;
    Parsed::Run(cfg)
}

/// Read one line of at most [`MAX_LINE`] bytes. `Ok(None)` is end of input;
/// `Ok(Some(Err(())))` is an over-long line that has been discarded.
fn read_line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> io::Result<Option<Result<(), ()>>> {
    buf.clear();
    let mut too_long = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if buf.is_empty() && !too_long { None } else { Some(finish(too_long)) });
        }
        let (take, found) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i.saturating_add(1), true),
            None => (chunk.len(), false),
        };
        if !too_long {
            buf.extend_from_slice(chunk.get(..take).unwrap_or_default());
            too_long = buf.len() > MAX_LINE;
            if too_long {
                buf.clear();
            }
        }
        input.consume(take);
        if found {
            return Ok(Some(finish(too_long)));
        }
    }
}

fn finish(too_long: bool) -> Result<(), ()> {
    if too_long { Err(()) } else { Ok(()) }
}

fn main() -> ExitCode {
    let cfg = match parse_args(std::env::args().skip(1)) {
        Parsed::Run(cfg) => cfg,
        Parsed::Exit(code) => return code,
    };
    eprintln!(
        "ananasar-mcp {} ready on stdio ({}; simulated server binds {})",
        env!("CARGO_PKG_VERSION"),
        if cfg.allow_write { "writes allowed" } else { "read-only" },
        cfg.bind
    );
    let mut mcp = Mcp::new(cfg);
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout().lock();
    let mut buf = Vec::new();
    loop {
        let reply = match read_line(&mut input, &mut buf) {
            Ok(None) => return ExitCode::SUCCESS,
            Ok(Some(Err(()))) => Some(
                json!({ "jsonrpc": "2.0", "id": null,
                    "error": { "code": INVALID_REQUEST, "message": "message too long" } })
                .to_string(),
            ),
            Ok(Some(Ok(()))) => {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim();
                if line.is_empty() { None } else { handle_line(&mut mcp, line) }
            }
            Err(e) => {
                eprintln!("ananasar-mcp: read error: {e}");
                return ExitCode::FAILURE;
            }
        };
        if let Some(reply) = reply
            && (writeln!(stdout, "{reply}").and_then(|()| stdout.flush())).is_err()
        {
            return ExitCode::SUCCESS; // the client closed the pipe
        }
    }
}
