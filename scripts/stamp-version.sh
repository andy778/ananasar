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
# populated registry cache, which CI runners do not have.
awk -v v="${version}" '
  /^name = "ananasar-(core|net|cli|gui)"$/ { print; getline; sub(/"[^"]*"/, "\"" v "\""); print; next }
  { print }
' Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock
echo "stamped workspace version ${version}"
