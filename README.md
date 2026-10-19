<div align="center">

<img src="assets/logo.svg" width="120" alt="Ananasar logo: two pineapples">

# Ananasar

**A Modbus/TCP client and server, written in Rust.**<br>
A port of Ananas, the free Modbus/TCP test tool from http://www.tuomio.fi/ananas/, built with [REA](https://rea.tools/) as a hands-on look at what AI-assisted development can do today.

[![Build](https://github.com/andy778/ananasar/actions/workflows/build.yml/badge.svg)](https://github.com/andy778/ananasar/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/andy778/ananasar?logo=github&cacheSeconds=300)](https://github.com/andy778/ananasar/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/andy778/ananasar/total?logo=github&cacheSeconds=300)](https://github.com/andy778/ananasar/releases)
[![License](https://img.shields.io/github/license/andy778/ananasar?cacheSeconds=300)](LICENSE)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/andy778/ananasar/badge)](https://securityscorecards.dev/viewer/?uri=github.com/andy778/ananasar)

[Download](#download) · [Quick start](#quick-start) · [Documentation](#documentation) · [Build](#build-from-source)

<img src="assets/screenshot.png" width="820" alt="Ananasar: the register list, the Server status panel with a client polling, and the logging controls">

<sub>The register list, the live Server status panel with a client polling, and the Logging controls.</sub>

</div>

## At a glance

| Area | Feature | What it does | Read more |
| --- | --- | --- | --- |
| Server | Function codes | Answers FC 3, 4, 6, 7, 8, 16, 22 and 23; each can be switched off. Proper exception replies, unit-id filtering, two clients at a time. | [GUI manual](docs/gui.md#modbus-options) |
| Server | Fault injection | Answer every request with a chosen exception code (1-8, 10, 11). | [GUI manual](docs/gui.md#modbus-options) |
| Server | Status panel | Live per-client IP, poll interval, last request fields, traffic totals and error flags. | [GUI manual](docs/gui.md#server-panel) |
| Client | Poll engine | Sorted address lists, consecutive addresses merged into few queries, Cycle / Wait / Immediate modes. | [GUI manual](docs/gui.md#client-window) |
| Client | Manual writes | FC6, FC16 and FC22 (mask write) from a dialog, plus FC7 exception status. | [GUI manual](docs/gui.md#manual-commands) |
| Tools | Register scanner | Reads a range one register at a time and reports which answer (GUI and `ananasar scan`). | [GUI](docs/gui.md#register-scanner), [CLI](docs/cli.md#ananasar-scan) |
| Tools | Data log | Every request and reply per connection as hex, optionally with decoded descriptions. | [GUI](docs/gui.md#data-log), [CLI](docs/cli.md#ananasar-server) |
| Tools | Storm test | `ananasar storm` and Tools > Storm Test… in the GUI: parallel connections, rate ramps, fault injection, connection churn, latency percentiles and a JSON/CSV report, for equipment you are authorised to test. Modbus-level, not a link-level storm generator. | [Storm testing](docs/storm-testing.md), [GUI](docs/gui.md#storm-test) |
| Tools | Register editing | Hex or decimal list, inline editing, bit editor, 32-bit Uint/Float views with four byte orders. | [GUI manual](docs/gui.md#set-register-value) |
| Tools | Save / load | Register values as `*.ananas` text files. | [GUI manual](docs/gui.md#file-menu) |
| Integrations | CLI | Headless `server`, `client`, `scan` and shell completions. | [CLI manual](docs/cli.md) |
| Integrations | MCP server | Lets an AI assistant read devices and run a simulated server; read-only by default. | [MCP manual](docs/mcp.md) |
| Integrations | DDE server (Windows) | Serves registers to Excel and other DDE clients, with hot links. | [DDE manual](docs/dde.md) |
| Quality | Fuzzing | The parsers that read network input are fuzzed. | [Build](#build-from-source) |
| Quality | Parity testing | Wire-level comparison with the original's replies. | [Parity testing](docs/parity-testing.md) |
| Quality | Signed releases | Release files are signed with cosign; binaries embed their dependency list (cargo-auditable). | [Verify a download](#verify-a-download) |

## Why Ananasar

- **One binary, one engine.** `ananasar` is the GUI, the CLI and the MCP server in a single file; they share the same register model and protocol code, so they behave the same way.
- **Runs where you work.** The original is a Windows program whose download site no longer exists. Ananasar builds for Windows, macOS and Linux and is MIT licensed.
- **Familiar, with the sharp edges filed off.** The menus, dialogs and Server panel follow the original's, while the server enforces the Modbus size limits instead of overflowing a buffer (see [design notes](#design-notes)).
- **Checkable.** Releases are signed, parsers are fuzzed and the replies are compared with the original's at wire level. It is still a hobby project, provided as is with no support commitment ([SECURITY.md](SECURITY.md)).

## Download

Prebuilt binaries are attached to every [release](https://github.com/andy778/ananasar/releases/latest):

| Platform | Files |
| --- | --- |
| Windows x64 | `ananasar.exe` |
| macOS (Apple silicon + Intel) | `ananasar-macos-universal.tar.gz` |
| Linux x86_64 | `ananasar-linux-x86_64.tar.gz` |

One program does everything: `ananasar` is the command line tool, `ananasar gui` opens the window and `ananasar mcp` runs the MCP server. On Windows, double-clicking `ananasar.exe` opens the GUI.

The macOS build is unsigned, so Gatekeeper blocks it on first run. Clear the quarantine flag after extracting:

```bash
tar xzf ananasar-macos-universal.tar.gz
xattr -dr com.apple.quarantine ananasar
./ananasar gui
```

The Windows executables are not code-signed either, so Windows SmartScreen may warn the first time you run them ("More info" → "Run anyway").

### Verify a download

Each release has a `SHA256SUMS` file covering every file you download, signed with [cosign](https://github.com/sigstore/cosign) (keyless, from the release workflow); the signature is `SHA256SUMS.sigstore.json`. Download the files you want together with those two, then check the signature and the checksums:

```bash
cosign verify-blob \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github.com/andy778/ananasar/\.github/workflows/build\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

On macOS use `shasum -a 256 -c SHA256SUMS` (it reports files you did not download as missing; ignore those).

The binaries are built with [cargo-auditable](https://github.com/rust-secure-code/cargo-auditable), so their dependency list is embedded and can be checked for known vulnerabilities with `cargo audit bin ananasar`.

*Signatures and embedded audit data start with the first release made after they were added; older releases (v0.1.x) have neither.*

## Quick start

**GUI.** Double-click `ananasar.exe` on Windows, or run `ananasar gui`. It starts a server on port 502 on Windows (15020 on other platforms, where ports below 1024 need privileges). If that port is taken, it moves to the next free port and shows a notice. Press **Show** in the Client box to open the client window and poll the server, or point the client at any other Modbus/TCP device.

**CLI.** Two terminals:

```bash
# terminal 1: a server on a high port
ananasar server --port 15020

# terminal 2: scan which registers answer, then poll some of them
ananasar scan 127.0.0.1:15020 --start 0 --end 9
ananasar client 127.0.0.1:15020 --read-holding 0,1,2 -n 3
```

More in [Getting started](docs/getting-started.md), the [GUI manual](docs/gui.md) and the [CLI manual](docs/cli.md).

## Use it from an AI assistant (MCP)

`ananasar mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio. It gives an assistant tools to read Modbus/TCP devices, run a simulated server on your machine and inspect its traffic. It is **read-only by default**: writes to remote devices need `--allow-write`.

```bash
claude mcp add ananasar -- ananasar mcp
```

Then ask, for example: *"Start a simulated Modbus server on port 15021, set holding registers 100-103 to 1, 2, 3, 4, then read them back through the client."* Setup for Claude Desktop, the full tool list and the safety model are in the [MCP manual](docs/mcp.md).

## Documentation

| Page | Contents |
| --- | --- |
| [Getting started](docs/getting-started.md) | Install, first run, default ports, a first client/server session |
| [GUI manual](docs/gui.md) | Every window, menu and option of `ananasar gui` |
| [CLI manual](docs/cli.md) | `ananasar server`, `client`, `scan`, `storm` and `completions` |
| [Storm testing](docs/storm-testing.md) | Load and robustness checks with `ananasar storm` and the Storm Test window |
| [MCP manual](docs/mcp.md) | Using Ananasar from an AI assistant |
| [DDE manual](docs/dde.md) | Serving registers to Excel and other DDE clients (Windows) |
| [Troubleshooting](docs/troubleshooting.md) | Ports, firewalls, no reply, exceptions, reporting a bug |
| [Parity testing](docs/parity-testing.md) | Comparing Ananasar with the original at wire level |
| [Differences](docs/differences.md) | Where Ananasar deliberately differs from the original, and why |
| [Releasing](docs/releasing.md) | How releases are made |

The full index is [docs/README.md](docs/README.md).

## Build from source

Needs Rust 1.92 for the core and CLI, and 1.95 for the GUI (egui 0.36).

```bash
cargo build --release      # binaries in target/release/
cargo test                 # protocol and optimizer tests
```

### Debian / Ubuntu

Debian's packaged Rust is older than 1.92, so install the toolchain with [rustup](https://rustup.rs) instead of `apt`.

```bash
sudo apt install build-essential pkg-config git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"

git clone https://github.com/andy778/ananasar.git && cd ananasar
cargo build --release
```

The CLI and server (`target/release/ananasar`) run headless and need nothing more. The GUI (`target/release/ananasar-gui`) needs a desktop session (X11 or Wayland). The windowing libraries are loaded at run time, so a minimal install may need them:

```bash
sudo apt install libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcursor1 libxi6 libxrandr2 libgl1 libegl1
```

To skip building, download the `ananasar-linux-x86_64.tar.gz` release; it is built on Ubuntu 24.04, so it needs a comparably recent glibc.

The parsers that read network input are also fuzzed (`cargo +nightly fuzz run frame` from the repository root, using [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)). CI builds and releases the Windows, macOS and Linux binaries.

The README screenshot is rendered headlessly from the real window; see [docs/screenshots.md](docs/screenshots.md) to regenerate it.

## Project layout

- [`crates/ananasar-core`](crates/ananasar-core): `std`-only, I/O-free protocol engine
  - `protocol`: MBAP framing, function and exception codes
  - `registers`: the shared register model
  - `server`: request validation and dispatch
  - `log`: raw and interpretive log entry formatting
  - `client`: the poll engine and query builders
- [`crates/ananasar-cli`](crates/ananasar-cli): the `ananasar` binary: the command line tool, plus `ananasar gui` and `ananasar mcp` (cargo features `gui` and `mcp`, on by default; `--no-default-features` builds the small headless CLI)
- [`crates/ananasar-storm`](crates/ananasar-storm): the library behind `ananasar storm`, see [docs/storm-testing.md](docs/storm-testing.md)
- [`crates/ananasar-mcp`](crates/ananasar-mcp): the MCP server library behind `ananasar mcp`, see [docs/mcp.md](docs/mcp.md)
- [`crates/ananasar-dde`](crates/ananasar-dde): the Windows DDE server used by the GUI, see [docs/dde.md](docs/dde.md)
- [`crates/ananasar-gui`](crates/ananasar-gui): the egui application library behind `ananasar gui`
- [`crates/ananasar-parity`](crates/ananasar-parity): wire-level scenario runner, recording proxy and diff for comparing servers, see [docs/parity-testing.md](docs/parity-testing.md)
- [`fuzz`](fuzz): cargo-fuzz targets for the frame parser and the server handler

## Design notes

- The server and client share one register file, as the original does.
- The optimizer only merges addresses that are already consecutive in the sorted list, capped by the max-per-query setting, matching the original's greedy coalescing.
- **Intentional fix:** the original validated only `start + count <= 65536` and built FC3/FC4 replies in a fixed 513-byte buffer, so quantities of 253–255 overflowed it. This port enforces the Modbus limits (125 read / 123 write) and returns `IllegalDataValue` instead.

## Known gaps

What is not done or differs from the original, stated plainly. Greyed-out entries in the Options menu are listed in the [GUI manual](docs/gui.md#not-implemented).

| Area | Gap | Issue |
| --- | --- | --- |
| Server | The connection is closed on a bad protocol id or length high byte, as in the original. A length field that disagrees with the bytes sent is framed by the length field; the original answers from whatever bytes arrived. | [#93](https://github.com/andy778/ananasar/issues/93) |
| Server options | Purge Port Usage Blocking(s) is greyed out on purpose: Ananasar finds a free port by binding and keeps no record of used ports, so nothing can go stale. | [#105](https://github.com/andy778/ananasar/issues/105) |
| CLI | `ananasar client --write` writes zeros only, and there is no option for the address offset, the function code set or a forced exception. | none |
| Alarm data | Alarm data with timestamps over Modbus is under investigation only. | [#32](https://github.com/andy778/ananasar/issues/32) |
| Original's network-adapter features | Not planned. | none |

Places where Ananasar deliberately differs from the original (for example, it enforces the Modbus quantity limits and rejects FC8 requests too short to hold a sub-function) are listed in [Differences from the original](docs/differences.md) ([#94](https://github.com/andy778/ananasar/issues/94)).

## About the original

Ananas is a free Modbus/TCP test tool taken from `http://www.tuomio.fi/ananas/`. Ananasar is an independent reimplementation: it contains no code from the original, and the original program is not included here.

## License

[MIT](LICENSE). Ananasar is provided as is; see [SECURITY.md](SECURITY.md).

### Third-party software

The binaries include open-source libraries (mostly MIT and Apache-2.0, plus the BSD, ISC, Zlib and Unicode licences and the licences of the fonts bundled with egui). Every release attaches `ananasar-<version>-notices.zip`, which contains:

- `licenses.txt`: the licence texts of all dependencies,
- `dependencies.txt`: each dependency with its version and licence,
- `advisories.txt`: the RustSec advisories known when the release was built,
- `ananasar.cdx.json`: a CycloneDX software bill of materials (the GUI and MCP server are part of the one `ananasar` package),

and its checksum is in the signed `SHA256SUMS`. The RustSec advisories for the dependencies are not a release file: CI publishes them as code scanning alerts (Security tab) on every push to `main` and daily. GitHub adds the source code archives of the tag automatically. `ananasar about` prints the dependency list and the advisories embedded in the binary, and `ananasar about --licenses` the licence texts. The binaries also carry `cargo auditable` data, so `cargo audit bin <file>` checks them against the current advisory database. CI rejects a dependency whose licence is not on the allow-list in `deny.toml`.
