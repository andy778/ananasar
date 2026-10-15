# Getting started

Install Ananasar, start it, and run a first Modbus/TCP server and client session in the GUI and on the command line.

## Install

Download the files for your platform from the [latest release](https://github.com/andy778/ananasar/releases/latest). You can [verify the download](../README.md#verify-a-download) with cosign first.

| Platform | Download | Contains |
| --- | --- | --- |
| Windows x64 | `ananasar-gui.exe`, `ananasar.exe`, `ananasar-mcp.exe` | GUI, CLI, MCP server |
| macOS (Apple silicon and Intel) | `ananasar-macos-universal.tar.gz` | `ananasar-gui`, `ananasar`, `ananasar-mcp` |
| Linux x86_64 | `ananasar-linux-x86_64.tar.gz` | `ananasar-gui`, `ananasar`, `ananasar-mcp` |

There is no installer. Put the files anywhere you like, for example in a folder on your `PATH`.

**Windows.** The executables are not code-signed, so SmartScreen may warn on first run: choose "More info", then "Run anyway".

**macOS.** The build is unsigned, so Gatekeeper blocks it until you remove the quarantine flag:

```bash
tar xzf ananasar-macos-universal.tar.gz
xattr -dr com.apple.quarantine ananasar ananasar-mcp ananasar-gui
./ananasar-gui
```

**Linux.**

```bash
tar xzf ananasar-linux-x86_64.tar.gz
./ananasar-gui
```

To build from source instead, see [Build from source](../README.md#build-from-source).

## Default port

Modbus/TCP uses port 502. Ports below 1024 need administrator rights on Linux and macOS, so the programs differ:

| Program | Default port | If the port is busy |
| --- | --- | --- |
| `ananasar-gui` on Windows | 502 | Tries the next free port (up to 20 in a row) and shows a notice. |
| `ananasar-gui` on Linux and macOS | 15020 | Same. |
| `ananasar server` (all platforms) | 502 | Fails with "Socket initialisation failed on port 502". Pass `--port`. |

`ananasar-gui --port N` and `ananasar server --port N` use exactly the port you give. See [Troubleshooting](troubleshooting.md#port-problems) if the port cannot be used.

## First session in the GUI

1. Start `ananasar-gui`. The server is already running; the Server box in the main window shows the port as *Local port*.
2. Give a register a value: choose Edit > Value List > Edit (Ctrl+E), double-click a value in the register list, type `1234` and press Enter.
3. Press **Show** in the Client box. In the client window press **Connect…** and then **OK**. The IP and port already point at the embedded server.
4. In *Read Input/Holding register* choose Holding registers, pick address 0 and press **Add**. Select **Cyclic read/write** under Polling.
5. In the main window the Server box now shows the client's IP, the poll interval and the fields of the last request, and the client's log lists the replies. Open Edit > Set Register Value (Ctrl+R) to change a value while the client polls.

To use the client against another device instead, enter that device's IP address and port in the connection dialog; for the unit id, set the client's UID box. The [GUI manual](gui.md) describes every window.

## First session in the CLI

In one terminal start a server, loading nothing so all registers are 0:

```bash
ananasar server --port 15020
```

In a second terminal scan the first ten holding registers, then poll three of them:

```bash
ananasar scan 127.0.0.1:15020 --start 0 --end 9
ananasar client 127.0.0.1:15020 --read-holding 0,1,2 -n 3
```

Stop the server with Ctrl+C. The [CLI manual](cli.md) lists all options.

## Where files and logs go

| What | Where |
| --- | --- |
| Register files (`*.ananas`) | Wherever you choose in File > Open… / Save As… (GUI) or with `ananasar server --load FILE`. |
| GUI data log | `ananasar-<port>-<UTC time>.log` in the working directory the GUI was started from. The path is shown in the status bar tooltip while logging. |
| CLI data log | The directory given to `--log-dir`, same file name pattern. |
| Settings | Ananasar keeps no settings or configuration files. |

If the working directory is not writable, starting the log shows an error; start the GUI from a writable folder instead.

## See also

- [GUI manual](gui.md)
- [CLI manual](cli.md)
- [MCP manual](mcp.md) for using Ananasar from an AI assistant
- [Troubleshooting](troubleshooting.md)
