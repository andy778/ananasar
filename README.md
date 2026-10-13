# Ananasar (Rust port of Ananas)

[![Build](https://img.shields.io/github/actions/workflow/status/andy778/ananasar/build.yml?branch=main&logo=github&cacheSeconds=3600)](https://github.com/andy778/ananasar/actions/workflows/build.yml)
[![GitHub all releases](https://img.shields.io/github/downloads/andy778/ananasar/total?logo=github&cacheSeconds=3600)](https://github.com/andy778/ananasar/releases)
[![GitHub release (latest by date)](https://img.shields.io/github/v/release/andy778/ananasar?logo=github&cacheSeconds=3600)](https://github.com/andy778/ananasar/releases/latest)
[![License](https://img.shields.io/github/license/andy778/ananasar)](LICENSE)

A Rust reimplementation of **Ananas**, the Modbus/TCP client–server tool
originally shipped as `ananas64.exe` (Borland Delphi 2006). The protocol engine
is reverse-engineered from that binary; this repo is the clean-room-ish rewrite.

## Layout

- `crates/ananasar-core` — `std`-only, platform-agnostic protocol engine:
  - `protocol` — MBAP framing, function & exception codes.
  - `registers` — the shared register model (two 65536×u16 banks, holding + input).
  - `server` — request validation + dispatch (FC 3/4/6/16/22/23, +7/8), from `FUN_00686ef0`.
  - `client` — the poll engine: sorted address lists, the consecutive-run
    optimizer, round-robin scheduler and query builders, from `FUN_006647a0`,
    `FUN_00665640`, `FUN_006652b0`, `FUN_00665150`.
- `crates/ananasar-cli` — headless `ananas` binary (server + client over TCP).

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
