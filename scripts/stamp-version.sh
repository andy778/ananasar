#!/usr/bin/env bash
# On a v* tag build, set the workspace version to the tag so the binaries report
# the released version (e.g. `ananasar --version`). Does nothing otherwise.
set -euo pipefail
case "${GITHUB_REF:-}" in
  refs/tags/v*) ;;
  *) exit 0 ;;
esac
version="${GITHUB_REF_NAME#v}"
sed -i.bak -E "s/^version = \".*\"/version = \"${version}\"/" Cargo.toml
rm -f Cargo.toml.bak
# Cargo.lock must agree with the new version for `--locked` builds. Edit the
# workspace packages' entries directly: `cargo update --offline` needs a
# populated registry cache, which CI runners do not have. Workspace members are
# the `ananasar-*` packages with no `source` line (registry and git packages
# have one), so new crates are picked up without a hard-coded list.
awk -v v="${version}" '
  function flush(   i) {
    if (n == 0) return
    if (member) sub(/"[^"]*"/, "\"" v "\"", blk[3])
    for (i = 1; i <= n; i++) print blk[i]
    n = 0; member = 0
  }
  /^\[\[package\]\]$/ { flush() }
  { blk[++n] = $0 }
  /^source = / { src = 1 }
  /^name = "ananasar-/ { cand = 1 }
  /^$/ { if (cand && !src) member = 1; flush(); cand = 0; src = 0 }
  END { if (cand && !src) member = 1; flush() }
' Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock
echo "stamped workspace version ${version}"
