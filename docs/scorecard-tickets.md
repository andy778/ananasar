# OpenSSF Scorecard — findings & tickets

Baseline run on `github.com/andy778/ananasar` with `scorecard` v (local), token
scope `repo`. **Aggregate score: 2.7 / 10** before fixes.

These are tracked as tickets and **not published** (no push, no public issues)
pending review. Checks are grouped by what was done.

## Fixed in this branch (local commits, not pushed)

| # | Check | Was | Fix |
|---|-------|-----|-----|
| T1 | License | 0 | Added `LICENSE` (MIT), matching `Cargo.toml license = "MIT"`. Also enables the README license badge. |
| T2 | Security-Policy | 0 | Added `SECURITY.md` with private vulnerability-reporting instructions. |
| T3 | Pinned-Dependencies | 0 | Pinned every GitHub Action in `build.yml` to a full commit SHA (`@<sha> # <tag>`). |
| T4 | Token-Permissions | 0 | Added top-level `permissions: contents: read` in `build.yml`; the release job keeps a scoped `contents: write`. |
| T5 | Dependency-Update-Tool | 0 | Added `.github/dependabot.yml` (cargo + github-actions, weekly). |

## Open tickets — repository settings (cannot be fixed in code)

- **T6 — Branch-Protection (0).** Enable protection on `main`: require PRs,
  require the `test` + `windows` checks to pass, disallow force-push. Via UI
  (Settings → Branches) or:
  `gh api -X PUT repos/andy778/ananasar/branches/main/protection ...`
- **T7 — Code-Review (0).** "0/3 approved changesets." Follows from T6: route
  changes through PRs with at least one approving review before merge.

## Open tickets — additional CI hardening (code, deferred)

- **T8 — SAST (0).** Partially addressed: a `cargo-deny` job (advisories, bans,
  sources — `deny.toml`) now runs in CI as a supply-chain gate. Full SAST
  (CodeQL) still deferred: CodeQL's Rust support is newer and needs validation
  so CI stays green; best enabled once the repo is public (free code scanning).
- **T9 — Signed-Releases. DONE in CI (verified on the next tag).** The release job now signs every asset keyless with cosign and attaches `*.sigstore.json` bundles. Original note:  After a release exists, add SLSA provenance /
  artifact signing to the release job (e.g. `slsa-framework/slsa-github-generator`
  or cosign) so release assets are verifiable. Best done public. Deferred.
- **T10 — Fuzzing (0). DONE.** Added `cargo-fuzz` targets `frame` and
  `server_process` (fuzz/) for the untrusted request parsers, plus a scheduled
  `fuzz` CI workflow. Local smoke: 52.6M execs (frame) and 39.0M execs
  (server_process) with zero crashes.

## Open tickets — informational / time-based (low or no action)

- **T11 — Vulnerabilities (9).** 1 finding: **RUSTSEC-2026-0192** —
  `ttf-parser 0.25.1` is **unmaintained** (not an exploit; no patched version).
  Transitive only, Linux-GUI path:
  `eframe → egui-winit → winit → sctk-adwaita → ab_glyph → owned_ttf_parser → ttf-parser`.
  Out of our control; resolves when egui/winit migrate (e.g. to `skrifa`).
  Documented in `SECURITY.md`. Track upstream; re-check on `eframe` bumps.
- **T12 — Maintained (0).** "created within the last 90 days." Time-based;
  clears with sustained commit/PR activity. No action.
- **T13 — CI-Tests (-1).** "no pull request found." Clears once changes flow
  through PRs (see T6/T7) and CI runs on them.
- **T14 — CII-Best-Practices (0).** Optional: apply for an OpenSSF Best
  Practices badge at bestpractices.coreinfrastructure.org.
- **T15 — Packaging (-1).** No packaging workflow. Optional: publish
  `ananasar-core`/`ananasar-cli` to crates.io, or ship releases (partly covered
  by the release job already added).
- **T16 — Contributors (0).** Solo project; inherent. No action.

## Expected impact of T1–T5

License, Security-Policy, and Dependency-Update-Tool move 0 → 10;
Token-Permissions and Pinned-Dependencies improve substantially. Estimated
aggregate after these fixes: ~5–6 / 10, with the remainder gated on
branch-protection/PR-review (T6/T7) and the deferred CI items.
