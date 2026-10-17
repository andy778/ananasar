# Releasing

Releases are automated with [semantic-release](https://github.com/semantic-release/semantic-release).

1. A push to `main` runs `.github/workflows/semantic-release.yml`.
2. semantic-release reads the Conventional Commits since the last `v*` tag: `fix:` gives a patch, `feat:` a minor and a `!` or `BREAKING CHANGE:` a major release. `chore:`, `docs:`, `ci:` and `refactor:` alone release nothing.
3. If a release is due it creates the `vX.Y.Z` tag and the GitHub Release with generated notes.
4. The tag starts `build.yml`. Its `notices` job runs `scripts/make-notices.sh` (licence texts, dependency list, advisory report and CycloneDX SBOMs, embedded into `ananasar about` and attached to the release). The build jobs then stamp the version into the workspace (`scripts/stamp-version.sh`), build Windows, macOS and Linux (one `ananasar` binary per platform: `ananasar.exe` on Windows, a tarball with `ananasar` on macOS and Linux), signs the files with cosign and attaches them to the release. A `source` job adds `ananasar-<version>-source.tar.gz` and `.zip` (`git archive` of the tagged commit), and the `release` job fails before attaching anything if an expected file is missing.

`main` is protected, so merge PRs with **Rebase and merge** (or a merge commit), not squash: a squash merge turns the whole PR into a single commit and loses the individual `feat`/`fix` types.

## One-time GitHub App setup

The tag has to be pushed with a GitHub App token. A push made with the default `GITHUB_TOKEN` does not start other workflows, so the build would never run.

1. Create an app: Settings → Developer settings → GitHub Apps → New GitHub App. Any name, no webhook, no callback URL, "Only on this account".
2. Repository permissions: **Contents** read and write, **Issues** read and write, **Pull requests** read and write. Nothing else.
3. Create the app, note its **App ID**, then generate a **private key** (a `.pem` file).
4. Install the app on `andy778/ananasar` only.
5. In the repository, add Actions secrets `RELEASE_APP_ID` (the App ID) and `RELEASE_APP_PRIVATE_KEY` (the whole `.pem` contents).
6. If the first run reports that it cannot push, add the app as a bypass actor on the `protect-main` ruleset (Settings → Rules → Rulesets). Tags are not covered by that ruleset, but semantic-release also checks push access on the branch.

Without the two secrets the `semantic-release` workflow fails at its first step and nothing is released.

## See also

- [Documentation index](README.md)
- [Project README](../README.md)
