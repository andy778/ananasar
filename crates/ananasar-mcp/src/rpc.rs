//! JSON-RPC 2.0 framing and the MCP methods this server answers, for the
//! stdio transport: one JSON message per line.
//!
//! Implemented: `initialize` (the handshake of the 2024-11-05 through
//! 2025-11-25 revisions), `ping`, `tools/list` and `tools/call`.
//! Notifications are accepted and ignored. Nothing here writes to stdout; the
//! caller sends the returned line.

use crate::Mcp;
use serde_json::{Map, Value, json};

/// Protocol versions this server can speak, newest first.
pub const SUPPORTED_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
/// The message is not a valid JSON-RPC request.
pub const INVALID_REQUEST: i64 = -32600;
/// Unknown method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// Bad method parameters (also an unknown tool).
pub const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Ananasar Modbus/TCP tools. modbus_* tools act as a Modbus client against a device: \
call modbus_connect first, then read or write registers (addresses are 0-based wire addresses). \
Writes to real devices can change physical equipment: only write when the user asked for it, and \
read back to verify. server_* tools run a simulated Modbus server on this machine for testing; \
server_start needs an explicit port and never guesses one.";

struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

fn error_line(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

/// Handle one incoming line. Returns the line to send back, or `None` for
/// notifications and for messages that need no answer.
pub fn handle_line(mcp: &mut Mcp, line: &str) -> Option<String> {
    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error_line(Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
    };
    let Value::Object(msg) = value else {
        return Some(error_line(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON-RPC request object (batches are not supported)",
        ));
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // A response to a request we never sent, or garbage.
        if msg.contains_key("result") || msg.contains_key("error") {
            return None;
        }
        return Some(error_line(Value::Null, INVALID_REQUEST, "missing 'method'"));
    };
    let Some(id) = msg.get("id") else {
        return None; // notification (e.g. notifications/initialized)
    };
    if !(id.is_string() || id.is_number()) {
        return Some(error_line(Value::Null, INVALID_REQUEST, "'id' must be a string or a number"));
    }
    let response = match dispatch(mcp, method, msg.get("params")) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } }),
    };
    Some(response.to_string())
}

fn dispatch(mcp: &mut Mcp, method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": mcp.tools() })),
        "tools/call" => tools_call(mcp, params),
        _ => Err(RpcError::new(METHOD_NOT_FOUND, format!("method not found: {method}"))),
    }
}

fn initialize(params: Option<&Value>) -> Value {
    let requested = params.and_then(|p| p.get("protocolVersion")).and_then(Value::as_str);
    // Echo the client's version if we speak it, otherwise offer our newest.
    let version = requested
        .and_then(|r| SUPPORTED_VERSIONS.iter().find(|v| **v == r))
        .unwrap_or(&SUPPORTED_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "ananasar-mcp",
            "title": "Ananasar Modbus/TCP",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
    })
}

fn tools_call(mcp: &mut Mcp, params: Option<&Value>) -> Result<Value, RpcError> {
    let params = params
        .and_then(Value::as_object)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call needs a params object"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call needs a string 'name'"))?;
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err(RpcError::new(INVALID_PARAMS, "'arguments' must be an object")),
    };
    let outcome = mcp
        .call_tool(name, args)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("unknown tool: {name}")))?;
    Ok(match outcome {
        Ok(data) => {
            let text = serde_json::to_string_pretty(&data).unwrap_or_else(|_| data.to_string());
            json!({ "content": [{ "type": "text", "text": text }], "structuredContent": data, "isError": false })
        }
        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn run(mcp: &mut Mcp, line: &str) -> Value {
        let out = handle_line(mcp, line).expect("a response");
        serde_json::from_str(&out).expect("valid JSON")
    }

    fn mcp() -> Mcp {
        Mcp::new(Config::default())
    }

    #[test]
    fn initialize_echoes_a_supported_version() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "ananasar-mcp");
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_offers_newest_for_unknown_version() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r["id"], "a");
        assert_eq!(r["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_answer() {
        let mut m = mcp();
        assert!(handle_line(&mut m, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(handle_line(&mut m, r#"{"jsonrpc":"2.0","id":3,"result":{}}"#).is_none());
    }

    #[test]
    fn ping_and_unknown_method() {
        let mut m = mcp();
        assert_eq!(run(&mut m, r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#)["result"], json!({}));
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#);
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn malformed_input_is_answered_with_errors() {
        let mut m = mcp();
        assert_eq!(run(&mut m, "{not json")["error"]["code"], PARSE_ERROR);
        assert_eq!(run(&mut m, "[1,2]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(run(&mut m, r#"{"jsonrpc":"2.0","id":1}"#)["error"]["code"], INVALID_REQUEST);
        assert_eq!(run(&mut m, r#"{"jsonrpc":"2.0","id":{},"method":"ping"}"#)["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn tools_list_has_schemas_for_every_tool() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 16);
        for t in tools {
            let name = t["name"].as_str().unwrap();
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name}");
            assert!(!t["description"].as_str().unwrap().is_empty());
            let schema = &t["inputSchema"];
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            let props = schema["properties"].as_object().unwrap();
            for req in schema["required"].as_array().unwrap() {
                assert!(props.contains_key(req.as_str().unwrap()), "{name}: required {req} not a property");
            }
            assert!(t["annotations"]["readOnlyHint"].is_boolean());
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let m = mcp();
        let mut names: Vec<&str> = m.tools().iter().filter_map(|t| t["name"].as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn unknown_tool_is_a_protocol_error_but_bad_arguments_are_tool_errors() {
        let mut m = mcp();
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_read_holding","arguments":{"address":0,"count":126}}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("'count'"));
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_read_holding","arguments":{"address":0}}}"#);
        assert_eq!(r["result"]["isError"], true);
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_read_holding","arguments":{"address":0,"count":1,"extra":1}}}"#);
        assert_eq!(r["result"]["isError"], true);
        let r = run(&mut m, r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_read_holding","arguments":[1]}}"#);
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn tool_needing_a_connection_reports_it() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_read_holding","arguments":{"address":0,"count":1}}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("modbus_connect"));
    }

    #[test]
    fn writes_are_refused_in_read_only_mode_before_anything_else() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"modbus_write_single","arguments":{"address":0,"value":1}}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("read-only"));
    }

    #[test]
    fn server_start_requires_a_port() {
        let r = run(&mut mcp(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"server_start","arguments":{}}}"#);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("'port'"));
    }
}
