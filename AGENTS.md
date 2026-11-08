# Ananasar: rules for agents and contributors

These rules apply to every contributor, human or AI agent. `CLAUDE.md` imports this file; [CONTRIBUTING.md](CONTRIBUTING.md) is the human-friendly walkthrough.

Rust port of Ananas, the free Modbus/TCP tool, taken from http://www.tuomio.fi/ananas/.

## Crates
- `ananasar-core`: std-only, I/O-free protocol engine (framing, register model, server dispatch, poll engine, log formatting). Keep it free of sockets, threads and files.
- `ananasar-net`: blocking server and frame reader.
- `ananasar-cli`: clap; produces the one `ananasar` binary, which also embeds `ananasar-gui` (egui, `ananasar gui`) and `ananasar-mcp` (`ananasar mcp`) through the default features `gui` and `mcp`.
- `ananasar-storm` (`ananasar storm`), `ananasar-dde` (Windows DDE server used by the GUI), `ananasar-parity` (wire-level comparison runner).
- `fuzz`: cargo-fuzz targets, excluded from the workspace.
- `xtask`: repository tasks behind the `cargo xtask` alias (`cargo xtask ci`).
- [ARCHITECTURE.md](ARCHITECTURE.md) maps the crates, what each may depend on and the invariants between them.

## Commands
- All of CI's local checks: `cargo xtask ci` (fmt, clippy, tests, layers, the Rust 1.92 builds, cargo-deny, the script tests). It skips, with a message, any step whose toolchain or tool is not installed, and prints a summary. The single commands:
- Layers: `cargo xtask layers` checks the crate dependency graph against ARCHITECTURE.md.
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` (workspace lints are in `Cargo.toml`).
- Test: `cargo test --workspace --locked`.
- Coverage: `cargo llvm-cov --html` (needs `cargo-llvm-cov`; `coverage.yml` covers core, net, storm, mcp, gui and the CLI without `gui`).
- Rust 1.92 check: `cargo +1.92 test --locked -p ananasar-core -p ananasar-net -p ananasar-storm -p ananasar-mcp` and `cargo +1.92 test --locked -p ananasar-cli --no-default-features`. Only the GUI (and so the default `ananasar` binary) needs 1.95.
- Fuzz: `cargo +nightly fuzz run frame`.
- Supply chain: `cargo deny check advisories bans sources licenses`.

## Workflow
- `main` is protected by a ruleset: change it through a branch and PR, with the required checks (`ci passed`, `pr title`, fuzz and CodeQL) passing.
- Run `cargo xtask ci` before pushing, and say in the PR which steps it skipped.
- All GitHub Actions stay pinned to commit SHAs, with `permissions: contents: read` unless a job needs more. The one CI exception is the `badge` job in `coverage.yml`: on pushes to `main` it has `contents: write` to commit `coverage.json` to the orphan `badges` branch. The other is `risky-paths.yml`: it has `pull-requests: write` to label PRs `needs-maintainer`, and under `pull_request_target` it must never check out or run the PR's code.
- Commit messages use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`), with a `!` or a `BREAKING CHANGE:` footer for breaking changes.
- Commits and PRs carry no agent attribution (no `Co-Authored-By` trailers for tools). The person who opens a PR answers for it.

## Releases
- Releases are automated by `scripts/next-release.py` (run by `.github/workflows/semantic-release.yml`) from the Conventional Commits on `main` (see `docs/releasing.md`). Do not create `v*` tags by hand. `python3 scripts/next-release.py --dry-run` shows the next release; its tests are `python3 -m unittest discover -s scripts`.
- PRs are squash-merged: the PR title becomes the commit on `main` and decides the version bump, so it must be a Conventional Commit with a deliberately chosen type (the `pr title` check enforces this). One PR is one change with one type.
- Commits are signed (SSH or GPG); `main` requires signed commits.
- The tag build in `build.yml` stamps the version from the tag, builds, signs with cosign and attaches the binaries. The script writes the release notes.

## Working rules
1. **Issue first.** Every change starts from an open issue, before any code is written or a PR is opened; open one if none exists. Read it and its comments, search open PRs for it, and comment on the issue that you are taking it. Every PR links its issues (`Fixes #N`, or `Refs #N` when it does not close them). Only Dependabot PRs are exempt. Agents pick up only issues labelled `agent-ready` (it states where, the tests to add, what is out of scope and a "Done when" that can be checked; see [CONTRIBUTING.md](CONTRIBUTING.md#agent-ready-issues)), unless the person assigning the work says otherwise. A PR is done when it meets the issue's "Done when".
2. **Verify, don't guess.** Open a definition before using it, and confirm a bug's cause in the code.
3. **Keep fixes narrow and tested.** Fix what the issue describes. A bug fix comes with a test that fails without the fix.
4. **Review the diff before opening a PR.** Nothing unrelated changed, and the description is true.
5. **Report only what ran.** Say what you tested and what you could not run.
6. **Network input is hostile.** Parsers and the server must not panic or overflow on any frame; enforce the Modbus limits and answer with an exception instead. `ananasar-core`, `ananasar-net`, `ananasar-mcp` and `ananasar-storm` deny `clippy::unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` and `unimplemented` outside tests; where something truly cannot fail, use a narrow `#[allow]` with a reason saying why.
7. **Guards say why.** A check that looks removable has a comment saying what it prevents. Read it before changing it.
8. **Ask before** anything hard to undo: releases, tags, force-pushes, new dependencies with a new licence, or changes to CI permissions.
9. **Describe behaviour, not the binary.** Comments and docs say what the code does, in terms of the Modbus specification and Ananasar's own code. They do not cite disassembler labels (`FUN_…`, `DAT_…`) or addresses in the original binary: those mean nothing to readers and fit only one build of it.

## Never
- Commit the original `Ananas64.exe`, the documentation PDF or any zip of them (they are gitignored on purpose).
- Name the original's author. Credit REA and say the original was taken from http://www.tuomio.fi/ananas/ only as the README does.
- Use anything from the Ananas documentation PDF. It was not written by the tool's author, so nothing from it may be used or quoted: no section references, wording, examples or file naming. Behaviour comes from the program itself or from the Modbus specification.
- Promise support or a private security channel in `SECURITY.md`.
