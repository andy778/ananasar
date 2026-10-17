#!/usr/bin/env bash
# Writes the third-party notices of a release into the directory given as $1
# (default: target/notices):
#   licenses.txt      the licence texts of all dependencies   (cargo-about)
#   dependencies.txt  one line per dependency: name version<TAB>licence
#   advisories.txt    RustSec advisories known when this ran   (cargo-audit); embedded into `ananasar about` only, not shipped
#   ananasar.cdx.json CycloneDX software bill of materials     (cargo-cyclonedx)
# The release build points ANANASAR_NOTICES at this directory, which embeds the
# first three into `ananasar about`. Needs cargo-about, cargo-audit and
# cargo-cyclonedx.
set -euo pipefail
out="${1:-target/notices}"
mkdir -p "$out"

cargo about generate about.hbs --workspace -o "$out/licenses.txt"
cargo about generate about-deps.hbs --workspace -o "$out/dependencies.txt.raw"
# One sorted, de-duplicated line per dependency; the template leaves blank lines.
grep -v '^$' "$out/dependencies.txt.raw" | LC_ALL=C sort -u > "$out/dependencies.txt"
rm -f "$out/dependencies.txt.raw"

# Findings the project accepts, with the reason, from the ignore list in
# deny.toml: one "ID<TAB>reason" line each.
accepted="$(awk '
  /^ignore *= *\[/ { in_list = 1; next }
  in_list && /^\]/ { in_list = 0 }
  in_list && /^ *#/ { sub(/^ *# ?/, ""); reason = reason (reason ? " " : "") $0; next }
  in_list && /"RUSTSEC-/ { id = $0; gsub(/[ ",]/, "", id); print id "	" reason; reason = "" }
' deny.toml)"

# No colour codes, and no --deny: that mode only adds a bare "error:" line for
# warnings such as unmaintained crates. A vulnerability still makes cargo-audit
# exit non-zero; the report is wanted either way.
audit="$(cargo audit --quiet --color never 2>&1 || true)"
found="$(printf '%s
' "$audit" | sed -n 's/^ID: *//p')"

{
  echo "Checked on $(date -u +%Y-%m-%d) against the RustSec advisory database."
  echo
  if [ -n "$audit" ]; then
    printf '%s
' "$audit"
  else
    echo "No advisories found."
  fi
  echo
  unaccepted=""
  for id in $found; do
    printf '%s
' "$accepted" | cut -f1 | grep -qx "$id" || unaccepted="$unaccepted $id"
  done
  if [ -n "$unaccepted" ]; then
    echo "NOT ACCEPTED: the findings above include${unaccepted}, which the project"
    echo "has not accepted. They are reported in full above."
    echo
  fi
  echo "Accepted findings (ignored in deny.toml; informational notices only):"
  if [ -n "$accepted" ]; then
    printf '%s
' "$accepted" | while IFS="$(printf '	')" read -r id reason; do
      if printf '%s
' "$found" | grep -qx "$id"; then state="present"; else state="not present now"; fi
      echo "  $id ($state): $reason"
    done
  else
    echo "  none"
  fi
} > "$out/advisories.txt"

# One SBOM for the shipped program. The gui and mcp libraries are built into
# `ananasar-cli` through its default features, so its SBOM covers them.
# cargo-cyclonedx writes a file next to each crate's Cargo.toml.
cargo cyclonedx --format json --spec-version 1.5 --override-filename sbom
mv crates/ananasar-cli/sbom.json "$out/ananasar.cdx.json"
# The remaining workspace crates get an sbom.json too; remove them.
rm -f crates/*/sbom.json
echo "notices written to $out"
