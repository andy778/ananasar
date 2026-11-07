# Contributing to Ananasar

Thanks for your interest. Ananasar is a hobby project, provided as is ([SECURITY.md](SECURITY.md)): issues and pull requests are welcome, but there is no promise of a review, a reply or a merge.

The binding rules for everyone, people and AI agents alike, are in [AGENTS.md](AGENTS.md). This page walks through them.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/andy778/ananasar/issues/new/choose) and pick a form. For bugs, [Troubleshooting](docs/troubleshooting.md#reporting-a-bug) lists what helps. Issues are public, so remove addresses or register contents you do not want to publish. Security problems go in a public issue too; there is no private channel.

## Start with an issue

Every change starts from an issue, before you write code or open a pull request. Search for an existing one first; if there is none, open one and describe what you want to change. Comment on the issue that you are working on it, so nobody else starts the same thing. The pull request then links it with `Fixes #123` (or `Refs #123` if it does not finish the issue). One issue may take several pull requests, and one pull request may close several issues. Dependabot updates are the only exception.

The `pr issue` check enforces this: it fails until the pull request description has a line like `Fixes #123`, `Closes #123`, `Resolves #123` or `Refs #123` naming an existing issue.

## Setting up

Install Rust with [rustup](https://rustup.rs). The core and the headless CLI need Rust 1.92; the GUI (and so the default `ananasar` binary) needs 1.95. On Debian or Ubuntu, follow [Build from source](README.md#debian--ubuntu).

```bash
git clone https://github.com/andy778/ananasar.git && cd ananasar
cargo build
cargo run -p ananasar-cli -- gui
```

The [project layout](README.md#project-layout) in the README says what each crate does.
[ARCHITECTURE.md](ARCHITECTURE.md) explains how the crates fit together and the rules that hold between them.

## Before you open a pull request

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

To see which lines the tests reach, install [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) and run `cargo llvm-cov --html`; the report is in `target/llvm-cov/html/index.html`. CI posts a per-crate table in the summary of the `coverage` workflow.

If you touched `ananasar-core`, `-net`, `-storm`, `-mcp` or the CLI, check that they still build on Rust 1.92:

```bash
rustup toolchain install 1.92
cargo +1.92 test --locked -p ananasar-core -p ananasar-net -p ananasar-storm -p ananasar-mcp
cargo +1.92 test --locked -p ananasar-cli --no-default-features
```

- **Bug fixes come with a test** that fails without the fix. Revert the fix once and watch the test fail.
- **Network input is untrusted.** Code that reads frames must not panic, index out of bounds or allocate by a size taken from the wire.
- **Keep the change focused.** One topic per pull request; unrelated clean-ups go in their own.
- **Update the docs** in `docs/` when you change behaviour a user can see.

CI runs the same checks plus the Windows and macOS builds and `cargo-deny`.

## Commit messages and releases

Pull requests are **squash-merged**: each one lands on `main` as a single commit, signed by GitHub, whose subject is the pull request title. The title must be a [Conventional Commit](https://www.conventionalcommits.org) (the `pr title` check enforces it), and its type decides the next release, so choose it deliberately:

| Type | Release |
| --- | --- |
| `fix:`, `perf:` | patch |
| `feat:` | minor |
| `feat!:` or a `BREAKING CHANGE:` footer | major |
| `docs:`, `refactor:`, `ci:`, `chore:`, `test:`, `style:`, `build:` | none |

One pull request is one change with one type: a feature and an unrelated fix go in separate pull requests. Commits on your branch should use Conventional Commits too, since a `BREAKING CHANGE:` footer in any of them is kept in the squash commit's body. Sign your commits ([SSH or GPG](https://docs.github.com/en/authentication/managing-commit-signature-verification)); `main` accepts only signed commits. Releases, tags and release notes are automated ([Releasing](docs/releasing.md)).

## Where behaviour comes from

Ananasar is an independent reimplementation. Behaviour comes from observing the original program or from the [Modbus specification](https://modbus.org/specs.php). Never:

- commit the original program, its documentation PDF or archives of them;
- use or quote anything from the original's documentation PDF (wording, examples, section references, file names);
- add reverse-engineering internals such as disassembler function names or addresses.

## Using AI tools

AI coding tools are welcome; Ananasar itself was built with them. You are responsible for what you submit: read and understand every line, run the checks yourself and say in the pull request what you tested. Do not add tool attribution such as `Co-Authored-By` trailers. Agents find their instructions in [AGENTS.md](AGENTS.md), which `CLAUDE.md` imports.

## Licence

Ananasar is [MIT](LICENSE) licensed. Unless you say otherwise, any contribution you submit is licensed under the same terms.
