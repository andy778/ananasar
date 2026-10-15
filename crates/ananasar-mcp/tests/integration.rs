//! End to end: drive the MCP layer with JSON-RPC lines, starting the simulated
//! server on an ephemeral port and talking to it with the in-process client.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_mcp::rpc::handle_line;
use ananasar_mcp::{Config, Mcp};
use serde_json::{Value, json};

struct Harness {
    mcp: Mcp,
    next_id: u64,
}

impl Harness {
    fn new(allow_write: bool) -> Self {
        Self { mcp: Mcp::new(Config { allow_write, ..Config::default() }), next_id: 1 }
    }

    /// Call a tool; returns `(is_error, structuredContent or error text)`.
    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        self.next_id += 1;
        let line = json!({
            "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        })
        .to_string();
        let out = handle_line(&mut self.mcp, &line).expect("response");
        let v: Value = serde_json::from_str(&out).expect("json");
        assert_eq!(v["id"], self.next_id);
        let result = &v["result"];
        if result["isError"] == true {
            (true, result["content"][0]["text"].clone())
        } else {
            (false, result["structuredContent"].clone())
        }
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (err, v) = self.call(name, args);
        assert!(!err, "{name} failed: {v}");
        v
    }

    fn err(&mut self, name: &str, args: Value) -> String {
        let (err, v) = self.call(name, args);
        assert!(err, "{name} unexpectedly succeeded: {v}");
        v.as_str().unwrap_or_default().to_owned()
    }
}

#[test]
fn client_and_simulated_server_round_trip() {
    let mut h = Harness::new(true);

    let started = h.ok("server_start", json!({ "port": 0, "init_input": "ascending", "log_mode": "interpretive" }));
    let port = started["port"].as_u64().expect("port");
    assert_ne!(port, 0);
    assert!(started["listening"].as_str().unwrap().starts_with("127.0.0.1:"), "binds loopback by default");
    assert!(h.err("server_start", json!({ "port": 0 })).contains("already running"));

    // Seed the simulation, then read it back over Modbus/TCP.
    h.ok("server_set_registers", json!({ "bank": "holding", "address": 10, "values": [7, 8, 9] }));
    let info = h.ok("ananasar_info", json!({}));
    assert_eq!(info["remote_writes_allowed"], true);
    assert!(info["client"].is_null());

    assert!(h.err("modbus_read_holding", json!({ "address": 0, "count": 1 })).contains("modbus_connect"));
    h.ok("modbus_connect", json!({ "host": "127.0.0.1", "port": port, "unit_id": 1 }));
    let read = h.ok("modbus_read_holding", json!({ "address": 10, "count": 3 }));
    assert_eq!(read["values"], json!([7, 8, 9]));
    let input = h.ok("modbus_read_input", json!({ "address": 5, "count": 2 }));
    assert_eq!(input["values"], json!([5, 6]));

    // Writes land in the server's register file.
    h.ok("modbus_write_single", json!({ "address": 20, "value": 1234 }));
    h.ok("modbus_write_multiple", json!({ "address": 21, "values": [1, 2, 3] }));
    h.ok("modbus_mask_write", json!({ "address": 20, "and_mask": 0x00FF, "or_mask": 0x1200 }));
    let regs = h.ok("server_get_registers", json!({ "bank": "holding", "address": 20, "count": 4 }));
    assert_eq!(regs["values"], json!([(1234 & 0xFF) | 0x1200, 1, 2, 3]));
    assert_eq!(h.ok("modbus_read_exception_status", json!({}))["status"], 0);

    // An address past the end becomes a Modbus exception, not a crash.
    let msg = h.err("modbus_read_holding", json!({ "address": 65535, "count": 2 }));
    assert!(msg.contains("exception 2") && msg.contains("Illegal Data Address"), "{msg}");

    // Statistics and log reflect the traffic.
    let stats = h.ok("server_stats", json!({}));
    assert_eq!(stats["clients"], 1);
    let slot = &stats["slots"][0];
    assert_eq!(slot["connected"], true);
    assert_eq!(slot["ip"], "127.0.0.1");
    assert_eq!(slot["function"], 3);
    assert_eq!(slot["start_address"], 65535);
    assert_eq!(slot["header_check"], "ok");
    assert!(slot["bytes_received"].as_u64().unwrap() > 0);
    assert_eq!(stats["return_exception"], 2);
    assert!(stats["log_entries"].as_u64().unwrap() >= 7);
    let log = h.ok("server_get_log", json!({ "max_chars": 100_000 }));
    assert!(log["log"].as_str().unwrap().contains("Connection: 1"));
    let tail = h.ok("server_get_log", json!({ "max_chars": 10, "clear": true }));
    assert!(tail["log"].as_str().unwrap().chars().count() <= 10);
    assert_eq!(h.ok("server_get_log", json!({}))["log"], "");

    // Options: disabling a function code and forcing an exception.
    let opts = h.ok("server_options", json!({ "active_function_codes": [4, 6, 16, 22] }));
    assert_eq!(opts["active_function_codes"], json!([4, 6, 16, 22]));
    let msg = h.err("modbus_read_holding", json!({ "address": 0, "count": 1 }));
    assert!(msg.contains("exception 1"), "{msg}");
    let opts = h.ok("server_options", json!({ "forced_exception": 6 }));
    assert_eq!(opts["forced_exception_name"], "Slave Device Busy");
    let msg = h.err("modbus_read_input", json!({ "address": 0, "count": 1 }));
    assert!(msg.contains("exception 6"), "{msg}");
    h.ok("server_options", json!({ "forced_exception": 0, "active_function_codes": [3, 4] }));
    assert_eq!(h.ok("modbus_read_holding", json!({ "address": 0, "count": 1 }))["count"], 1);
    assert!(h.err("server_options", json!({ "active_function_codes": [5] })).contains("not supported"));

    // A unit id the server does not answer to times out cleanly.
    h.ok("server_options", json!({ "unit_id": 9 }));
    let msg = h.err("modbus_read_holding", json!({ "address": 0, "count": 1, "timeout_ms": 200 }));
    assert!(msg.contains("timeout"), "{msg}");

    h.ok("modbus_disconnect", json!({}));
    h.ok("server_stop", json!({}));
    assert!(h.err("server_stats", json!({})).contains("not running"));
}

#[test]
fn read_only_mode_never_writes() {
    let mut writer = Harness::new(true);
    let port = writer.ok("server_start", json!({ "port": 0 }))["port"].as_u64().unwrap();

    // Second MCP instance, read-only, as a client of the first one's server.
    let mut h = Harness::new(false);
    h.ok("modbus_connect", json!({ "host": "127.0.0.1", "port": port }));
    for (tool, args) in [
        ("modbus_write_single", json!({ "address": 0, "value": 1 })),
        ("modbus_write_multiple", json!({ "address": 0, "values": [1] })),
        ("modbus_mask_write", json!({ "address": 0, "and_mask": 0, "or_mask": 1 })),
    ] {
        assert!(h.err(tool, args).contains("read-only"), "{tool}");
    }
    let regs = writer.ok("server_get_registers", json!({ "bank": "holding", "address": 0, "count": 1 }));
    assert_eq!(regs["values"], json!([0]), "nothing reached the server");
    assert_eq!(h.ok("modbus_read_holding", json!({ "address": 0, "count": 1 }))["values"], json!([0]));
}

#[test]
fn limits_and_bad_input_are_tool_errors() {
    let mut h = Harness::new(true);
    h.ok("server_start", json!({ "port": 0 }));
    assert!(h.err("server_get_registers", json!({ "bank": "holding", "address": 0, "count": 126 })).contains("'count'"));
    assert!(h.err("server_get_registers", json!({ "bank": "coil", "address": 0, "count": 1 })).contains("'bank'"));
    assert!(h.err("server_get_registers", json!({ "bank": "holding", "address": 65535, "count": 2 })).contains("past"));
    assert!(h.err("server_set_registers", json!({ "bank": "input", "address": 65535, "values": [1, 2] })).contains("past"));
    assert!(h.err("server_set_registers", json!({ "bank": "input", "address": 0, "values": [70000] })).contains("values[0]"));
    let too_many: Vec<u32> = vec![0; 124];
    assert!(h.err("modbus_write_multiple", json!({ "address": 0, "values": too_many })).contains("'values'"));
    assert!(h.err("modbus_connect", json!({ "host": "127.0.0.1", "port": 0 })).contains("'port'"));
    assert!(h.err("modbus_connect", json!({ "host": "bad host", "port": 502 })).contains("spaces"));
    // Setting input registers works (only possible on the simulation).
    h.ok("server_set_registers", json!({ "bank": "input", "address": 65535, "values": [42] }));
    let v = h.ok("server_get_registers", json!({ "bank": "input", "address": 65535, "count": 1 }));
    assert_eq!(v["values"], json!([42]));
}
