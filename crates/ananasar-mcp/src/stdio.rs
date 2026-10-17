//! The stdio transport: newline-delimited JSON-RPC on stdin/stdout.

use crate::rpc::{INVALID_REQUEST, handle_line};
use crate::{Config, Mcp};
use serde_json::json;
use std::io::{self, BufRead, Write};

/// Longest accepted message line. Longer lines are skipped with an error.
const MAX_LINE: usize = 1024 * 1024;

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

/// Serve MCP on stdin/stdout until the input ends or the client closes the
/// pipe. Diagnostics go to stderr; stdout carries protocol messages only.
/// Returns the process exit status.
pub fn serve(cfg: Config) -> i32 {
    eprintln!(
        "ananasar mcp {} ready on stdio ({}; simulated server binds {})",
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
            Ok(None) => return 0,
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
                eprintln!("ananasar mcp: read error: {e}");
                return 1;
            }
        };
        if let Some(reply) = reply
            && (writeln!(stdout, "{reply}").and_then(|()| stdout.flush())).is_err()
        {
            return 0; // the client closed the pipe
        }
    }
}
