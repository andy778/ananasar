# Releasing

Releases are automated with [semantic-release](https://github.com/semantic-release/semantic-release).

1. A push to `main` runs `.github/workflows/semantic-release.yml`.
2. semantic-release reads the Conventional Commits since the last `v*` tag: `fix:` gives a patch, `feat:` a minor and a `!` or `BREAKING CHANGE:` a major release. `chore:`, `docs:`, `ci:` and `refactor:` alone release nothing.
3. If a release is due it creates the `vX.Y.Z` tag and the GitHub Release with generated notes.
4. The tag starts `build.yml`. Its `notices` job runs `scripts/make-notices.sh` (licence texts, dependency list, the advisory report that `ananasar about` embeds, and one CycloneDX SBOM). The build jobs then stamp the version into the workspace (`scripts/stamp-version.sh`) and build Windows, macOS and Linux (one `ananasar` binary per platform: `ananasar.exe` on Windows, a tarball with `ananasar` on macOS and Linux). The `release` job zips the licence texts, dependency list and SBOM (not the advisory report) into `ananasar-<version>-notices.zip`, fails if an expected file is missing, writes `SHA256SUMS` for the three binaries and the zip, signs that one file with cosign (`SHA256SUMS.sigstore.json`) and attaches everything. A release has six files of our own (three binaries, the notices zip, `SHA256SUMS`, `SHA256SUMS.sigstore.json`) plus the source archives GitHub adds.

To verify a download, check the signature of `SHA256SUMS` with `cosign verify-blob`, then the files with `sha256sum -c`; the commands are in the README.

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

Advisories are not part of a release. `audit.yml` runs cargo-audit on every push to `main` and daily, converts the result with `scripts/audit-sarif.py` (findings accepted in `deny.toml` are left out) and uploads it to code scanning.

## See also

- [Documentation index](README.md)
- [Project README](../README.md)
