# Contributing with an AI coding agent

This guide is for people who have an AI coding subscription but have never worked with git or Rust. It takes you from nothing installed to an open pull request (a proposed change to the project). Developers can use [CONTRIBUTING.md](../CONTRIBUTING.md) instead.

You do not need to write code yourself. Your agent does the typing; you choose the task, read what it did and answer for it.

## 1. What to expect

- Ananasar is a hobby project, provided as is. Pull requests are welcome, but there is no promise of a review, a reply or a merge ([SECURITY.md](../SECURITY.md), [CONTRIBUTING.md](../CONTRIBUTING.md)).
- **You answer for what you submit**, even if an agent wrote every line. Read the change, ask the agent to explain anything you do not follow, and say truthfully in the pull request what you tested.
- The binding rules are in [AGENTS.md](../AGENTS.md). The agent must follow them; this guide only explains how to get there.

## 2. Install what you need

You need four things. Where the steps differ by system, follow the official page rather than a copy of it here.

1. **git**, which records changes. Installers for Windows, macOS and Linux: <https://git-scm.com/downloads>.
2. **Rust**, through rustup: <https://rustup.rs>. The default build (it includes the window, `ananasar gui`) needs Rust 1.95 or newer. If Rust is already installed, bring it up to date:

   ```bash
   rustup update
   ```

   On Debian or Ubuntu the GUI also needs a few system packages; see [Build from source](../README.md#debian--ubuntu).
3. **The `gh` command**, GitHub's command-line tool: <https://cli.github.com>. Log in once and follow the prompts (choose GitHub.com and a web-browser login):

   ```bash
   gh auth login
   ```

   You need a free GitHub account for this.
4. **An AI coding agent** that can run commands on your computer and reads `AGENTS.md`. Examples are Claude Code, Codex CLI, GitHub Copilot, Gemini CLI and Cursor. None is required. Claude Code reads `AGENTS.md` through `CLAUDE.md`, which imports it.

On Windows, run the commands in this guide in PowerShell or Git Bash.

## 3. Set up commit signing

The `main` branch accepts only *signed* commits, which prove who made a change. Set this up once, using GitHub's own instructions for details: [Telling git about your signing key](https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key) and [Generating a new SSH key](https://docs.github.com/en/authentication/connecting-to-github-with-ssh/generating-a-new-ssh-key-and-adding-it-to-the-ssh-agent).

1. Generate a key (press Enter to accept the defaults; a passphrase is a good idea):

   ```bash
   ssh-keygen -t ed25519 -C "you@example.com"
   ```

2. Add the public key (the `.pub` file) to GitHub as a **signing key**: GitHub, Settings, SSH and GPG keys, New SSH key, and set *Key type* to **Signing Key**. A key added only as an *Authentication Key* does not make commits show as verified.
3. Tell git to sign with it. Replace the path with your own `.pub` file:

   ```bash
   git config --global gpg.format ssh
   git config --global user.signingkey ~/.ssh/id_ed25519.pub
   git config --global commit.gpgsign true
   ```

   Also set the name and email git writes on your commits (use the email your GitHub account accepts):

   ```bash
   git config --global user.name "Your Name"
   git config --global user.email "you@example.com"
   ```

4. After your first commit (section 4 onwards), check it:

   ```bash
   git log --show-signature -1
   ```

   It should say the signature is good. If it does not, your agent can help you debug it; show it the output.

Issue [#190](https://github.com/andy778/ananasar/issues/190) may relax the signing rule for branches in forks. Do not wait for it.

## 4. Fork and clone

A fork is your own copy of the project on GitHub, where you are free to push changes.

```bash
gh repo fork andy778/ananasar --clone
cd ananasar
```

Later, to bring your fork up to date with the original, run these inside the clone:

```bash
gh repo sync
git pull
```

Make a new branch for each issue, named after it:

```bash
git switch -c fix-issue-123
```

## 5. Find work

1. Open the list of [issues labelled `agent-ready`](https://github.com/andy778/ananasar/issues?q=is%3Aissue+is%3Aopen+label%3Aagent-ready). These are small and precise enough for an agent to finish. Issues without the label need the maintainer's go-ahead first: ask in a comment on the issue.
2. Read the issue **and all its comments**. Together they are the whole brief.
3. Check that nobody is already on it: no open pull request should link it, and nobody should have commented that they are taking it. In the issue, the list of linked pull requests is shown on the right; you can also search the [open pull requests](https://github.com/andy778/ananasar/pulls) for the issue number.
4. Comment on the issue that you are taking it ([AGENTS.md](../AGENTS.md), rule 1). Replace 123 with the issue number:

   ```bash
   gh issue comment 123 --repo andy778/ananasar --body "Taking this."
   ```

5. Find the **Done when** list in the issue. It is a set of checks anyone can run. Your pull request is finished when every one is true, and a reviewer will check it against that list.

## 6. Give the agent its task

Start your agent in the cloned `ananasar` folder, on your new branch, and paste this, replacing `123` with the issue number:

```text
Do GitHub issue andy778/ananasar#123. Read the issue and all its comments
(gh issue view 123 --repo andy778/ananasar --comments) and follow AGENTS.md.
Keep to the issue's scope and change nothing else. Use signed commits with
Conventional Commit messages and no agent attribution. Run `cargo xtask ci`
and fix what it reports. Then open a pull request whose title is a
Conventional Commit and whose description links the issue with `Fixes #123`,
and tell me which steps of `cargo xtask ci` were skipped.
```

Before the agent pushes anything, **read what it did**: look at the changed files (`git diff main`) or ask it to summarise each change. Ask it to explain anything you do not follow. If it changed something the issue did not ask for, have it undo that.

## 7. Before opening the pull request

- `cargo xtask ci` passes. It runs the same checks as GitHub and ends with a summary. It skips, with a message, any step whose tool is not installed; the summary lists them. **Say which steps were skipped in the pull request.**
- The change touches only what the issue asked for.
- The pull request description, which comes from a template, is filled in truthfully. Ticking a box for something that did not happen is worse than leaving it empty.
- The title is a [Conventional Commit](https://www.conventionalcommits.org), such as `docs: add a guide for contributors` or `fix: reject a short frame`. The word before the colon decides whether a release is made, so [CONTRIBUTING.md](../CONTRIBUTING.md#commit-messages-and-releases) lists the types; when unsure, ask the maintainer in the issue.
- The description contains a line such as `Fixes #123` (or `Refs #123` if the pull request does not finish the issue).

## 8. What happens next

### The automatic checks

GitHub runs checks on your pull request. See their state with:

```bash
gh pr checks
```

| Check | What a failure usually means |
| --- | --- |
| `ci passed` | Formatting, linting, a test or a build failed. Run `cargo xtask ci` again and read the first error. |
| `pr title` | The title is not a Conventional Commit. Edit the title. |
| `pr issue` | The description has no line like `Fixes #123` naming an existing issue. Edit the description. |
| fuzz | A fuzz test found an input that breaks the code. Run the log through your agent; this is a real bug in the change. |
| CodeQL | The security scan flagged something. Read the finding and have the agent fix it. |

To give a failure to your agent, fetch the log of the failed step and paste it in:

```bash
gh run view --log-failed
```

(`gh run view` without arguments lets you pick a run; `gh pr checks` shows which one failed.) Ask the agent to fix it, check the fix yourself, then push again; the checks rerun.

### Review

The maintainer reviews when time allows. If there are comments, ask your agent to change the branch, and push again. The pull request updates itself.

Never force-push over someone else's branch. Only push to your own.

## 9. What the agent must never do

These come from [AGENTS.md](../AGENTS.md), which is the binding version. Make sure your agent knows them:

- Never commit the original `Ananas64.exe`, its documentation PDF or any archive of them.
- Never use or quote anything from that PDF: no wording, examples, section references or file names.
- Never name the original's author. The README says where the original came from; do not add more.
- Never promise support or a private security channel in `SECURITY.md`.
- Never create tags or releases. They are automated.
- Ask the maintainer first before anything hard to undo: force-pushes, new dependencies with a new licence, or changes to what the automatic checks are allowed to do.
- Never add agent attribution (such as `Co-Authored-By` lines) to commits or pull requests.
