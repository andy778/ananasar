# MCP server (`ananasar-mcp`)

`ananasar-mcp` lets an AI assistant (Claude Code, Claude Desktop or any other [Model Context Protocol](https://modelcontextprotocol.io) client) use Ananasar's engine:

- as a **Modbus/TCP client** against real devices (read registers; write them only if you allow it), and
- to **run and inspect a simulated Modbus server** on your machine: set register values, change which function codes it answers, force exception replies, and read its statistics and data log.

Typical uses: testing a PLC or gateway integration, reproducing a device's behaviour while an assistant helps debug a client, and exploring an unfamiliar device's register map.

## Install and register

Download `ananasar-mcp` (`ananasar-mcp.exe` on Windows) from the [release page](https://github.com/andy778/ananasar/releases/latest), or build it:

```bash
cargo install --path crates/ananasar-mcp   # from a checkout; or: cargo build --release -p ananasar-mcp
```

**Claude Code**

```bash
claude mcp add ananasar -- /full/path/to/ananasar-mcp
# let it write to remote devices (see "Safety" first):
claude mcp add ananasar -- /full/path/to/ananasar-mcp --allow-write
```

**Claude Desktop** (`claude_desktop_config.json`; on Windows use `C:\\path\\to\\ananasar-mcp.exe`)

```json
{
  "mcpServers": {
    "ananasar": {
      "command": "/full/path/to/ananasar-mcp",
      "args": []
    }
  }
}
```

Add `"--allow-write"` to `args` to enable writes. Restart the client after editing the file.

## Options

| Option | Meaning |
| --- | --- |
| *(none)* | Read-only: the write tools exist but refuse to run. |
| `--allow-write` | Enable `modbus_write_single`, `modbus_write_multiple` and `modbus_mask_write` against remote devices. |
| `--read-only` | The default, accepted so a config can say it explicitly. Combining it with `--allow-write` is an error. |
| `--bind <IP>` | Address the simulated server listens on. Default `127.0.0.1`. |

## Tools

All addresses are 0-based wire addresses. Registers are unsigned 16-bit values. Failures (bad arguments, a device exception, a timeout, writes disabled) come back as tool results with `isError: true` and a message the assistant can act on; an unknown tool name is a JSON-RPC error. Every call takes the arguments listed in its `inputSchema` and rejects any others.

| Tool | What it does |
| --- | --- |
| `ananasar_info` | Mode (read-only or not), bind address, state of the client connection and the simulated server. |
| `modbus_connect` | `host`, `port`, optional `unit_id` (default 1). Opens a connection, replacing any existing one. |
| `modbus_disconnect` | Closes it. |
| `modbus_read_holding` | FC3: `address`, `count` 1-125, optional `timeout_ms`. |
| `modbus_read_input` | FC4: same arguments. |
| `modbus_write_single` | FC6: `address`, `value`. **Write.** |
| `modbus_write_multiple` | FC16: `address`, `values` (1-123). **Write.** |
| `modbus_mask_write` | FC22: `address`, `and_mask`, `or_mask`. **Write.** |
| `modbus_read_exception_status` | FC7. |
| `server_start` | Starts the simulated server. `port` is **required** (there is no default; `0` picks a free port and the result reports it). Optional `unit_id` (0 = any), `log_mode` (`raw`/`interpretive`), `init_holding` / `init_input` (`zero`/`ascending`). One server at a time, at most two clients. |
| `server_stop` | Stops it and frees the port. |
| `server_get_registers` | Reads 1-125 registers of the `holding` or `input` bank straight from the simulation. |
| `server_set_registers` | Sets 1-125 registers in either bank (input registers can only be set this way). |
| `server_stats` | What the GUI's Server panel shows: clients, per client IP, poll interval, last request's transaction id / protocol id / length / unit id / function / start address / word count, byte totals, header check, plus the out-of-sync, incorrect length, incorrect protocol id and return exception flags. |
| `server_options` | Reads, and with arguments changes, the active function codes (3, 4, 6, 7, 8, 16, 22, 23), a forced exception reply (`forced_exception`: 1-8, 10, 11, or 0 for off) and the accepted `unit_id`. |
| `server_get_log` | Tail of the data log (`max_chars`, `clear`). The log is kept in memory, most recent 1 MiB. |

Each tool also carries the MCP annotations `readOnlyHint`, `destructiveHint`, `idempotentHint` and `openWorldHint`, so clients can ask for confirmation before the destructive ones.

## Example prompts

- "Start a simulated Modbus server on port 15021 with ascending holding registers, connect to it and read registers 10 to 14."
- "Make the simulated server answer every request with exception 6 (busy), then show me what my client does against it, and check `server_stats` for what it sent."
- "Connect to 192.168.1.50:502, unit 3, and read holding registers 0-9. Don't write anything."
- "On the simulator, set input registers 30-31 to a float 12.5 (0x4148 0x0000), then read them back as FC4."
- "Turn off function code 16 on the simulated server and see whether my client falls back to FC6."

## Safety and threat model

An MCP tool call is made by a language model, on the basis of text it read, which may include text from places you do not control. So the server is built to limit what a wrong or manipulated call can do.

- **Writes to remote devices change real hardware.** They are off unless you start the server with `--allow-write`, and each write tool says so in its description. Most MCP clients also ask you to approve each tool call; keep that on for the write tools. Modbus/TCP has no authentication, so anything that can reach a device can write to it.
- **The simulated server listens on `127.0.0.1`** unless you pass `--bind`. The bind address is a startup option, not a tool argument, so the assistant cannot widen it. It only holds a register file in memory and writes nothing to disk. Use a port that is free; the server never guesses one, so it will not collide with a port you use for a real device simulation (for example 15020), unless you ask for that port.
- **Sizes are capped** (125 registers per read, 123 per write, 1 MiB per input line, 1 MiB of log). Timeouts are bounded (50 ms to 60 s).
- **The client can connect to any host** the machine can reach, which is the point of the tool. If that is too much, run `ananasar-mcp` in a container or on a host whose network access you have restricted.
- **Output is data.** Register contents and log text come from remote devices; a client should treat them as untrusted, like any tool output.
- Nothing is written to stdout except protocol messages; diagnostics go to stderr. The server uses no network except the connections you ask for.

Read-only mode only stops this server's own write tools; it is not a network firewall.

## Design notes

- **Where it lives:** a separate crate and binary (`crates/ananasar-mcp`) instead of an `ananasar mcp` subcommand. The CLI stays free of the JSON dependency, and an MCP entry in a config file can point at a binary that has no other behaviour. It depends only on `ananasar-core`, `ananasar-net` and `serde_json`.
- **Implementation:** the stdio transport is newline-delimited JSON-RPC 2.0, which is small enough to write by hand (`initialize`, `ping`, `tools/list`, `tools/call`). That avoids an async runtime and a large SDK, builds on Rust 1.92 like the core, net and CLI crates, and keeps the supply chain short. Tool arguments are validated against the same JSON Schema the server publishes.
- **Protocol versions:** it answers the `initialize` handshake of revisions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05, echoing the version the client asks for. The newer stateless revision (2026-07-28, no `initialize`) is not implemented; clients that speak both fall back to the handshake.
- **Not included:** resources and prompts, multiple simultaneous client connections, TLS, per-host allow lists.
