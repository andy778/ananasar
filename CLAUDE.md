# Ananasar: instructions for Claude

Rust port of the public-domain Ananas Modbus/TCP tool. Crates: `ananasar-core` (std-only, I/O-free engine), `ananasar-net` (blocking server + frame reader), `ananasar-cli` (clap), `ananasar-gui` (egui), `fuzz` (excluded from the workspace).

## Workflow
- `main` is protected by a ruleset: change it through a branch and PR, with `test (linux)` and `build windows x64 exes` passing.
- Before pushing: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Workspace lints are in `Cargo.toml`.
- Core, net and CLI must build on Rust 1.92; only the GUI needs 1.95.
- All GitHub Actions stay pinned to commit SHAs, with `permissions: contents: read` unless a job needs more.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`), with a `!` or a `BREAKING CHANGE:` footer for breaking changes.

## Releases
- Releases are automated with semantic-release from the Conventional Commits on `main` (see `docs/releasing.md`). Do not create `v*` tags by hand.
- Merge PRs with "Rebase and merge" so each commit keeps its type. Choose commit types deliberately: they decide the version bump.
- The tag build in `build.yml` stamps the version from the tag, builds, signs with cosign and attaches the binaries. semantic-release writes the release notes.

## Never
- Commit the original `ananas64.exe`, the manual PDF or any zip of them (they are gitignored on purpose).
- Put reverse-engineering internals (Ghidra function names, addresses) in the repo. Credit REA and the public-domain original only as the README does.
- Promise support or a private security channel in `SECURITY.md`.
