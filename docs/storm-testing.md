# Modbus/TCP storm testing (`ananasar storm`)

`ananasar storm` drives a Modbus/TCP target at a controlled load and records how it behaves: how fast it answers, whether it answers correctly, and what happens when the traffic gets rough. It is meant for **internal checks** of Modbus devices and of your own server/poller setups, for example "does this gateway still answer within 50 ms with eight pollers attached", "what happens to the PLC when a client sends garbage" or "does the poller recover when the server restarts".

## Safety first

**Use it only against equipment you own or are authorised to test.** Storm traffic can slow, confuse, reset or crash a device, and some devices need a power cycle afterwards. Do not point it at a running plant, a vessel's live control system or anyone else's network without the owner's written agreement.

The tool enforces some caution:

- It prints a notice on every run.
- The defaults are modest: 4 connections, 10 requests/s each (40/s in total), 10 seconds, reads only.
- These need the explicit flag `--i-am-authorised`, and the command refuses with a message without it:
  - a total rate above 1000 requests/s (an unlimited rate, `--rate 0`, counts; for a ramp the peak counts),
  - more than 64 connections (idle `--half-open` connections count),
  - any fault injection,
  - churn without a pause (`--churn-interval-ms 0`) or above 1000 connects/s.
- Write function codes (6, 16, 22, 23) are off unless you give them a weight in `--mix`. They change register values on the target.

## What it is not

This is a **Modbus-level** tool. It does not replace the **link-level network storm tests** (broadcast, multicast, unicast floods) used in marine classification, for example DNV control system network robustness testing. Those flood the Ethernet layer and need a raw packet generator such as [Ostinato](https://ostinato.org) or a commercial marine test tool. Run one of those for the classification evidence.

You can run `ananasar storm` *while* such a flood is running to see how the Modbus traffic behaves during it. The per-second timeline file (below) carries wall-clock timestamps so it can be lined up with the flood's start and stop.

The reports are plain measurements for an internal record. They are not a certified test report.

## Usage

```text
ananasar storm <HOST:PORT> [options]
```

```bash
# Default modest run, with the report written to ./results
ananasar storm 192.168.1.50:502 --out results

# 8 connections at 50 requests/s each for a minute (400/s in total)
ananasar storm plc.local:502 -c 8 -r 50 -d 60 --out results

# Ramp each of 4 connections from 5 to 400 requests/s over 30 s, then hold for 10 s
ananasar storm 10.0.0.5:502 --rate 5 --ramp-to 400 --ramp-secs 30 -d 40 --i-am-authorised

# Mix of reads and writes on addresses 0-199, up to 20 registers per request
ananasar storm 10.0.0.5:502 --mix 3=60,4=20,16=20 --addr-end 199 --qty-max 20

# Fault injection: 2 % wrong length fields, 5 % requests split across two segments
ananasar storm 10.0.0.5:502 --fault-bad-length 2 --fault-split 5 --i-am-authorised

# Connection churn (connect, disconnect, repeat every 50 ms per thread) with 10 idle connections held open
ananasar storm 10.0.0.5:502 --churn --churn-interval-ms 50 --half-open 10
```

Run `ananasar storm --help` for every option.

### Load

| Option | Meaning |
| --- | --- |
| `-c, --connections N` | Parallel connections, each on its own thread (default 4). |
| `-r, --rate R` | Requests per second **per connection** (default 10). `0` sends the next request as soon as the reply arrives. |
| `--ramp-to R2` `--ramp-secs S` | Change the rate linearly from `--rate` to `R2` over `S` seconds (default: the whole run), then hold `R2`. |
| `-d, --duration-secs S` | Run time (default 10, 1 s to 24 h). |
| `--timeout-ms N` | Reply timeout (default 1000). |
| `-u, --uid N` | Unit id in requests (default 1). |

Each connection has one request in flight. If the target answers slower than the pacing interval, the achieved rate falls below the target; the report shows the achieved rate. The pacer does not "catch up" with a burst.

### Request mix

`--mix FC=WEIGHT,...` chooses among function codes 3, 4, 6, 16, 22 and 23 by weight (default `3=70,4=30`; unlisted functions get 0). `--addr-start`, `--addr-end`, `--qty-min` and `--qty-max` bound the addresses and quantities; quantities are limited further per function by the protocol (125 for reads, 123 for FC16, 121 for the write part of FC23) and by the size of the address range.

`--seed N` makes the request sequence, the write data and the fault rolls repeatable. Without it a seed is taken from the clock; it is printed and written to the report. The same seed gives the same sequence per connection; timing (and so the exact number of requests) still varies from run to run.

### Fault injection

Each is a percentage of requests (their sum must not exceed 100) and needs `--i-am-authorised`.

| Option | What is sent |
| --- | --- |
| `--fault-short-header PCT` | 1-6 bytes, fewer than an MBAP header, then nothing. |
| `--fault-bad-protocol PCT` | A frame with a non-zero protocol id. |
| `--fault-bad-length PCT` | A frame whose length field disagrees with the bytes sent. |
| `--fault-truncated-pdu PCT` | A correct length field, but fewer bytes than it promises. |
| `--fault-oversized-pdu PCT` | A PDU of 254-400 bytes, above the 253-byte limit. |
| `--fault-coalesced PCT` | `--coalesce-count` (default 3) valid requests in one segment. Replies are checked in order. |
| `--fault-split PCT` | One valid request written as two segments, `--split-delay-ms` (default 50) apart. The latency includes that delay. |

After a malformed frame the tool waits up to `--fault-wait-ms` (default 200) and records whether the target **answered**, **closed the connection** or **ignored** it, then reconnects, because the byte stream can no longer be trusted. Coalesced and split requests are valid requests, so they are counted and validated like normal ones.

### Connection churn

`--churn` replaces the request stream: each connection thread connects, optionally sends one request (`--churn-request`), disconnects and starts again, every `--churn-interval-ms` (default 100; 0 = as fast as possible). The report counts cycles, failed connects and connect latency. `--half-open N` also holds N extra connections open and idle for the whole run (in churn or normal mode). These are ordinary open sockets that never send anything, not raw half-open TCP handshakes.

## Output

While running, one line per second:

```text
[   3s] sent     20  replies     20  exc    0  timeout    0  err    0  mismatch   0  fault    0  cycles     0  mean 0.11 ms
```

At the end a summary is printed. With `--out DIR` three files are written, named by the UTC start time (`ananasar-storm-2026-10-15_19-49-05.*`; a repeat in the same second gets `-2`, `-3`, ...):

- `.json`: all parameters (including the version, the seed and a `reproduce` command line), all results and the timeline.
- `.csv`: the same parameters and results as `section,key,value` rows.
- `.timeline.csv`: one row per second with Unix-millisecond timestamps, for lining up with an external flood or event log.

### What is measured

- **Latency** from writing the request to receiving the complete reply, for every well-formed reply (normal or exception): min, mean, p50, p95, p99, max. The mean, min and max are exact; the percentiles come from a histogram and are within about 3 %.
- **ok** replies and Modbus **exceptions by code**.
- **timeouts**, **resets** (connection closed or reset), other I/O errors and **connect errors**, plus **reconnects**.
- **mismatches**, counted separately from errors: a reply that does not match its request. It checks the protocol id, the transaction id, the unit id and the function code (or its exception form), that a read reply's byte count and data length fit the quantity asked for, and that a write reply echoes the request. The breakdown names the failed check.
- With faults, the count of each injected fault and the target's reaction to malformed frames.

Every request counts exactly once: `requests_sent` equals ok + exceptions + mismatches + timeouts + resets + I/O errors (`requests_accounted` in the report). Injected malformed frames are not requests and are counted under faults.

Against `ananasar server` the run should have no mismatches. Note that `ananasar server` itself accepts only two clients at a time (as the original did), so use at most `-c 2` against it.

## The GUI

Tools > Storm Test… in `ananasar gui` opens the same tool in a window, built on the same library, so the load, the request mix, the faults, the churn and the report files are the ones described above. See [Storm Test](gui.md#storm-test) in the GUI manual. It is an Ananasar addition; the original program has no storm test.

- The window shows the same notice and the same modest defaults, and the target field starts empty.
- The authorisation box, "I am authorised to test this equipment", is required for exactly what `--i-am-authorised` is required for (see Safety first), and the library checks the rules again before anything is sent. The box is cleared when a test starts.
- Start and Stop control the test, which runs on worker threads. Stop takes effect within one reply timeout.
- Progress is shown live: counters, latency mean, median, 95th and 99th percentile and maximum, and a per-second chart. The percentiles come from the same histogram as the report.
- **Save report** writes the `.json`, `.csv` and `.timeline.csv` files, with the same names, into a folder you choose. The timeline has the same wall-clock timestamps for lining up with an external flood.

## Limits

- On the command line, Ctrl-C ends the process without a report; use `--duration-secs` to bound a run. In the window, Stop keeps the partial results.
- One request is in flight per connection (no pipelining apart from the coalesced fault).
- The tool does not analyse the target's internals; correlate with the device's own logs if you need to know why it misbehaved.
