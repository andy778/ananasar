# Parity testing against the original Ananas

`ananasar-parity` checks at wire level that Ananasar answers like the original Ananas. It runs the same Modbus/TCP requests against two servers, records every frame and diffs the recordings. GUI behaviour is not covered here; that stays a manual check.

The original `Ananas64.exe` is never part of the repository. You run it yourself on Windows.

## The tool

```bash
cargo run -p ananasar-parity -- <command>
```

| Command | Purpose |
| --- | --- |
| `run <host:port> [--unit N] [--timeout-ms N] [--out FILE]` | Run the fixed scenarios against a server and record the exchange. |
| `proxy <listen> <target> --out FILE` | Sit between any client and any server and record the traffic. |
| `diff <a.rec> <b.rec>` | Compare two recordings. Exit status 1 if they differ. |
| `list` | Print the scenario names. |

The scenarios cover FC3 and FC4 reads, FC6, FC7, FC8, FC16, FC22 and FC23, the transaction limits (125 registers read, 123 written, and one over each), address edges at register 65535, zero counts, unknown function codes, unit ids 0, 1 and 255, a wrong protocol id, several bad length fields and pipelined requests. `ananasar-parity list` shows them all.

A recording is plain text: `> hex` for a frame sent, `< hex` for a reply, `< none` when the server stayed silent for the timeout and `< closed` when it closed the connection. `## name` starts a section. Recordings carry no timing, and `diff` ignores transaction ids, so only the bytes and the presence of a reply are compared.

## Before every run

The scenarios write to registers, so a run is only comparable to another run that started from the same state. Start each server fresh with all registers zero (no file loaded, no scrolling values) and run once per start. Set the server's unit id to accept the unit you pass with `--unit` (the default is 1; Ananasar's default of 0 accepts any).

Both servers must also have the same function codes enabled. Ananasar's default set is 3, 4, 6, 16, 22 and 23; match it in the original's options, or the difference you see is a configuration difference.

## Cross-run 1: original server, Ananasar as the client side

1. Start `Ananas64.exe`, set it to server mode with the options above and let it listen on port 502 (or any port).
2. Record the scenarios against it:

   ```bash
   cargo run -p ananasar-parity -- run 127.0.0.1:502 --out original.rec
   ```

3. Restart Ananasar's server with fresh registers and record the same run:

   ```bash
   cargo run -p ananasar-cli -- server --port 1502
   cargo run -p ananasar-parity -- run 127.0.0.1:1502 --out ananasar.rec
   ```

4. Diff them:

   ```bash
   cargo run -p ananasar-parity -- diff original.rec ananasar.rec
   ```

Each difference is printed with the section, the entry number and both lines, with a short decode (unit, function, exception code).

## Cross-run 2: Ananasar server, original as the client

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

## Keeping recordings as golden fixtures

`crates/ananasar-core/tests/golden.rs` replays every `*.rec` file in `crates/ananasar-core/tests/golden/` against the engine and fails if a reply differs. CI therefore checks the recordings without the executable.

- `spec.rec` holds worked examples and exception rules from the Modbus specification.
- To add what the original really answers, run `run` against it as above and save the file there as `ananas-<what>.rec`. Replay assumes the file's first request meets all-zero registers and a default Ananasar configuration (unit id 0, FC 3, 4, 6, 16, 22, 23), and that the requests in the file are one session in order.
- Only commit recordings taken from the running original or from the specification. Do not commit recordings of Ananasar itself, since they only prove it agrees with itself.
- A recording that shows a real difference fails the test. Either fix Ananasar, or remove the section and note the deviation in the code where Ananasar differs on purpose (for example the 125/123 quantity limits).

Keep the section for a scenario only when it is useful as a regression test: trim the file to the sessions you want checked.

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
