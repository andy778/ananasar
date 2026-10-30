# Ananasar: instructions for Claude

Rust port of Ananas, the free Modbus/TCP tool, taken from http://www.tuomio.fi/ananas/. Crates: `ananasar-core` (std-only, I/O-free engine), `ananasar-net` (blocking server + frame reader), `ananasar-cli` (clap; produces the one `ananasar` binary, which also embeds `ananasar-gui` (egui, `ananasar gui`) and `ananasar-mcp` (`ananasar mcp`) through the default features `gui` and `mcp`), `fuzz` (excluded from the workspace).

## Workflow
- `main` is protected by a ruleset: change it through a branch and PR, with `test (linux)` and `build windows x64 exes` passing.
- Before pushing: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Workspace lints are in `Cargo.toml`.
- Core, net, storm, mcp and the CLI built with `--no-default-features` must build on Rust 1.92; only the GUI (and so the default `ananasar` binary) needs 1.95.
- All GitHub Actions stay pinned to commit SHAs, with `permissions: contents: read` unless a job needs more.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`), with a `!` or a `BREAKING CHANGE:` footer for breaking changes.

## Releases
- Releases are automated by `scripts/next-release.py` (run by `.github/workflows/semantic-release.yml`) from the Conventional Commits on `main` (see `docs/releasing.md`). Do not create `v*` tags by hand.
- Merge PRs with "Rebase and merge" so each commit keeps its type. Choose commit types deliberately: they decide the version bump.
- The tag build in `build.yml` stamps the version from the tag, builds, signs with cosign and attaches the binaries. the script writes the release notes.

## Never
- Commit the original `Ananas64.exe`, the documentation PDF or any zip of them (they are gitignored on purpose).
- Put reverse-engineering internals (Ghidra function names, addresses) in the repo. Credit REA and say the original was taken from http://www.tuomio.fi/ananas/ only as the README does; never name its author.
- Use anything from the Ananas documentation PDF. It was written by Wärtsilä, not by the tool's author, so nothing from it may be used or quoted: no section references, wording, examples or file naming. Behaviour comes from the program itself or from the Modbus specification.
- Promise support or a private security channel in `SECURITY.md`.
