<!--
Thanks for contributing. The rules are in AGENTS.md; CONTRIBUTING.md explains them.
Delete any section that does not apply.
-->

## Issues

<!-- Required: every PR starts from an issue (AGENTS.md, rule 1). -->
Fixes #

## What this changes

<!-- What behaves differently afterwards, and why. -->

## How you verified it

<!-- The commands you ran and what they said. Say what you could not run. -->

- [ ] `cargo xtask ci` passes; the steps it skipped are listed above
- [ ] A bug fix has a test that fails without the fix
- [ ] Docs in `docs/` updated if user-visible behaviour changed

## Commits

- [ ] The PR title is a Conventional Commit with a deliberately chosen type: it becomes the squash commit and decides the release (`fix`/`perf` = patch, `feat` = minor, `!` = major; `docs`/`refactor`/`ci`/`chore`/`test` release nothing)
- [ ] One change with one type; unrelated changes are in their own PRs
- [ ] Commits are signed

## Robustness

- [ ] Code that reads network input does not panic, index out of bounds or allocate by a size taken from the wire

## Provenance

- [ ] Behaviour comes from observing the original program or from the Modbus specification
- [ ] Nothing from the original's documentation PDF: no wording, examples, section references or file names
- [ ] No original binaries or archives
- [ ] Comments describe behaviour, not disassembler labels or addresses

## Anything a reviewer should look at first

<!-- The decision you are least sure about, or a change existing users will notice. -->
