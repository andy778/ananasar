# Architecture

This is a short map of Ananasar for people and agents finding their way in: what each crate owns, what it may depend on, and the rules that must hold between them. The rules for contributing are in [AGENTS.md](AGENTS.md); the manuals are in [docs/](docs/README.md).

## Bird's-eye view

Ananasar is a Modbus/TCP client and server. One binary, `ananasar`, is the command line tool, the GUI (`ananasar gui`) and an MCP server (`ananasar mcp`).

At the centre is a register model: two banks (holding and input) of 65536 16-bit registers. A server answers Modbus requests from that model; a client polls a device and writes what it reads into the same model. Everything that touches the protocol goes through one I/O-free engine, `ananasar-core`, which turns request bytes into reply bytes. The other crates add sockets, threads, windows and files around it.

```
ananasar-cli (the `ananasar` binary) -> gui?, mcp?, storm, net, core
ananasar-gui    -> storm, net, core, dde (Windows only)
ananasar-mcp    -> net, core
ananasar-storm  -> net, core
ananasar-parity -> net, core
ananasar-dde    -> core
ananasar-net    -> core
fuzz            -> core
ananasar-core   -> std only

(? = optional cargo feature, on by default)
```

`cargo xtask layers` (also part of `cargo xtask ci` and of the `test (linux)` job in `build.yml`) enforces this graph: each crate may depend only on the `ananasar-*` crates listed for it, `ananasar-core` has no dependencies at all, and the headless crates (core, net, dde, storm, mcp, parity) must not pull in egui, eframe, winit or wgpu, even indirectly. Its table is `ALLOWED` in `xtask/src/layers.rs`, and a test keeps it the same as this graph, so adding a crate means updating both.

A request on the wire takes this path: `ananasar-net` reads one MBAP frame from the socket, `ananasar_core::server::process` validates it against the shared `RegisterFile` and returns an `Outcome` (reply bytes, drop, or close), and `ananasar-net` writes the reply and records the traffic for the log and status panel.

## Crates

Lower crates never depend on higher ones. Each entry says what the crate owns and what it may use.

### `crates/ananasar-core`

The protocol engine. Depends on `std` only.

- `protocol`: MBAP framing, function and exception codes.
- `registers`: the shared register model (`RegisterFile`, `Bank`).
- `server`: request validation and dispatch, including the quantity limits.
- `client`: the poll engine, the address-list optimizer and the query builders.
- `scanner`, `log`, `ananas_file`, `notices`, `sync`: register scanning, raw and interpretive log formatting, `*.ananas` text parsing, licence notices, and `sync::lock` (a mutex lock that survives a poisoned lock).

No sockets, threads, files or clocks: callers pass bytes and values in and get bytes and values out. That keeps it testable, fuzzable and portable.

### `crates/ananasar-net`

Blocking networking shared by the CLI, GUI, MCP server, storm and parity tools: the frame reader, the two-client server loop (`MAX_CLIENTS`, an idle timeout), client connect helpers and per-connection statistics. Threads over blocking `std::net` sockets, no async runtime. Depends on `ananasar-core` and `socket2`.

### `crates/ananasar-cli`

The `ananasar` binary: clap subcommands `server`, `client`, `scan`, `storm`, `completions` and `about`. With the default features `gui` and `mcp` it also embeds `ananasar-gui` and `ananasar-mcp`; `--no-default-features` gives the small headless CLI. Depends on everything below it. Its one `unsafe` module, `console`, detaches the Windows console when the GUI starts. Keep logic here thin: anything the GUI or MCP server could also need belongs in `core` or `net`.

### `crates/ananasar-gui`

The egui/eframe application behind `ananasar gui`: the register list, the client window, Set Register Value, the scanner, the storm window and the menus. It runs an embedded `ananasar-net` server on the same register file the client window reads and writes. Depends on `core`, `net`, `storm`, and on Windows `dde`.

### `crates/ananasar-mcp`

The Model Context Protocol server behind `ananasar mcp`: a small hand-written JSON-RPC 2.0 handler over stdio, plus tools for a Modbus client session and a simulated server. Read-only towards remote devices unless `--allow-write`. Depends on `core`, `net` and `serde_json`. See [docs/mcp.md](docs/mcp.md).

### `crates/ananasar-storm`

The library behind `ananasar storm` and the GUI's Storm Test window: parallel connections, rate ramps, fault injection, churn and latency reports, for equipment you are authorised to test. Depends on `core` and `net`. See [docs/storm-testing.md](docs/storm-testing.md).

### `crates/ananasar-dde`

The Windows DDE server that serves the shared register file to Excel and other DDE clients. Depends on `core`, and on `windows-sys` on Windows. `unsafe` is allowed only in its `server` module (DDEML is a C API); item parsing and formatting are safe and tested on every platform. See [its README](crates/ananasar-dde/README.md) and [docs/dde.md](docs/dde.md).

### `crates/ananasar-parity`

A developer tool, not shipped: a scenario runner, a recording proxy and a diff for comparing Modbus/TCP servers byte for byte, with committed recordings. Depends on `core` and `net`. See [docs/parity-testing.md](docs/parity-testing.md).

### `fuzz`

cargo-fuzz targets for the frame parser (`frame`) and the server handler (`server_process`). Depends on `ananasar-core` only. It needs nightly, so it is excluded from the workspace.

### `xtask`

Repository tasks behind the `cargo xtask` alias in `.cargo/config.toml`. `cargo xtask ci` runs the checks CI runs; `cargo xtask layers` runs only the dependency graph check. No dependencies; never shipped.

## Invariants

**Core stays I/O-free.** `ananasar-core` has no dependencies and does no I/O. Sockets live in `net`, threads in `net`, `storm`, `gui` and `mcp`, files and windows in the front ends. If a change to core needs a socket or a file, the boundary is in the wrong place.

**Network input is hostile.** Every byte from a socket is untrusted, and no frame may panic the server or overflow anything. The workspace lints deny `unwrap`, `expect`, `panic!`, `unreachable!`, slice indexing and unchecked arithmetic in non-test code, and forbid `unsafe` (`ananasar-dde` and `ananasar-cli` deny it and allow it in one module each, for Windows FFI). `net` caps a request at a fixed size before handing it to core. Shared state is locked with `sync::lock`, so a panic in one thread cannot poison the register file for the rest. The `frame` and `server_process` fuzz targets check the parsers. `ananasar-parity`, a developer tool that never runs as a server, still allows unchecked arithmetic and indexing (#59).

**The Modbus quantity limits are enforced.** `server::process` answers a read of more than 125 registers (`MAX_READ`) or a write of more than 123 (`MAX_WRITE`), a zero quantity, or a byte count that does not match, with an `IllegalDataValue` exception reply instead of building an oversized reply. This is a deliberate difference from the original, listed in [docs/differences.md](docs/differences.md).

**One register model.** The CLI server, the GUI and the MCP server's simulated server all hold a `core::registers::RegisterFile` behind `Arc<Mutex<_>>` and serve it through `net`'s server and `core::server::process`. In the GUI the embedded server, the client window and the DDE server share one file (server and client sharing one register file is how the original works too). No front end has its own copy of the protocol or register logic, so they behave the same on the wire.

**Toolchains.** The workspace's `rust-version` is 1.92. `ananasar-core`, `-net`, `-storm`, `-mcp` and the headless `ananasar-cli` (`--no-default-features`, with or without `mcp`) must build and test on 1.92; CI's `msrv` job checks this so the CLI and server can be built on older toolchains and servers without a desktop. `ananasar-gui` declares `rust-version = "1.95"` because egui and eframe require it, so the default `ananasar` binary, which embeds the GUI, needs 1.95.
