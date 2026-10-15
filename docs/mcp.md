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

Use the full path to the binary in the commands below. `ananasar-mcp --help` lists its options and `ananasar-mcp --version` prints the version.

### Claude Code

```bash
claude mcp add ananasar -- /full/path/to/ananasar-mcp
# let it write to remote devices (see "Safety and threat model" first):
claude mcp add ananasar -- /full/path/to/ananasar-mcp --allow-write
```

`claude mcp list` shows whether the server connects.

### Claude Desktop

Edit `claude_desktop_config.json`, then restart Claude Desktop:

| System | Location |
| --- | --- |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |

```json
{
  "mcpServers": {
    "ananasar": {
      "command": "C:\path\to\ananasar-mcp.exe",
      "args": []
    }
  }
}
```

On macOS use a path such as `/Users/you/bin/ananasar-mcp`. Add `"--allow-write"` to `args` to enable writes. In JSON, backslashes in Windows paths must be doubled.

### Other clients

Any client that can start a stdio MCP server works: set the command to the binary and pass the options as arguments.

## Options

| Option | Meaning |
| --- | --- |
| *(none)* | Read-only: the write tools exist but refuse to run. |
| `--allow-write` | Enable `modbus_write_single`, `modbus_write_multiple` and `modbus_mask_write` against remote devices. |
| `--read-only` | The default, accepted so a config can say it explicitly. Combining it with `--allow-write` is an error. |
| `--bind <IP>` | Address the simulated server listens on. Default `127.0.0.1`. |

## Tools

All addresses are 0-based wire addresses (no address offset). Registers are unsigned 16-bit values (0-65535). Each call takes the arguments below and rejects any others. Failures (bad arguments, a device exception, a timeout, writes disabled) come back as tool results with `isError: true` and a message the assistant can act on; an unknown tool name is a JSON-RPC error.

`timeout_ms` is the reply timeout of the remote calls: 50 to 60 000 ms, default 3000.

### Client tools

Use `modbus_connect` first; the other `modbus_*` tools work on that connection.

| Tool | Parameters | What it does |
| --- | --- | --- |
| `ananasar_info` | none | Mode (read-only or writes allowed), bind address, and the state of the client connection and the simulated server. |
| `modbus_connect` | `host`, `port` (1-65535), optional `unit_id` (0-255, default 1) | Opens a connection, replacing any existing one. |
| `modbus_disconnect` | none | Closes it. |
| `modbus_read_holding` | `address`, `count` (1-125), optional `timeout_ms` | FC3. |
| `modbus_read_input` | `address`, `count` (1-125), optional `timeout_ms` | FC4. |
| `modbus_read_exception_status` | optional `timeout_ms` | FC7. |
| `modbus_write_single` | `address`, `value`, optional `timeout_ms` | FC6. **Write.** |
| `modbus_write_multiple` | `address`, `values` (1-123 numbers), optional `timeout_ms` | FC16. **Write.** |
| `modbus_mask_write` | `address`, `and_mask`, `or_mask`, optional `timeout_ms` | FC22: new value = (current AND `and_mask`) OR (`or_mask` AND NOT `and_mask`). **Write.** |

### Simulated server tools

| Tool | Parameters | What it does |
| --- | --- | --- |
| `server_start` | `port` (0-65535, **required**), optional `unit_id` (0 = any, the default), `log_mode` (`raw` default, or `interpretive`), `init_holding` and `init_input` (`zero` default, or `ascending`) | Starts the simulated server on the bind address. There is no default port; `0` picks a free port and the result reports it. One server at a time, at most two clients. |
| `server_stop` | none | Stops it and frees the port. Register contents and the log are discarded; clients still connected are served until they disconnect or go idle. |
| `server_get_registers` | `bank` (`holding` or `input`), `address`, `count` (1-125) | Reads registers straight from the simulation, without the network. |
| `server_set_registers` | `bank`, `address`, `values` (1-125 numbers) | Sets registers in either bank. This is the only way to set input registers. |
| `server_stats` | none | What the GUI's Server panel shows: clients, per client IP, poll interval, last request's transaction id, protocol id, length, unit id, function, start address and word count, byte totals and header check, plus the out-of-sync, incorrect length, incorrect protocol id and return exception flags. |
| `server_options` | optional `active_function_codes` (from 3, 4, 6, 7, 8, 16, 22, 23), `forced_exception` (1-8, 10, 11, or 0 for off), `unit_id` | With no arguments, reads the options; with arguments, changes those given. |
| `server_get_log` | optional `max_chars` (1-100 000, default 8000), `clear` (default false) | Tail of the data log. The log is kept in memory, the most recent 1 MiB. |

Each tool also carries the MCP annotations `readOnlyHint`, `destructiveHint`, `idempotentHint` and `openWorldHint`, so clients can ask for confirmation before the destructive ones.

## Example prompts

| Prompt | Tools it uses |
| --- | --- |
| "Show me the state of Ananasar." | `ananasar_info` |
| "Connect to 192.168.1.50:502, unit 3, and read holding registers 0-9. Don't write anything." | `modbus_connect`, `modbus_read_holding` |
| "Read input registers 30-31 from the connected device and tell me the 32-bit float they form." | `modbus_read_input` |
| "Does the device report an exception status?" | `modbus_read_exception_status` |
| "Write 1 to holding register 200." (needs `--allow-write`) | `modbus_write_single` |
| "Write 10, 20, 30 to holding registers 300-302." (needs `--allow-write`) | `modbus_write_multiple` |
| "Set bit 3 of holding register 5 without changing the other bits." (needs `--allow-write`) | `modbus_mask_write` |
| "Disconnect from the device." | `modbus_disconnect` |
| "Start a simulated server on port 15021 with ascending holding registers." | `server_start` |
| "On the simulator, set input registers 30-31 to the float 12.5 (0x4148 0x0000), then read them back as FC4 through the client." | `server_set_registers`, `modbus_connect`, `modbus_read_input` |
| "What is in holding registers 100-103 on the simulator?" | `server_get_registers` |
| "Show me what my client sent to the simulator." | `server_stats`, `server_get_log` |
| "Turn off function code 16 on the simulated server and see whether my client falls back to FC6." | `server_options` |
| "Make the simulated server answer every request with exception 6 (busy), then check what my client does." | `server_options`, `server_stats` |
| "Stop the simulated server." | `server_stop` |

## Safety and threat model

An MCP tool call is made by a language model, on the basis of text it read, which may include text from places you do not control. So the server is built to limit what a wrong or manipulated call can do.

- **Writes to remote devices change real hardware.** They are off unless you start the server with `--allow-write`, and each write tool says so in its description. Most MCP clients also ask you to approve each tool call; keep that on for the write tools. Modbus/TCP has no authentication, so anything that can reach a device can write to it.
- **The simulated server listens on `127.0.0.1`** unless you pass `--bind`. The bind address is a startup option, not a tool argument, so the assistant cannot widen it. It only holds a register file in memory and writes nothing to disk. Use a port that is free; the server never guesses one, so it will not collide with a port you use for a real device simulation (for example 15020), unless you ask for that port.
- **Sizes are capped** (125 registers per read, 123 per write, 1 MiB per input line, 1 MiB of log). Timeouts are bounded (50 ms to 60 s).
- **The client can connect to any host** the machine can reach, which is the point of the tool. If that is too much, run `ananasar-mcp` in a container or on a host whose network access you have restricted.
- **Output is data.** Register contents and log text come from remote devices; a client should treat them as untrusted, like any tool output.
- Nothing is written to stdout except protocol messages; diagnostics go to stderr. The server uses no network except the connections you ask for.

Read-only mode only stops this server's own write tools; it is not a network firewall.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| The client does not list the server, or it shows as failed | Run the binary by hand (`ananasar-mcp --version`) to see that it starts. Use the full path in the configuration, and on Windows double the backslashes in JSON. Restart Claude Desktop after editing its configuration. |
| "Writes are disabled" or a similar error from a write tool | The server runs read-only. Add `--allow-write` to the command and restart the client. |
| `server_start` fails with a bind error | The port is in use or needs privileges (ports below 1024 on Linux and macOS). Use another port, or `0` for any free one. |
| Another machine cannot reach the simulated server | It listens on `127.0.0.1` by default; start `ananasar-mcp` with `--bind 0.0.0.0` (or a specific address). Also check the firewall. |
| Reads time out | Check the unit id given to `modbus_connect`, remember that addresses are 0-based, and raise `timeout_ms`. See [Troubleshooting](troubleshooting.md#no-reply-from-a-device). |
| A tool says the address range is invalid | At most 125 registers per read and 123 per write; the range must stay within 0-65535. |
| Nothing at all happens | The server writes protocol messages to stdout and diagnostics to stderr. Look at the client's MCP log for stderr output. |

## Design notes

- **Where it lives:** a separate crate and binary (`crates/ananasar-mcp`) instead of an `ananasar mcp` subcommand. The CLI stays free of the JSON dependency, and an MCP entry in a config file can point at a binary that has no other behaviour. It depends only on `ananasar-core`, `ananasar-net` and `serde_json`.
- **Implementation:** the stdio transport is newline-delimited JSON-RPC 2.0, which is small enough to write by hand (`initialize`, `ping`, `tools/list`, `tools/call`). That avoids an async runtime and a large SDK, builds on Rust 1.92 like the core, net and CLI crates, and keeps the supply chain short. Tool arguments are validated against the same JSON Schema the server publishes.
- **Protocol versions:** it answers the `initialize` handshake of revisions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05, echoing the version the client asks for. The newer stateless revision (2026-07-28, no `initialize`) is not implemented; clients that speak both fall back to the handshake.
- **Not included:** resources and prompts, multiple simultaneous client connections, TLS, per-host allow lists.

## See also

- [Getting started](getting-started.md)
- [CLI manual](cli.md) for the same engine from a shell
- [GUI manual](gui.md) for the Server panel that `server_stats` mirrors
- [Troubleshooting](troubleshooting.md)
