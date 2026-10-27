# CLI manual

The `ananasar` command runs a Modbus/TCP server, polls a server, scans for registers that answer and generates shell completions, with the same engine as the GUI. The same program also opens the GUI (`ananasar gui`) and runs the [MCP server](mcp.md) (`ananasar mcp`); both are on by default and left out of a `--no-default-features` build, which is the small headless CLI.

```text
ananasar <COMMAND>

Commands:
  server       Run a Modbus/TCP server backed by an in-memory register file
  client       Poll a Modbus/TCP server and print the responses
  storm       Drive a Modbus/TCP target at a controlled load
  completions  Print a shell completion script to stdout
  scan         Read a range of registers one by one and report which respond
```

`ananasar --version` prints the version and `ananasar <command> --help` lists the options of a command. All register addresses are 0-based wire addresses; there is no address offset option.

## ananasar server

Runs a server with 65 536 holding and 65 536 input registers, all 0 at the start. It serves until you stop it with Ctrl+C. At most two clients are served at a time; a connection that sends nothing for a minute is closed.

| Option | Default | Meaning |
| --- | --- | --- |
| `-p`, `--port <PORT>` | 502 | TCP port to listen on. Unlike the GUI, the CLI does not fall back to another port; a busy port is an error. Port 502 usually needs root on Linux and macOS. |
| `-u`, `--uid <UID>` | 0 | Accepted unit id. 0 accepts any. Requests for another unit id get no reply. |
| `--load <FILE>` | none | Load register values from a `*.ananas` file before serving. |
| `--log-dir <DIR>` | none | Write a data log file in this directory (created if needed). |
| `--interpretive` | off | Add decoded descriptions to the log entries. Requires `--log-dir`. |
| `--wrong-ti` | off | Put transaction id `DE AD` in every reply (Options > Modbus > Return Incorrect TI). |
| `--wrong-protocol-id` | off | Put protocol id `AB CD` in normal replies (Return Incorrect Protocol ID). |
| `--wrong-length-high` | off | Put `EF` in the length high byte of normal replies (Return Incorrect Length High Byte). |
| `--out-of-synch` | off | Answer each request one request late (Reply Out of Synch). |
| `--address-offset` | off | Wire address n reaches register n + 1 (Use Address Offset). |

The server answers function codes 3, 4, 6, 7, 8, 16, 22 and 23 and listens on all interfaces (`0.0.0.0`). The function code set and forced exceptions can be changed in the GUI and through [MCP](mcp.md), not from the CLI.

```bash
# a server on a high port
ananasar server --port 15020

# preloaded registers and a decoded data log
ananasar server --port 15020 --load plant.ananas --log-dir logs --interpretive
```

It prints one line when it starts and one for each client that connects or disconnects:

```text
Ananasar server listening on 0.0.0.0:15020 (unit id 0, max 2 clients)
logging to logs/ananasar-15020-2026-10-15_09-30-00.log
client connected: 127.0.0.1:50422
```

The log file is named `ananasar-<port>-<UTC date and time>.log`. Each entry is numbered and shows the elapsed milliseconds, the connection (1 or 2), the query and the reply, as hex and, with `--interpretive`, decoded. If the port cannot be bound or the log or load file cannot be used, the command prints the reason to stderr and exits with status 1.

## ananasar client

Sends a fixed number of polls to a server and prints one line per reply. At least one of `--read-holding`, `--read-input` and `--write` is required (otherwise: "nothing to poll", exit status 2).

```text
ananasar client [OPTIONS] <TARGET>
```

| Option | Default | Meaning |
| --- | --- | --- |
| `<TARGET>` | required | Server as `host:port`. |
| `-u`, `--uid <UID>` | 1 | Unit id to address. |
| `--read-holding <ADDR,...>` | none | Holding registers to read with FC3, comma-separated. |
| `--read-input <ADDR,...>` | none | Input registers to read with FC4, comma-separated. |
| `--write <ADDR,...>` | none | Holding registers to write with FC16, comma-separated. |
| `-n`, `--count <N>` | 5 | Number of polls to send. |
| `--interval-ms <MS>` | 1000 | Delay between polls in milliseconds. |
| `--zero-ti` | off | Send transaction id 0 in every request instead of counting up. |

The address lists work like the client window's lists: they are sorted, and consecutive addresses are merged into one query (up to 125 for reads, 123 for writes). Each poll sends **one** query; the polls take the lists in turn (holding read, input read, write) and each list in runs, so `--read-holding 10,11,12,100` needs two polls to cover both runs.

```bash
ananasar client 127.0.0.1:15020 --read-holding 10,11,12,100 -n 4 --interval-ms 500
```

```text
connected to 127.0.0.1:15020
TI 0: FC 0x03 ok, 7 data bytes
TI 1: FC 0x03 ok, 3 data bytes
TI 2: FC 0x03 ok, 7 data bytes
TI 3: FC 0x03 ok, 3 data bytes
```

A successful reply is reported as its function code and the length of the reply data (the byte count byte plus the register bytes), not the register values; to see values, use [`ananasar scan`](#ananasar-scan) or the GUI. An exception reply is shown as `TI n: exception on FC 0x03, code 2`. Exit status is 0 after all polls, 1 if the connection, a send or a reply fails (the first failure ends the run), and 2 for no poll lists.

`--write` sends the value held in the command's own register file, which starts as 0 and is never changed, so `--write` writes zeros. To write a chosen value use the GUI client, or the MCP tools with `--allow-write`.

## ananasar scan

Reads a range one register at a time (FC3 or FC4 with a count of 1) and reports which addresses answer. The sample output below is illustrative.

```text
ananasar scan [OPTIONS] <TARGET>
```

| Option | Default | Meaning |
| --- | --- | --- |
| `<TARGET>` | required | Server as `host:port`. |
| `-u`, `--uid <UID>` | 1 | Unit id to address. |
| `-b`, `--bank <holding\|input>` | holding | Register bank to scan. |
| `--start <ADDR>` | 0 | First address. |
| `--end <ADDR>` | 99 | Last address, inclusive. |
| `--timeout-ms <MS>` | 1000 | Reply timeout per request. |
| `--only-ok` | off | Print only addresses that answered with a value. |

```bash
ananasar scan 192.168.1.50:502 --uid 3 --bank input --start 0 --end 20 --only-ok
```

```text
    0:  1234 (0x04D2)
    1:     0 (0x0000)
21 registers scanned, 2 succeeded, 19 failed, total duration 41.20ms
```

Without `--only-ok` an address that answered with an exception prints `exception N` and one that did not answer prints `no reply`. If a request gets no usable reply, the scan stops with a message, because the stream can no longer be matched to requests. The exit status is 0 if at least one register answered with a value and 1 otherwise, which makes `scan` usable as a reachability check in scripts.

## ananasar storm

(Called `ananasar stress` before; that name still works.)

Drives a target at a controlled load and records latency and errors, with optional ramps, fault injection and connection churn. It refuses heavy or risky settings unless you pass `--i-am-authorised`; use it only on equipment you are authorised to test. It is Modbus-level and does not replace link-level network storm tests. All options, examples and the report format are in [Storm testing](storm-testing.md); `ananasar storm --help` lists them.

```bash
ananasar storm 192.168.1.50:502 --out results
```

## ananasar completions

Prints a completion script to stdout for `bash`, `zsh`, `fish`, `powershell` or `elvish`.

```bash
ananasar completions bash > ~/.local/share/bash-completion/completions/ananasar
ananasar completions powershell | Out-String | Invoke-Expression
```

## ananasar about

Shows the version, the dependencies with their licences and the RustSec advisories known when the binary was built. `--deps`, `--advisories` and `--licenses` print one part; `--licenses` prints the full licence texts. A development build (not from a release) has no data embedded and says so. To check a binary against the current advisory database, run `cargo audit bin <path to ananasar>`. The licence texts, the dependency list and a CycloneDX SBOM are in the notices zip attached to every release; current advisories are code scanning alerts in the repository's Security tab (`audit.yml`).

## See also

- [Getting started](getting-started.md) for a first server and client session
- [Storm testing](storm-testing.md)
- [GUI manual](gui.md) for the windows behind the same engine
- [Troubleshooting](troubleshooting.md) for ports, unit ids and exceptions
