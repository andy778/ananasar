<p align="center">
  <img src="assets/logo.svg" width="320" alt="ananasar — two pineapples">
</p>

# Ananasar (Rust port of Ananas)

[![Build](https://github.com/andy778/ananasar/actions/workflows/build.yml/badge.svg)](https://github.com/andy778/ananasar/actions/workflows/build.yml)
[![GitHub all releases](https://img.shields.io/github/downloads/andy778/ananasar/total?logo=github&cacheSeconds=3600)](https://github.com/andy778/ananasar/releases)
[![GitHub release (latest by date)](https://img.shields.io/github/v/release/andy778/ananasar?logo=github&cacheSeconds=3600)](https://github.com/andy778/ananasar/releases/latest)
[![License](https://img.shields.io/github/license/andy778/ananasar)](LICENSE)

A Rust reimplementation of **Ananas**, a Modbus/TCP client–server test tool.

## Layout

- `crates/ananasar-core` — `std`-only, platform-agnostic protocol engine:
  - `protocol` — MBAP framing, function & exception codes.
  - `registers` — the shared register model (two 65536×u16 banks, holding + input).
  - `server` — request validation + dispatch (FC 3/4/6/16/22/23, +7/8).
  - `client` — the poll engine: sorted address lists, the consecutive-run
    optimizer, round-robin scheduler and query builders.
- `crates/ananasar-cli` — headless `ananasar` binary (server + client over TCP).

## Build & test

```bash
cargo build --release      # native (Linux) binary at target/release/ananasar
cargo test                 # protocol/optimizer unit tests
```

### Windows binary

The Arch system `rustc` has no cross std. Use rustup:

```bash
rustup target add x86_64-pc-windows-gnu
sudo pacman -S mingw-w64-gcc
cargo build --release --target x86_64-pc-windows-gnu
```

### Prebuilt binaries

Each tagged release has, built by CI:

- **Windows x64** — `ananasar.exe`, `ananasar-gui.exe`
- **macOS universal** (arm64 + x86_64) — `ananasar-macos-universal.tar.gz`
- **Linux x86_64** — `ananasar-linux-x86_64.tar.gz`

The macOS binaries are **unsigned**, so Gatekeeper blocks them on first run.
After extracting, clear the quarantine flag:

```bash
tar xzf ananasar-macos-universal.tar.gz
xattr -dr com.apple.quarantine ananasar ananasar-gui
./ananasar-gui
```

## Run (headless)

```bash
# server (port 502 needs root; use a high port otherwise)
ananas server --port 15020 --uid 0

# client: poll holding registers, write a block, etc.
ananas client 127.0.0.1:15020 --read-holding 10,11,12,100 --count 4
ananas client 127.0.0.1:15020 --write 42,43,44 --count 1
```

## Fidelity notes

- Server and client share one `RegisterFile`, as the original does
  ("server and client are sharing registers", manual §2.2).
- The optimizer merges only addresses that are already consecutive in the
  sorted list, capped by the max-per-query setting — matching the original's
  greedy coalescing loop.
- **Intentional fix:** the original validated only `start + count <= 65536` and
  built FC3/FC4 replies in a fixed 513-byte global, so quantities of 253–255
  overflowed it. This port enforces the Modbus limits (125 read / 123 write) and
  returns `IllegalDataValue` instead.

## Not yet ported

The GUI (Delphi VCL forms), the `*.ananas` save/load format, raw/interpretive
log files, the register scanner, and the Windows-only DDE / adapter-IP features.
