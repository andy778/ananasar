# Security Policy

This is a personal hobby project, provided **as is** with no warranty and no
guaranteed support, maintenance, or response. See the [LICENSE](LICENSE).

## Reporting a vulnerability

Found a security issue? Just open a public
[issue](https://github.com/andy778/ananasar/issues). There is no private
disclosure process and no commitment to triage, respond, or fix within any
timeframe. Fixes, if any, land on `main`.

## Scope and notes

- `ananasar` is a Modbus/TCP test tool. Modbus/TCP has no authentication or
  encryption; do not expose the server to untrusted networks.
- Known transitive advisory: `ttf-parser` (RUSTSEC-2026-0192, *unmaintained*)
  is pulled in via the `eframe` GUI stack on Linux only. It is informational
  (no known exploit, no patched version); tracked upstream in `egui`/`winit`.
