# Parity testing against the original Ananas

`ananasar-parity` checks at wire level that Ananasar answers like the original Ananas. It runs the same Modbus/TCP requests against two servers, records every frame and compares the recordings. GUI behaviour is not covered here; that stays a manual check.

The original `Ananas64.exe` is never part of the repository and never runs in CI. You run it yourself on Windows. What CI checks is a set of committed recordings of the original (the golden fixtures below), so no executable is needed there.

## The tool

```bash
cargo run -p ananasar-parity -- <command>
```

| Command | Purpose |
| --- | --- |
| `list [PROFILE]`, `list --profiles` | Print a profile's scenarios with one line each, or the profiles and the server setup each needs. |
| `run <host:port> [--profile P] [--only A,B] [--unit N] [--timeout-ms N] [--out FILE]` | Run a profile's scenarios against any server and record the exchange. |
| `cross [--addr host:port] [--profile P] [--strict-txn] [--record DIR]` | Run a profile against the original and against Ananasar and print one verdict per scenario. Skipped, and reported as skipped, when no address is given. |
| `ours [--profile P] [--out FILE]` | Start Ananasar's server in-process, set up like the profile, and record the profile's scenarios against it. |
| `diff <a.rec> <b.rec> [--strict-txn]` | Compare two recordings and print `same` or `DIFF` per scenario. Exit status 1 if any differ. |
| `proxy <listen> <target> --out FILE` | Sit between any client and any server and record the traffic. |

A recording is plain text: `> hex` for a frame sent, `< hex` for a reply, `< none` when the server stayed silent for the timeout and `< closed` when it closed the connection. `## name` starts a section. Recordings carry no timing. `diff` ignores transaction ids unless `--strict-txn` is given (the fault options change the id in a reply, so those need it; `cross` turns it on for `fault-ti` by itself).

## The scenario catalog

The scenarios are data, in `crates/ananasar-parity/src/scenarios/`, one file per area:

| File | Covers |
| --- | --- |
| `read.rs` | FC3 and FC4: 1, 10, 125 and 126 registers, quantity 0, both ends of the address space, short, empty and long PDUs. |
| `write.rs` | FC6 and FC16 (limits at 123 and 124, byte count faults, address edges, a write that ends exactly at register 65535 and one that runs one past it) and FC22 (the specification example, extreme masks, short and long PDUs). |
| `readwrite.rs` | FC23: write-before-read ordering, the 125/121 limits, zero counts, address edges, byte count faults. |
| `diagnostics.rs` | FC7, and FC8 with every sub-function from 0 to 0x14 plus unassigned ones. |
| `exceptions.rs` | Illegal function (unknown codes including the boundaries 0x7F, 0x80, 0xFF), illegal data address, illegal data value, and which wins when both apply. |
| `malformed.rs` | Unit ids 0, 1, 2, 247, 248 and 255; bad protocol ids; bad length fields; header-only and one- or five-byte input; an oversize PDU; pipelined requests; requests split across TCP segments (after the header, inside the length field, one byte at a time, mid-frame). |
| `faults.rs` | The same short probe set under every non-default profile (below), plus the address offset boundary reads. |

Every scenario has a name, a one-line description (`list` prints it) and a profile. Each scenario gets its own TCP connection; within a profile they share one server, so register state carries from one scenario to the next, in catalog order. A run is only comparable to another run that started from all-zero registers. Scenarios that may write to unpredictable places (a write PDU too short to hold its fields, a write past register 65535) are marked and run last.

Quantities of 253 or more registers are deliberately not in the catalog: the original does not bound them and they are unsafe to send to it.

### Profiles

A profile names the server setup its scenarios were written for. `list --profiles` shows them:

| Profile | Setup in the server |
| --- | --- |
| `default` | All function codes on, no fault option, no forced exception. |
| `disable-fc3` … `disable-fc23` | Options > Modbus > Active MB Functions: that one function code off, the rest on. |
| `exception-01` … `exception-0b` | Options > Modbus > Return Exception: that exception code. |
| `fault-ti`, `fault-protocol-id`, `fault-length-high` | Return Incorrect TI, Protocol ID, or Length High Byte. |
| `fault-out-of-synch` | Reply Out of Synch. |
| `address-offset` | Use Address Offset. |

Everything else stays at its default: unit id filter off (any unit id is answered), registers zero. Use `--unit N` for the unit id the scenarios send (default 1).

## Pointing it at the original

`Ananas64.exe` is a GUI program. It listens on port 502 and, if that is taken, moves to the next free port. If Ananasar's GUI is running, it holds 502 and the original ends up on 503 or higher; a second copy of the original goes one higher again. To find the port, read `Options > TCP > Port > Monitor port N` in the original, or ask Windows:

```powershell
Get-NetTCPConnection -State Listen | Where-Object { $_.OwningProcess -in (Get-Process Ananas64).Id } | Select-Object LocalPort, OwningProcess
```

You can also close the other programs and start the original first, or change its port with `Options > TCP > Port > Change Port`. Use a port you know and pass it as the address.

Before a run:

1. In the original, set the options the profile lists (Options > Modbus). Switching Active MB Functions back on pops up a short notice dialog; click OK, it blocks the window until you do.
2. Zero the registers: `Edit > Initialize > Input Registers > All to '0'` and `Holding Registers > All to '0'`.
3. Make sure nothing else talks to it (a client window in the original, another tool).

Then run the profile. With no address the cross-run is skipped and says so; nothing connects anywhere:

```bash
# skipped, prints "SKIPPED: no original server given ..."
cargo run -p ananasar-parity -- cross

# the address from an environment variable
ANANAS_ORIGINAL_ADDR=127.0.0.1:503 cargo run -p ananasar-parity -- cross --profile default

# or from a flag, also writing the original's frames to golden/default.rec
cargo run -p ananasar-parity -- cross --addr 127.0.0.1:503 --profile default --record crates/ananasar-parity/golden
```

`cross` prints one line per scenario, `same` or `DIFF` with both frames and a short decode, and exits 1 if any differ. For a profile with an option Ananasar lacks (`fault-*`, `address-offset`) it records and says there is nothing to compare against.

To keep two recordings and diff them later, `run` and `ours` write files and `diff` compares them:

```bash
cargo run -p ananasar-parity -- run 127.0.0.1:503 --profile default --out original.rec
cargo run -p ananasar-parity -- ours --profile default --out ananasar.rec
cargo run -p ananasar-parity -- diff original.rec ananasar.rec --strict-txn
```

`--only a,b` runs just the named scenarios, which helps to find which scenario changed the original's state.

A few things to know about the original while reading recordings:

- It keeps state between requests that a spec-conforming server would not: short or empty PDUs are answered from earlier bytes, so such a scenario's reply depends on the traffic before it. The catalog order is fixed for that reason; do not reorder scenarios without re-recording.
- Some FC8 sub-functions are counters, which also depend on earlier traffic.
- Using FC8 sub-function 2 or 10, or switching FC8 on, makes the original show a notice dialog. It does not stop the server answering, but it blocks the window until dismissed.

## Golden fixtures

`crates/ananasar-parity/golden/<profile>.rec` holds one recording of the original per profile: only frames as hex, the scenario names and notes. No binaries, no screenshots. `crates/ananasar-parity/tests/golden.rs` starts Ananasar's real server, set up like the profile, runs every scenario over loopback and compares byte for byte, transaction ids included. It fails on any difference that the file does not list.

Notes are comment lines that start with `#!`:

```
## write_multiple_over_limit
#! quirk: the original accepts 124 registers although 123 is the specified maximum
#! deviation: the original does not enforce the 125 read / 123 write register limits; Ananasar answers illegal data value ... (issue 94)
```

- `#! deviation: reason` inside a section: Ananasar differs here on purpose or is known to lack something. The test expects the section to differ and fails when it no longer does, so a fixed difference forces the note out.
- `#! quirk: text`: the original's reply looks like a bug or an oddity. Information only; it excuses nothing by itself.
- `#! default-deviation: reason` before the first section: any section may differ, for the stated reason. Used for the profiles whose option Ananasar does not implement yet.

Every deviation names the issue where it is tracked. The test also checks that the golden files and the profile list match one to one, that the sections match the catalog (change the catalog, re-record the golden), and that notes carry a real reason.

To record or refresh a golden, set the original up as above and run `cross --record` (or `run --out`), then add the notes for the sections that differ from `ours`. Only commit recordings taken from the running original. Do not commit recordings of Ananasar itself, since they only prove it agrees with itself.

The older `crates/ananasar-core/tests/golden/` files (replayed by `crates/ananasar-core/tests/golden.rs` against the engine alone, no sockets) stay as they are; `spec.rec` there holds examples from the Modbus specification.

### What the recordings found so far

The first full recording of all profiles found these differences. Each has an issue; the deliberate ones are collected in one.

- Fix made in this change: with Return Exception on, the original keeps illegal function for unsupported function codes and illegal data address for spans past register 65535; Ananasar forced the chosen code on everything.
- Return Incorrect TI, Protocol ID and Length High Byte, Reply Out of Synch and Use Address Offset are not implemented (issues 88, 89, 90).
- FC8 sub-functions and the FC7 status byte (issues 91, 92).
- The original closes the connection on a bad protocol id or length high byte (issue 93).
- Places where Ananasar follows the specification and the original does not: quantity limits, zero quantities, no PDU length checks, function codes above 0x7F, writes past register 65535, error precedence (issue 94).

## Cross-run: Ananasar server, original as the client

Here the original is the client, so use the proxy to record what it sends and what Ananasar answers.

1. Start the Ananasar server on a spare port and the proxy in front of it:

   ```bash
   cargo run -p ananasar-cli -- server --port 1502
   cargo run -p ananasar-parity -- proxy 127.0.0.1:1503 127.0.0.1:1502 --out original-client.rec
   ```

2. In the original, switch to client mode, point it at `127.0.0.1:1503`, poll for a while, then disconnect. A connection is written to the file when it closes.
3. Do the same with Ananasar's client (GUI, or `ananasar client 127.0.0.1:1503 --read-holding 0,10 -n 5`) against a new proxy file, then diff the two.

This mostly shows request differences: how the original merges addresses into queries, which transaction ids and unit ids it uses, how it behaves after an exception. Poll counts and timing vary between sessions, so use the same poll list and the same number of polls on both sides, or compare the first few entries by eye.

The proxy works for any pair, so it also records the original client against the original server, which is the reference for how the original itself behaves on the wire.

## Property tests

`crates/ananasar-core/tests/properties.rs` and `crates/ananasar-net/tests/properties.rs` use [proptest](https://docs.rs/proptest) (a dev-dependency, pinned to an exact version) to check the codec, the register file and the server engine against small reference models:

- frames round-trip through `build` and `frame` for any unit, function and payload, and arbitrary bytes never panic the frame parser, the engine or the stream reader;
- the stream reader returns a frame whole however the bytes are segmented, and two pipelined frames come back in order;
- registers written with FC6 or FC16 read back through FC3 as written, FC4 reads the input bank and no write touches it;
- FC22 matches the specification's formula for any current value and masks, and the extreme masks behave as stated;
- FC23 applies the write before the read, also when the ranges overlap;
- spans past the end give illegal data address, bad quantities illegal data value, disabled functions illegal function, a forced exception the chosen code (with the two exceptions found in the original).

## A neutral third client

To tell "the original is odd" from "Ananasar is odd", also ask each server the same thing with an independent client:

```bash
mbpoll -m tcp -a 1 -t 4 -r 1 -c 10 -1 127.0.0.1 -p 502
```

or with pymodbus:

```python
from pymodbus.client import ModbusTcpClient

c = ModbusTcpClient("127.0.0.1", port=502)
c.connect()
print(c.read_holding_registers(0, count=10, slave=1))
print(c.write_register(5, 0x1234, slave=1))
```

Point either at a `proxy` listen address to capture the frames. Library clients enforce the specification limits themselves, so they cannot send an over-limit request; use `ananasar-parity run` for those.

## The feature table

`docs/parity.toml` lists which behaviours of the original are shipped, partial or missing, with the test (or issue) behind each. `cargo test -p ananasar-core --test parity_table` fails when a shipped entry names a test that does not exist. Update the table in the same PR that adds or changes a feature.
