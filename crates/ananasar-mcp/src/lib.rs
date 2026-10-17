//! `ananasar-mcp`: a Model Context Protocol server that lets an AI assistant
//! use Ananasar's Modbus/TCP client and simulated server.
//!
//! The protocol layer ([`rpc`]) is a small hand-written JSON-RPC 2.0 handler
//! for the stdio transport; [`Mcp`] holds the tool state and implements the
//! tools on top of `ananasar-core` and `ananasar-net`.
#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, reason = "test code: a panic is a failed test")
)]

pub mod rpc;
pub mod schema;
mod session;
mod sim;
mod stdio;

use ananasar_core::log::LogMode;
use ananasar_core::registers::{Bank, InitMode};
use ananasar_core::sync::lock;
use serde_json::{Map, Value, json};
use session::{Fail, Session, exception_name};
use sim::{FUNCTION_CODES, Sim, exception_from_code};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub use stdio::serve;

/// What the tools may do.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Allow the `modbus_write_*` and `modbus_mask_write` tools.
    pub allow_write: bool,
    /// Address the simulated server binds to.
    pub bind: IpAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self { allow_write: false, bind: IpAddr::V4(Ipv4Addr::LOCALHOST) }
    }
}

/// A tool's outcome: structured data, or a message for the assistant.
pub type ToolResult = Result<Value, String>;

/// The tool state: at most one client connection and one simulated server.
pub struct Mcp {
    cfg: Config,
    session: Option<Session>,
    sim: Option<Sim>,
    tools: Vec<Value>,
}

type Args = Map<String, Value>;

fn int(a: &Args, key: &str) -> Option<i64> {
    a.get(key).and_then(Value::as_i64)
}

fn req_u16(a: &Args, key: &str) -> Result<u16, String> {
    int(a, key)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| format!("missing or invalid '{key}'"))
}

fn opt_u8(a: &Args, key: &str, default: u8) -> u8 {
    int(a, key).and_then(|n| u8::try_from(n).ok()).unwrap_or(default)
}

fn words(a: &Args, key: &str) -> Vec<u16> {
    a.get(key)
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(|n| n.as_i64().and_then(|n| u16::try_from(n).ok())).collect())
        .unwrap_or_default()
}

fn timeout(a: &Args) -> Duration {
    let ms = int(a, "timeout_ms").unwrap_or(schema::DEFAULT_TIMEOUT_MS);
    Duration::from_millis(u64::try_from(ms).unwrap_or(3000))
}

fn bank(a: &Args) -> Bank {
    if a.get("bank").and_then(Value::as_str) == Some("input") { Bank::Input } else { Bank::Holding }
}

fn init_mode(a: &Args, key: &str) -> InitMode {
    if a.get(key).and_then(Value::as_str) == Some("ascending") { InitMode::Ascending } else { InitMode::Zero }
}

fn hex_list(values: &[u16]) -> Vec<String> {
    values.iter().map(|v| format!("0x{v:04X}")).collect()
}

impl Mcp {
    /// A fresh instance with no connection and no server.
    pub fn new(cfg: Config) -> Self {
        Self { cfg, session: None, sim: None, tools: schema::definitions() }
    }

    /// The tool definitions, as returned by `tools/list`.
    pub fn tools(&self) -> &[Value] {
        &self.tools
    }

    /// Whether the tool `name` writes to a remote device.
    fn is_remote_write(name: &str) -> bool {
        matches!(name, "modbus_write_single" | "modbus_write_multiple" | "modbus_mask_write")
    }

    /// Run a tool. `None` means there is no such tool (a protocol error);
    /// otherwise the tool's result, with invalid arguments reported as an `Err`.
    pub fn call_tool(&mut self, name: &str, args: &Args) -> Option<ToolResult> {
        let def = self.tools.iter().find(|t| t.get("name").and_then(Value::as_str) == Some(name))?;
        if let Some(schema) = def.get("inputSchema")
            && let Err(e) = schema::validate(schema, args)
        {
            return Some(Err(format!("invalid arguments for {name}: {e}")));
        }
        if Self::is_remote_write(name) && !self.cfg.allow_write {
            return Some(Err(format!(
                "{name} is disabled: the MCP server is running in read-only mode, so nothing is written \
                 to remote devices. Ask the user to restart it with --allow-write if they really want this."
            )));
        }
        Some(match name {
            "ananasar_info" => Ok(self.info()),
            "modbus_connect" => self.connect(args),
            "modbus_disconnect" => Ok(self.disconnect()),
            "modbus_read_holding" => self.read(args, false),
            "modbus_read_input" => self.read(args, true),
            "modbus_write_single" => self.write_single(args),
            "modbus_write_multiple" => self.write_multiple(args),
            "modbus_mask_write" => self.mask_write(args),
            "modbus_read_exception_status" => self.exception_status(args),
            "server_start" => self.server_start(args),
            "server_stop" => self.server_stop(),
            "server_get_registers" => self.server_get(args),
            "server_set_registers" => self.server_set(args),
            "server_stats" => self.server_stats(),
            "server_options" => self.server_options(args),
            "server_get_log" => self.server_log(args),
            _ => return None,
        })
    }

    fn info(&self) -> Value {
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "mode": if self.cfg.allow_write { "writes allowed" } else { "read-only" },
            "remote_writes_allowed": self.cfg.allow_write,
            "simulated_server_bind": self.cfg.bind.to_string(),
            "client": self.session.as_ref().map(|s| json!({ "host": s.host, "port": s.port, "unit_id": s.unit_id() })),
            "server": self.sim.as_ref().map(|s| json!({
                "listening": s.addr.to_string(),
                "clients": s.server.clients.load(Ordering::SeqCst),
            })),
        })
    }

    // ----- client -----

    fn connect(&mut self, a: &Args) -> ToolResult {
        let host = a.get("host").and_then(Value::as_str).unwrap_or_default();
        let port = req_u16(a, "port")?;
        self.session = None;
        let s = Session::open(host, port, opt_u8(a, "unit_id", 1))?;
        let out = json!({ "connected": true, "host": s.host, "port": s.port, "unit_id": s.unit_id() });
        self.session = Some(s);
        Ok(out)
    }

    fn disconnect(&mut self) -> Value {
        json!({ "was_connected": self.session.take().is_some() })
    }

    /// Run `f` on the open connection, closing it if the failure was fatal.
    fn with_session<T>(&mut self, f: impl FnOnce(&mut Session) -> Result<T, Fail>) -> Result<T, String> {
        let Some(s) = self.session.as_mut() else {
            return Err("not connected: call modbus_connect first".into());
        };
        match f(s) {
            Ok(v) => Ok(v),
            Err(fail) => {
                if fail.fatal {
                    self.session = None;
                    Err(format!("{}; the connection was closed, call modbus_connect again", fail.msg))
                } else {
                    Err(fail.msg)
                }
            }
        }
    }

    fn read(&mut self, a: &Args, input: bool) -> ToolResult {
        let (addr, count) = (req_u16(a, "address")?, req_u16(a, "count")?);
        let t = timeout(a);
        let values = self.with_session(|s| s.read(input, addr, count, t))?;
        Ok(json!({
            "bank": if input { "input" } else { "holding" },
            "address": addr,
            "count": values.len(),
            "values": values,
            "hex": hex_list(&values),
        }))
    }

    fn write_single(&mut self, a: &Args) -> ToolResult {
        let (addr, value) = (req_u16(a, "address")?, req_u16(a, "value")?);
        let t = timeout(a);
        self.with_session(|s| s.write_single(addr, value, t))?;
        Ok(json!({ "written": true, "address": addr, "value": value }))
    }

    fn write_multiple(&mut self, a: &Args) -> ToolResult {
        let addr = req_u16(a, "address")?;
        let values = words(a, "values");
        let t = timeout(a);
        self.with_session(|s| s.write_multiple(addr, &values, t))?;
        Ok(json!({ "written": true, "address": addr, "count": values.len() }))
    }

    fn mask_write(&mut self, a: &Args) -> ToolResult {
        let (addr, and_mask, or_mask) = (req_u16(a, "address")?, req_u16(a, "and_mask")?, req_u16(a, "or_mask")?);
        let t = timeout(a);
        self.with_session(|s| s.mask_write(addr, and_mask, or_mask, t))?;
        Ok(json!({ "written": true, "address": addr, "and_mask": and_mask, "or_mask": or_mask }))
    }

    fn exception_status(&mut self, a: &Args) -> ToolResult {
        let t = timeout(a);
        let status = self.with_session(|s| s.exception_status(t))?;
        Ok(json!({ "status": status, "hex": format!("0x{status:02X}") }))
    }

    // ----- simulated server -----

    fn sim(&self) -> Result<&Sim, String> {
        self.sim.as_ref().ok_or_else(|| "the simulated server is not running: call server_start first".to_owned())
    }

    fn server_start(&mut self, a: &Args) -> ToolResult {
        if let Some(s) = &self.sim {
            return Err(format!("the simulated server is already running on {}; call server_stop first", s.addr));
        }
        let mode = if a.get("log_mode").and_then(Value::as_str) == Some("interpretive") {
            LogMode::Interpretive
        } else {
            LogMode::Raw
        };
        let sim = Sim::start(
            self.cfg.bind,
            req_u16(a, "port")?,
            opt_u8(a, "unit_id", 0),
            mode,
            (init_mode(a, "init_holding"), init_mode(a, "init_input")),
        )?;
        let out = json!({ "running": true, "listening": sim.addr.to_string(), "port": sim.addr.port() });
        self.sim = Some(sim);
        Ok(out)
    }

    fn server_stop(&mut self) -> ToolResult {
        let mut sim = self.sim.take().ok_or("the simulated server is not running")?;
        sim.stop();
        Ok(json!({ "running": false }))
    }

    fn server_get(&self, a: &Args) -> ToolResult {
        let (addr, count) = (req_u16(a, "address")?, req_u16(a, "count")?);
        let sim = self.sim()?;
        let regs = lock(&sim.server.regs);
        let values: Vec<u16> = regs
            .read_span(bank(a), addr, count)
            .ok_or_else(|| format!("address {addr} + count {count} runs past register 65535"))?
            .to_vec();
        Ok(json!({ "address": addr, "count": values.len(), "values": values, "hex": hex_list(&values) }))
    }

    fn server_set(&mut self, a: &Args) -> ToolResult {
        let addr = req_u16(a, "address")?;
        let values = words(a, "values");
        let bank = bank(a);
        let sim = self.sim()?;
        let count = u16::try_from(values.len()).map_err(|_| "too many values".to_owned())?;
        if addr.checked_add(count.saturating_sub(1)).is_none() {
            return Err(format!("address {addr} + {count} registers runs past register 65535"));
        }
        let mut regs = lock(&sim.server.regs);
        for (a, v) in (addr..=u16::MAX).zip(&values) {
            regs.set(bank, a, *v);
        }
        Ok(json!({ "set": true, "address": addr, "count": count }))
    }

    fn server_stats(&self) -> ToolResult {
        let sim = self.sim()?;
        let st = lock(&sim.server.stats);
        let slots: Vec<Value> = st
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| match slot {
                None => json!({ "slot": i.saturating_add(1), "connected": false }),
                Some(c) => json!({
                    "slot": i.saturating_add(1),
                    "connected": true,
                    "ip": c.ip,
                    "nagle": c.nagle,
                    "poll_interval_ms": c.poll_interval_ms,
                    "packet_size": c.packet_size,
                    "header_check": match &c.header_check {
                        None => Value::Null,
                        Some(Ok(())) => json!("ok"),
                        Some(Err(e)) => json!(e),
                    },
                    "transaction_id": c.transaction_id,
                    "protocol_id": c.protocol_id,
                    "length": c.length,
                    "unit_id": c.unit_id,
                    "function": c.function,
                    "start_address": c.start_address,
                    "word_count": c.word_count,
                    "bytes_received": c.bytes_received,
                    "bytes_sent": c.bytes_sent,
                }),
            })
            .collect();
        Ok(json!({
            "listening": sim.addr.to_string(),
            "clients": sim.server.clients.load(Ordering::SeqCst),
            "slots": slots,
            "out_of_sync": st.out_of_sync,
            "incorrect_length": st.incorrect_length,
            "incorrect_protocol_id": st.incorrect_protocol_id,
            "return_exception": st.return_exception,
            "log_entries": sim.log_entries(),
        }))
    }

    fn server_options(&self, a: &Args) -> ToolResult {
        let sim = self.sim()?;
        let mut cfg = lock(&sim.server.cfg);
        if a.contains_key("active_function_codes") {
            let codes = words(a, "active_function_codes");
            if let Some(bad) = codes.iter().find(|c| !FUNCTION_CODES.iter().any(|f| u16::from(*f) == **c)) {
                return Err(format!("function code {bad} is not supported; allowed: {FUNCTION_CODES:?}"));
            }
            cfg.supported = codes.iter().filter_map(|c| u8::try_from(*c).ok()).collect();
        }
        if let Some(code) = int(a, "forced_exception") {
            let code = u8::try_from(code).unwrap_or(0);
            cfg.forced_exception = if code == 0 {
                None
            } else {
                Some(exception_from_code(code).ok_or_else(|| format!("exception code {code} cannot be forced; use 1-8, 10 or 11"))?)
            };
        }
        if a.contains_key("unit_id") {
            cfg.unit_id = opt_u8(a, "unit_id", 0);
        }
        let c = &mut *cfg;
        for (key, flag) in [
            ("return_incorrect_ti", &mut c.wrong_transaction_id),
            ("return_incorrect_protocol_id", &mut c.wrong_protocol_id),
            ("return_incorrect_length_high_byte", &mut c.wrong_length_high),
            ("reply_out_of_synch", &mut c.out_of_synch),
            ("use_address_offset", &mut c.address_offset),
        ] {
            if let Some(on) = a.get(key).and_then(Value::as_bool) {
                *flag = on;
            }
        }
        let forced = cfg.forced_exception.map_or(0, |e| e as u8);
        Ok(json!({
            "active_function_codes": cfg.supported.iter().collect::<Vec<_>>(),
            "forced_exception": forced,
            "forced_exception_name": if forced == 0 { Value::Null } else { json!(exception_name(forced)) },
            "unit_id": cfg.unit_id,
            "unit_id_note": "0 answers any unit id",
            "return_incorrect_ti": cfg.wrong_transaction_id,
            "return_incorrect_protocol_id": cfg.wrong_protocol_id,
            "return_incorrect_length_high_byte": cfg.wrong_length_high,
            "reply_out_of_synch": cfg.out_of_synch,
            "use_address_offset": cfg.address_offset,
        }))
    }

    fn server_log(&self, a: &Args) -> ToolResult {
        let max = usize::try_from(int(a, "max_chars").unwrap_or(8000)).unwrap_or(8000);
        let clear = a.get("clear").and_then(Value::as_bool).unwrap_or(false);
        let sim = self.sim()?;
        Ok(json!({ "entries": sim.log_entries(), "log": sim.log_tail(max, clear) }))
    }
}
