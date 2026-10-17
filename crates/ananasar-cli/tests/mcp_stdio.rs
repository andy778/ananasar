//! Run the real binary (`ananasar mcp`) and speak newline-delimited JSON-RPC over its pipes.
#![cfg(feature = "mcp")]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a panic is a failed test"
)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_session_answers_one_line_per_request() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ananasar"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());

    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}).to_string(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        String::new(), // blank lines are ignored
        "not json".to_owned(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}).to_string(),
    ];
    for line in &input {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin); // end of input ends the server

    let mut replies = Vec::new();
    let mut line = String::new();
    while out.read_line(&mut line).unwrap() > 0 {
        replies.push(serde_json::from_str::<Value>(line.trim()).unwrap());
        line.clear();
    }
    assert_eq!(replies.len(), 3, "initialize, parse error, tools/list: {replies:?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert!(replies[2]["result"]["tools"].as_array().unwrap().len() >= 16);
    assert!(child.wait().unwrap().success());
}
