# Documentation

Manuals for using Ananasar, and notes on how it is tested and released.

## Manuals

| Page | What it covers |
| --- | --- |
| [Getting started](getting-started.md) | Installing on each platform, the default ports, a first client/server session in the GUI and the CLI, where files and logs go. |
| [GUI manual](gui.md) | A window-by-window tour of `ananasar-gui`: main window, menus, Set Register Value, Register Scanner, the client window and the Modbus and TCP options. |
| [CLI manual](cli.md) | Reference for `ananasar server`, `client`, `scan` and `completions`, with every option and examples. |
| [Stress testing](stress-testing.md) | Load, ramp, fault-injection and churn tests with `ananasar stress`, and what they do not replace. |
| [MCP manual](mcp.md) | Setup of `ananasar-mcp` for Claude Code and Claude Desktop, every tool, example prompts, the safety model and troubleshooting. |
| [DDE manual](dde.md) | Serving registers to Excel and other DDE clients on Windows: names, activation, operations and limitations. |
| [Troubleshooting](troubleshooting.md) | Port and firewall problems, a device that does not answer, Modbus exceptions, the data log and how to report a bug. |

## Project notes

| Page | What it covers |
| --- | --- |
| [Parity testing](parity-testing.md) | Running the same wire-level scenarios against Ananasar and the original and diffing the results. |
| [README screenshot](screenshots.md) | How the README screenshot is rendered headlessly and regenerated. |
| [Releasing](releasing.md) | How releases are cut from Conventional Commits and how the artifacts are built and signed. |

## See also

- [Project README](../README.md) for downloads, verification and building from source
- [SECURITY.md](../SECURITY.md)
