# Contributing to Ananasar

Thanks for your interest. Ananasar is a hobby project, provided as is ([SECURITY.md](SECURITY.md)): issues and pull requests are welcome, but there is no promise of a review, a reply or a merge.

New to git, or working through an AI coding agent? Start with the [guide for non-developers](docs/contributing-with-ai.md).

The binding rules for everyone, people and AI agents alike, are in [AGENTS.md](AGENTS.md). This page walks through them.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/andy778/ananasar/issues/new/choose) and pick a form. For bugs, [Troubleshooting](docs/troubleshooting.md#reporting-a-bug) lists what helps. Issues are public, so remove addresses or register contents you do not want to publish. Security problems go in a public issue too; there is no private channel.

## Start with an issue

Every change starts from an issue, before you write code or open a pull request. Search for an existing one first; if there is none, open one and describe what you want to change. Comment on the issue that you are working on it, so nobody else starts the same thing. The pull request then links it with `Fixes #123` (or `Refs #123` if it does not finish the issue). One issue may take several pull requests, and one pull request may close several issues. Dependabot updates are the only exception.

The `pr issue` check enforces this: it fails until the pull request description has a line like `Fixes #123`, `Closes #123`, `Resolves #123` or `Refs #123` naming an existing issue.

Each issue says when it is **done**: the forms ask for a "Done when" (required for features, optional for bugs), something anyone can check, such as a command and its output. A pull request is finished when it meets it, and a reviewer checks it against it. Agents pick up only `agent-ready` issues unless the person assigning the work says otherwise.

### Agent-ready issues

An `agent-ready` issue is small and specific enough that an agent can finish it in one session without making a design decision. The issue and its comments are the whole brief: an agent starting with no other context, no chat history and no notes, must be able to finish it from them. Besides the problem, it states:

- **Where:** the files or crates it touches.
- **Tests:** the tests to add, and what each one checks. A bug fix names the test that fails without the fix.
- **Out of scope:** what not to change, including tempting nearby fixes.
- **Done when:** checks anyone can run, such as a command and the output or exit code it should give.

Size: one crate (or one workflow, or one area of the docs) and up to a few hundred changed lines, tests not counted. Anything bigger is split into several issues, each `agent-ready` on its own, and the later ones say which issue they wait for.

Before adding the label, the maintainer checks that:

1. The problem and the wanted behaviour are stated, with no design decision left open, and nothing depends on context outside the issue.
2. Where, Tests, Out of scope and Done when are filled in as above.
3. It fits the size limit.
4. It needs nothing only the maintainer can do: no repository settings, rulesets, secrets, releases or new CI permissions.
5. It does not wait on an open pull request, or it says which one.

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

Run the checks CI runs with one command:

```bash
cargo xtask ci
```

It runs `cargo fmt --check`, clippy, the tests, a check of the crate dependency graph against ARCHITECTURE.md (`cargo xtask layers` on its own), the Rust 1.92 builds of `ananasar-core`, `-net`, `-storm`, `-mcp` and the headless CLI, `cargo-deny` and the release script tests, and ends with a summary. A step whose tool is missing is skipped with a message saying how to install it; for the full set:

```bash
rustup toolchain install 1.92
cargo install --locked cargo-deny
```

- **Bug fixes come with a test** that fails without the fix. Revert the fix once and watch the test fail.
- **Network input is untrusted.** Code that reads frames must not panic, index out of bounds or allocate by a size taken from the wire.
- **Keep the change focused.** One topic per pull request; unrelated clean-ups go in their own.
- **Update the docs** in `docs/` when you change behaviour a user can see.

CI runs the same checks plus the fuzzers, CodeQL and the Windows and macOS builds.

To see which lines the tests reach, install [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) and run `cargo llvm-cov --html`; the report is in `target/llvm-cov/html/index.html`. CI posts a per-crate table in the summary of the `coverage` workflow.

## Commit messages and releases

Pull requests are **squash-merged**: each one lands on `main` as a single commit, signed by GitHub, whose subject is the pull request title. The title must be a [Conventional Commit](https://www.conventionalcommits.org) (the `pr title` check enforces it), and its type decides the next release, so choose it deliberately:

| Type | Release |
| --- | --- |
| `fix:`, `perf:` | patch |
| `feat:` | minor |
| `feat!:` or a `BREAKING CHANGE:` footer | major |
| `docs:`, `refactor:`, `ci:`, `chore:`, `test:`, `style:`, `build:` | none |

One pull request is one change with one type: a feature and an unrelated fix go in separate pull requests. Commits on your branch should use Conventional Commits too, since a `BREAKING CHANGE:` footer in any of them is kept in the squash commit's body. Commits on `main` are signed: PRs are squash-merged, and GitHub signs each squash commit. Signing your branch commits is welcome but not required ([SSH or GPG](https://docs.github.com/en/authentication/managing-commit-signature-verification)). Releases, tags and release notes are automated ([Releasing](docs/releasing.md)).

## Where behaviour comes from

Ananasar is an independent reimplementation. Behaviour comes from observing the original program or from the [Modbus specification](https://modbus.org/specs.php). Never:

- commit the original program, its documentation PDF or archives of them;
- use or quote anything from the original's documentation PDF (wording, examples, section references, file names).

Comments and docs describe what the code does, in terms of the Modbus specification and Ananasar's own code, not disassembler labels or addresses in the original binary: those mean nothing to a reader and fit only one build.

## The `needs-maintainer` label

A workflow labels a PR `needs-maintainer` when it touches something that keeps the project safe, and CODEOWNERS asks the maintainer to review it. The paths are listed in [.github/CODEOWNERS](.github/CODEOWNERS) (CI, dependencies and `Cargo.lock`, `build.rs`, `clippy.toml`, `xtask/`, the frame parsers and server dispatch, `scripts/`, the agent rules). It also fires when a `.rs` file gains an added `unsafe` block, fn, impl, trait or extern block, a `#[unsafe(` attribute, or a new `#[allow(`, `#![allow(` or `#[expect(` attribute (also inside `#[cfg_attr(`). Lines that only mention these in a comment or string are ignored. Such PRs wait for the maintainer; the label goes away if a later push removes every trigger.

## Using AI tools

AI coding tools are welcome; Ananasar itself was built with them. You are responsible for what you submit: read and understand every line, run the checks yourself and say in the pull request what you tested. Do not add tool attribution such as `Co-Authored-By` trailers. Agents find their instructions in [AGENTS.md](AGENTS.md), which `CLAUDE.md` imports.

## Licence

Ananasar is [MIT](LICENSE) licensed. Unless you say otherwise, any contribution you submit is licensed under the same terms.
