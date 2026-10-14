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
cargo update --workspace --offline
echo "stamped workspace version ${version}"
