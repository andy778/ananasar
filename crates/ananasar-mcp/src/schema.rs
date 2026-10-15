//! Tool definitions (name, description, JSON Schema) and the small validator
//! that checks `tools/call` arguments against them.
//!
//! The validator understands exactly the subset of JSON Schema the tool
//! definitions use: an object with typed properties, `required`,
//! `additionalProperties: false`, and per property `type` (integer, string,
//! boolean, array), `minimum`/`maximum`, `enum`, `minItems`/`maxItems` and
//! `items`.

use serde_json::{Map, Value, json};

/// Largest register count of one read (Modbus limit for FC3/FC4).
pub const MAX_READ: i64 = 125;
/// Largest register count of one multi-register write (Modbus limit for FC16).
pub const MAX_WRITE: i64 = 123;
/// Default and bounds for the per-call reply timeout.
pub const DEFAULT_TIMEOUT_MS: i64 = 3000;
const MIN_TIMEOUT_MS: i64 = 50;
const MAX_TIMEOUT_MS: i64 = 60_000;

/// Behaviour hints sent as MCP tool annotations.
#[derive(Clone, Copy)]
struct Hint {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

/// Reads data without changing anything.
const READ_LOCAL: Hint = Hint { read_only: true, destructive: false, idempotent: true, open_world: false };
const READ_REMOTE: Hint = Hint { read_only: true, destructive: false, idempotent: true, open_world: true };
/// Changes only this process (connection or simulated server state).
const LOCAL_CHANGE: Hint = Hint { read_only: false, destructive: false, idempotent: true, open_world: false };
const CONNECT: Hint = Hint { read_only: false, destructive: false, idempotent: true, open_world: true };
/// Writes to a remote device.
const REMOTE_WRITE: Hint = Hint { read_only: false, destructive: true, idempotent: true, open_world: true };

fn int(desc: &str, min: i64, max: i64) -> Value {
    json!({ "type": "integer", "description": desc, "minimum": min, "maximum": max })
}

fn string(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

fn timeout() -> Value {
    int(
        "How long to wait for the device's reply, in milliseconds (default 3000).",
        MIN_TIMEOUT_MS,
        MAX_TIMEOUT_MS,
    )
}

fn u16_values(desc: &str, max_items: i64) -> Value {
    json!({
        "type": "array",
        "description": desc,
        "minItems": 1,
        "maxItems": max_items,
        "items": { "type": "integer", "minimum": 0, "maximum": 65535 },
    })
}

fn tool(name: &str, title: &str, description: &str, props: Value, required: &[&str], h: Hint) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": {
            "readOnlyHint": h.read_only,
            "destructiveHint": h.destructive,
            "idempotentHint": h.idempotent,
            "openWorldHint": h.open_world,
        },
    })
}

const WRITE_NOTE: &str = " WRITES TO THE REMOTE DEVICE and may change real hardware. Disabled unless the \
                          server was started with --allow-write; only use it when the user asked for \
                          this exact write.";

/// All tools, in a stable order.
pub fn definitions() -> Vec<Value> {
    let addr = || int("0-based register address as sent on the wire (65535 max).", 0, 65535);
    let bank = || json!({ "type": "string", "enum": ["holding", "input"], "description": "Register bank." });
    vec![
        tool(
            "ananasar_info",
            "Ananasar status",
            "Report the MCP server's mode (read-only or writes allowed), the bind address for the \
             simulated server, and the state of the Modbus client connection and the simulated server.",
            json!({}),
            &[],
            READ_LOCAL,
        ),
        tool(
            "modbus_connect",
            "Connect to a Modbus/TCP device",
            "Open a Modbus/TCP client connection to a device (replacing any existing connection). \
             Must be called before the other modbus_* tools. Addresses used by the other tools are \
             0-based wire addresses.",
            json!({
                "host": string("Host name or IP address of the device."),
                "port": int("TCP port (502 is the Modbus standard).", 1, 65535),
                "unit_id": int("Modbus unit id (slave address) put in every request. Default 1.", 0, 255),
            }),
            &["host", "port"],
            CONNECT,
        ),
        tool(
            "modbus_disconnect",
            "Disconnect from the Modbus device",
            "Close the current Modbus client connection.",
            json!({}),
            &[],
            LOCAL_CHANGE,
        ),
        tool(
            "modbus_read_holding",
            "Read holding registers (FC3)",
            "Read 1-125 holding registers starting at a 0-based address (function code 3).",
            json!({ "address": addr(), "count": int("Number of registers.", 1, MAX_READ), "timeout_ms": timeout() }),
            &["address", "count"],
            READ_REMOTE,
        ),
        tool(
            "modbus_read_input",
            "Read input registers (FC4)",
            "Read 1-125 input registers starting at a 0-based address (function code 4).",
            json!({ "address": addr(), "count": int("Number of registers.", 1, MAX_READ), "timeout_ms": timeout() }),
            &["address", "count"],
            READ_REMOTE,
        ),
        tool(
            "modbus_write_single",
            "Write one holding register (FC6)",
            &format!("Write one holding register (function code 6).{WRITE_NOTE}"),
            json!({
                "address": addr(),
                "value": int("Unsigned 16-bit value.", 0, 65535),
                "timeout_ms": timeout(),
            }),
            &["address", "value"],
            REMOTE_WRITE,
        ),
        tool(
            "modbus_write_multiple",
            "Write holding registers (FC16)",
            &format!("Write 1-123 consecutive holding registers (function code 16).{WRITE_NOTE}"),
            json!({
                "address": addr(),
                "values": u16_values("Unsigned 16-bit values, written to consecutive registers.", MAX_WRITE),
                "timeout_ms": timeout(),
            }),
            &["address", "values"],
            REMOTE_WRITE,
        ),
        tool(
            "modbus_mask_write",
            "Mask write a holding register (FC22)",
            &format!(
                "Mask write one holding register (function code 22): new = (current AND and_mask) OR \
                 (or_mask AND NOT and_mask).{WRITE_NOTE}"
            ),
            json!({
                "address": addr(),
                "and_mask": int("Bits kept from the current value.", 0, 65535),
                "or_mask": int("Bits set where and_mask is clear.", 0, 65535),
                "timeout_ms": timeout(),
            }),
            &["address", "and_mask", "or_mask"],
            REMOTE_WRITE,
        ),
        tool(
            "modbus_read_exception_status",
            "Read exception status (FC7)",
            "Read the device's exception status byte (function code 7).",
            json!({ "timeout_ms": timeout() }),
            &[],
            READ_REMOTE,
        ),
        tool(
            "server_start",
            "Start the simulated Modbus server",
            "Start a simulated Modbus/TCP server with an in-memory register file (65536 holding and \
             65536 input registers, at most 2 clients). It listens on the MCP server's bind address \
             (127.0.0.1 unless --bind was given). The port is required: there is no default, because \
             a fixed port may already be in use to simulate a real device. Use port 0 to let the OS \
             pick a free port; the result reports the actual port.",
            json!({
                "port": int("TCP port to listen on, or 0 for any free port.", 0, 65535),
                "unit_id": int("Unit id the server answers to; 0 (default) answers any unit id.", 0, 255),
                "log_mode": { "type": "string", "enum": ["raw", "interpretive"],
                    "description": "Data log detail: hex only (default) or hex plus decoded text." },
                "init_holding": { "type": "string", "enum": ["zero", "ascending"],
                    "description": "Initial holding registers: all 0 (default) or register N = N." },
                "init_input": { "type": "string", "enum": ["zero", "ascending"],
                    "description": "Initial input registers: all 0 (default) or register N = N." },
            }),
            &["port"],
            LOCAL_CHANGE,
        ),
        tool(
            "server_stop",
            "Stop the simulated Modbus server",
            "Stop the simulated server and free its port. Register contents and the log are discarded. \
             Clients that are still connected are served until they disconnect or go idle.",
            json!({}),
            &[],
            LOCAL_CHANGE,
        ),
        tool(
            "server_get_registers",
            "Read simulated server registers",
            "Read 1-125 registers of the simulated server's register file directly (no network).",
            json!({ "bank": bank(), "address": addr(), "count": int("Number of registers.", 1, MAX_READ) }),
            &["bank", "address", "count"],
            READ_LOCAL,
        ),
        tool(
            "server_set_registers",
            "Set simulated server registers",
            "Set 1-125 consecutive registers of the simulated server's register file. Input registers \
             can only be set this way (Modbus clients cannot write them). Affects only the simulation.",
            json!({
                "bank": bank(),
                "address": addr(),
                "values": u16_values("Unsigned 16-bit values.", MAX_READ),
            }),
            &["bank", "address", "values"],
            LOCAL_CHANGE,
        ),
        tool(
            "server_stats",
            "Simulated server statistics",
            "Show what the server panel of the Ananasar GUI shows: connected clients, per client the \
             last request's header fields, address, word count and byte totals, and the error flags \
             of the last request.",
            json!({}),
            &[],
            READ_LOCAL,
        ),
        tool(
            "server_options",
            "Get or set simulated server options",
            "Show the simulated server's options and change any that are given: the active function \
             codes, a forced exception reply, and the accepted unit id. Call with no arguments to \
             only read them.",
            json!({
                "active_function_codes": { "type": "array", "description":
                    "Function codes the server answers (others get Illegal Function). Allowed: 3, 4, 6, 7, 8, 16, 22, 23.",
                    "maxItems": 8, "items": { "type": "integer", "minimum": 1, "maximum": 23 } },
                "forced_exception": int(
                    "Answer every request with this Modbus exception code instead of processing it \
                     (1-8, 10, 11); 0 turns it off.", 0, 11),
                "unit_id": int("Unit id the server answers to; 0 answers any.", 0, 255),
            }),
            &[],
            LOCAL_CHANGE,
        ),
        tool(
            "server_get_log",
            "Read the simulated server data log",
            "Return the tail of the simulated server's data log (every request and reply, with the \
             connection number and time). The log lives in memory and keeps the most recent 1 MiB.",
            json!({
                "max_chars": int("Return at most this many characters from the end (default 8000).", 1, 100_000),
                "clear": { "type": "boolean", "description": "Empty the log after reading it (default false)." },
            }),
            &[],
            LOCAL_CHANGE,
        ),
    ]
}

/// Check `args` against a tool's `inputSchema`.
pub fn validate(schema: &Value, args: &Map<String, Value>) -> Result<(), String> {
    let props = schema.get("properties").and_then(Value::as_object);
    for key in args.keys() {
        if !props.is_some_and(|p| p.contains_key(key)) {
            return Err(format!("unknown argument '{key}'"));
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !args.contains_key(name) {
                return Err(format!("missing required argument '{name}'"));
            }
        }
    }
    for (key, value) in args {
        if let Some(sub) = props.and_then(|p| p.get(key)) {
            check_value(key, sub, value)?;
        }
    }
    Ok(())
}

fn check_value(path: &str, schema: &Value, v: &Value) -> Result<(), String> {
    match schema.get("type").and_then(Value::as_str) {
        Some("integer") => {
            let n = v.as_i64().ok_or_else(|| format!("'{path}' must be an integer"))?;
            let min = schema.get("minimum").and_then(Value::as_i64);
            let max = schema.get("maximum").and_then(Value::as_i64);
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return Err(format!(
                    "'{path}' is {n}, outside the allowed range {}..={}",
                    min.map_or_else(|| "-inf".to_owned(), |m| m.to_string()),
                    max.map_or_else(|| "inf".to_owned(), |m| m.to_string()),
                ));
            }
        }
        Some("string") => {
            let s = v.as_str().ok_or_else(|| format!("'{path}' must be a string"))?;
            if let Some(options) = schema.get("enum").and_then(Value::as_array)
                && !options.iter().any(|o| o.as_str() == Some(s))
            {
                let list: Vec<&str> = options.iter().filter_map(Value::as_str).collect();
                return Err(format!("'{path}' must be one of: {}", list.join(", ")));
            }
        }
        Some("boolean") => {
            if !v.is_boolean() {
                return Err(format!("'{path}' must be a boolean"));
            }
        }
        Some("array") => {
            let items = v.as_array().ok_or_else(|| format!("'{path}' must be an array"))?;
            let len = i64::try_from(items.len()).unwrap_or(i64::MAX);
            let min = schema.get("minItems").and_then(Value::as_i64).unwrap_or(0);
            let max = schema.get("maxItems").and_then(Value::as_i64).unwrap_or(i64::MAX);
            if len < min || len > max {
                return Err(format!("'{path}' has {len} items, expected {min}..={max}"));
            }
            if let Some(item_schema) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    check_value(&format!("{path}[{i}]"), item_schema, item)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
