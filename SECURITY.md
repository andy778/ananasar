# Security Policy

## Supported versions

This project is pre-1.0; only the latest `main` and the most recent tagged
release receive fixes.

## Reporting a vulnerability

Please report security issues privately via GitHub's
[private vulnerability reporting](https://github.com/andy778/ananasar/security/advisories/new)
rather than opening a public issue. You can expect an initial response within
a few days.

## Scope and notes

- `ananasar` is a Modbus/TCP test tool. Modbus/TCP has no authentication or
  encryption; do not expose the server to untrusted networks.
- Known transitive advisory: `ttf-parser` (RUSTSEC-2026-0192, *unmaintained*)
  is pulled in via the `eframe` GUI stack on Linux only. It is informational
  (no known exploit, no patched version); tracked upstream in `egui`/`winit`.
