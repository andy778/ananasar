# Ananasar: instructions for Claude

Rust port of the public-domain Ananas Modbus/TCP tool. Crates: `ananasar-core` (std-only, I/O-free engine), `ananasar-net` (blocking server + frame reader), `ananasar-cli` (clap), `ananasar-gui` (egui), `fuzz` (excluded from the workspace).

## Workflow
- `main` is protected by a ruleset: change it through a branch and PR, with `test (linux)` and `build windows x64 exes` passing.
- Before pushing: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Workspace lints are in `Cargo.toml`.
- Core, net and CLI must build on Rust 1.92; only the GUI needs 1.95.
- All GitHub Actions stay pinned to commit SHAs, with `permissions: contents: read` unless a job needs more.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`), with a `!` or a `BREAKING CHANGE:` footer for breaking changes.

## Releases
- We want [semantic-release](https://github.com/semantic-release/semantic-release) to compute versions and changelogs from the Conventional Commits. **Not set up yet.** Today releases are cut by pushing a `v*` tag, which runs the `release` job in `build.yml`.
- When adding it, keep the existing build, cosign signing and asset upload steps and trigger them from the release it creates. Keep the crate versions in `Cargo.toml` in sync with the tag.

## Never
- Commit the original `ananas64.exe`, the manual PDF or any zip of them (they are gitignored on purpose).
- Put reverse-engineering internals (Ghidra function names, addresses) in the repo. Credit REA and the public-domain original only as the README does.
- Promise support or a private security channel in `SECURITY.md`.
