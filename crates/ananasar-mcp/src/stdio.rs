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
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_io(cfg, &mut stdin.lock(), &mut stdout.lock())
}

/// The loop behind [`serve`], over any reader and writer so tests can drive it.
fn serve_io(cfg: Config, input: &mut impl BufRead, stdout: &mut impl Write) -> i32 {
    let mut mcp = Mcp::new(cfg);
    let mut buf = Vec::new();
    loop {
        let reply = match read_line(input, &mut buf) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::Cursor;

    /// A reader whose every read fails.
    struct Broken;
    impl io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("boom"))
        }
    }

    /// A writer whose flush fails, like a closed pipe.
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    fn run(input: &[u8]) -> (i32, Vec<Value>) {
        let mut out = Vec::new();
        let code = serve_io(Config::default(), &mut Cursor::new(input.to_vec()), &mut out);
        let replies =
            String::from_utf8(out).expect("utf8").lines().map(|l| serde_json::from_str(l).expect("json")).collect();
        (code, replies)
    }

    const PING: &str = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;

    #[test]
    fn empty_input_exits_zero_without_output() {
        assert_eq!(run(b""), (0, vec![]));
    }

    #[test]
    fn over_long_line_gets_an_error_and_the_next_line_is_served() {
        let mut input = vec![b'x'; MAX_LINE + 10];
        input.push(b'\n');
        input.extend_from_slice(PING.as_bytes());
        input.push(b'\n');
        let (code, replies) = run(&input);
        assert_eq!(code, 0);
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["error"]["code"], INVALID_REQUEST);
        assert_eq!(replies[0]["error"]["message"], "message too long");
        assert!(replies[0]["id"].is_null());
        assert_eq!(replies[1]["id"], 7);
        assert!(replies[1]["result"].is_object());
    }

    #[test]
    fn over_long_line_without_newline_at_eof_is_reported() {
        let (code, replies) = run(&vec![b'x'; MAX_LINE + 10]);
        assert_eq!(code, 0);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["error"]["message"], "message too long");
    }

    #[test]
    fn blank_lines_and_a_final_line_without_newline_are_handled() {
        let input = format!("\n  \n{PING}");
        let (code, replies) = run(input.as_bytes());
        assert_eq!(code, 0);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["id"], 7);
    }

    #[test]
    fn over_long_line_spanning_many_reads_is_discarded_whole() {
        let mut input = vec![b'x'; MAX_LINE * 2];
        input.push(b'\n');
        input.extend_from_slice(PING.as_bytes());
        input.push(b'\n');
        let mut reader = io::BufReader::with_capacity(4096, Cursor::new(input));
        let mut out = Vec::new();
        assert_eq!(serve_io(Config::default(), &mut reader, &mut out), 0);
        let text = String::from_utf8(out).expect("utf8");
        let replies: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).expect("json")).collect();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["error"]["message"], "message too long");
        assert_eq!(replies[1]["id"], 7);
    }

    #[test]
    fn read_error_exits_one() {
        assert_eq!(serve_io(Config::default(), &mut io::BufReader::new(Broken), &mut Vec::new()), 1);
    }

    #[test]
    fn closed_pipe_exits_zero() {
        let input = format!("{PING}\n{PING}\n");
        assert_eq!(serve_io(Config::default(), &mut Cursor::new(input.into_bytes()), &mut Closed), 0);
    }
}
