<!--
Thanks for contributing. The rules are in AGENTS.md; CONTRIBUTING.md explains them.
Delete any section that does not apply.
-->

## What this changes

<!-- What behaves differently afterwards, and why. If it fixes an issue: `Fixes #123`. -->

## How you verified it

<!-- The commands you ran and what they said. Say what you could not run. -->

- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Still builds on Rust 1.92 (core, net, storm, mcp, CLI with `--no-default-features`), if I touched them
- [ ] A bug fix has a test that fails without the fix
- [ ] Docs in `docs/` updated if user-visible behaviour changed

## Commits

- [ ] Conventional Commit types chosen deliberately (`fix` = patch, `feat` = minor, `!` = major; `docs`/`refactor`/`ci`/`chore` release nothing)
- [ ] Each commit stands on its own (PRs are merged with Rebase and merge)

## Robustness

- [ ] Code that reads network input does not panic, index out of bounds or allocate by a size taken from the wire

## Provenance

- [ ] Behaviour comes from observing the original program or from the Modbus specification
- [ ] Nothing from the original's documentation PDF: no wording, examples, section references or file names
- [ ] No original binaries or archives, and no reverse-engineering internals (function names, addresses)

## Anything a reviewer should look at first

<!-- The decision you are least sure about, or a change existing users will notice. -->
