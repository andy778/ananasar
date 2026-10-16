#!/usr/bin/env bash
# Writes the third-party notices of a release into the directory given as $1
# (default: target/notices):
#   licenses.txt      the licence texts of all dependencies   (cargo-about)
#   dependencies.txt  one line per dependency: name version<TAB>licence
#   advisories.txt    RustSec advisories known when this ran   (cargo-audit)
#   sbom.cdx.json     CycloneDX software bill of materials     (cargo-cyclonedx)
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

{
  echo "Checked on $(date -u +%Y-%m-%d) against the RustSec advisory database."
  echo
  # A vulnerability makes cargo-audit exit non-zero; the report is still wanted.
  cargo audit --quiet --deny warnings 2>&1 || true
} > "$out/advisories.txt"

# The SBOM covers the three shipped programs. cargo-cyclonedx writes the files
# next to each crate's Cargo.toml.
cargo cyclonedx --format json --spec-version 1.5 --override-filename sbom
for crate in ananasar-cli ananasar-mcp ananasar-gui; do
  mv "crates/$crate/sbom.json" "$out/$crate.cdx.json"
done
# The remaining workspace crates get an sbom.json too; remove them.
rm -f crates/*/sbom.json
echo "notices written to $out"
