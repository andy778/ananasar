# Troubleshooting

Common problems with ports, firewalls, silent devices and Modbus exceptions, and what to include when you report a bug.

## Port problems

**Port 502 needs administrator rights on Linux and macOS.** Ports below 1024 are privileged there. Use a higher port (`ananasar server --port 15020`; the GUI already defaults to 15020 on those systems), or run as root if you must serve on 502. A real device that expects port 502 may need a port redirect on the machine in front of Ananasar.

**The port is already in use.** Another program, or a second Ananasar, owns it.

- `ananasar-gui` without `--port` moves to the next free port (up to 20 in a row) and shows "Port changed to N. Default port D is currently used by other instance of Ananas." The port in use is the *Local port* row of the Server panel and in the Options > TCP > Port menu entry.
- `ananasar-gui --port N` and `ananasar server` do not fall back. The GUI shows "Cannot listen on port N: ..."; the CLI prints "Socket initialisation failed on port N" and exits with status 1. Pick another port or stop the other program.
- If the port was just freed it may take a moment before it can be bound again.

**The GUI shows a different port than I expected.** Check the notice at startup, or look at *Local port*. Use Options > TCP > Port > Change Port… to move it.

## Firewalls and security warnings

- **Windows Firewall** asks whether to allow the program on first start, because the server listens on all network interfaces. Allow it on the networks where clients will connect, or leave it blocked if you only test on the same machine.
- **A client on another machine cannot connect.** Check the firewall of the machine running the server, that the server listens (Options > TCP > Port > Monitor Port is checked), and that the client uses the machine's address, not `127.0.0.1`.
- **Windows SmartScreen** warns about the executables because they are not code-signed. Choose "More info", then "Run anyway". You can [verify the release signature](../README.md#verify-a-download) first.
- **macOS Gatekeeper** blocks the unsigned binaries. Run `xattr -dr com.apple.quarantine ananasar ananasar-mcp ananasar-gui` in the extracted folder; see [Getting started](getting-started.md#install).

Modbus/TCP has no authentication or encryption. Do not expose the server to untrusted networks ([SECURITY.md](../SECURITY.md)).

## No reply from a device

Work through these in order:

1. **Can you connect at all?** `ananasar scan host:502 --start 0 --end 0 --timeout-ms 3000` fails at once with "connect ... failed" if nothing listens there.
2. **Unit id.** Many devices and gateways answer only their own unit id; some answer only unit 0 or 255. The client's UID defaults to 1 (`--uid` in the CLI, the UID box in the client window, `unit_id` in MCP). A device that ignores a request with the wrong unit id simply does not reply. Ananasar's own server does the same when its UID is not 0.
3. **Address offset.** Device documentation often counts registers from 1 ("40001" style) while the wire address is 0-based. If register 100 in the manual does not answer, try address 99. The client window has **Use address offset** (subtracts one on the wire), as does the Register Scanner; the CLI and MCP use wire addresses directly, so subtract one yourself.
4. **Register bank and function.** Holding registers are read with FC3 and input registers with FC4. A device that only has one kind answers the other with exception 1 or 2.
5. **Too many registers per request.** Some devices accept far fewer than 125. In the client window use Edit > Transaction limiter > Registers to Read (5, 50, 100, 125).
6. **Transaction id.** A few devices want transaction id 0. Tick **TI always '00 00'** in the client window or pass `--zero-ti` to `ananasar client`.
7. **Timeouts.** The CLI scan waits `--timeout-ms` (1000 by default) and the GUI scanner 100 ms by default; slow devices may need more. After a missing reply the scanner stops, because the replies can no longer be matched to requests.
8. **Look at the traffic.** Turn on the [data log](#enable-the-data-log) on the Ananasar server, or compare with what the device's own software sends.

Also note that Ananasar's server serves at most two clients at a time and closes a connection that is silent for a minute.

## Modbus exceptions

A device that understands the request but cannot do it replies with an exception: the function code with bit 7 set, plus an exception code.

| Code | Name | Usual meaning |
| --- | --- | --- |
| 1 | Illegal Function | The function code is not supported (Ananasar: switched off under Options > Modbus > Active MB Functions, or unknown). |
| 2 | Illegal Data Address | The address range does not exist (Ananasar: it runs past register 65535). Often an address-offset mistake. |
| 3 | Illegal Data Value | The quantity or a field is invalid (Ananasar: count 0, more than 125 read or 123 written, or lengths that do not match). |
| 4 | Slave Device Failure | The device failed while handling the request. |
| 5 | Acknowledge | Accepted, still processing; poll again later. |
| 6 | Slave Device Busy | Busy with another command; retry later. |
| 7 | Negative Acknowledge | The program function cannot be performed. |
| 8 | Memory Parity Error | A parity error in the device's extended memory. |
| 10 (0x0A) | Gateway Path Unavailable | A gateway could not route to the target. |
| 11 (0x0B) | Gateway Target Device Failed to Respond | A gateway got no answer from the device behind it. |

Ananasar's server can send any of these on demand (Options > Modbus > Return Exception, or `server_options` in MCP) to test how a client reacts. The client window logs them as `exception FC 0x03 code 2`; `ananasar client` prints `exception on FC 0x03, code 2`.

## Enable the data log

The data log records every request and reply of the Ananasar **server**, per connection, as hex with optional decoded text.

- GUI: press **Start** in the Logging box of the main window (tick **Interpretive** first for decoded text). The file `ananasar-<port>-<UTC time>.log` appears in the working directory.
- CLI: `ananasar server --log-dir logs --interpretive`.
- MCP: `server_start` with `log_mode`, then `server_get_log`.

For the client side, the client window's log lists replies and errors; double-click clears it and right-click copies it.

## Reporting a bug

Open an [issue](https://github.com/andy778/ananasar/issues) and include:

- the version (`ananasar --version`, or File > About… in the GUI) and where you got it (release file or built from source);
- your operating system and whether you used the GUI, CLI or MCP server;
- the exact steps, the command line or the settings you used, and what you expected to happen;
- what happened instead, with any message box text or terminal output;
- for protocol problems, a data log with **Interpretive** on, trimmed to the relevant entries.

Remove addresses, register contents or names you do not want to publish; issues are public. Ananasar is a hobby project provided as is, so there is no guarantee of a reply or a fix ([SECURITY.md](../SECURITY.md)).

## See also

- [Getting started](getting-started.md)
- [GUI manual](gui.md)
- [CLI manual](cli.md)
