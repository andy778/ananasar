#!/usr/bin/env python3
"""Turns a cargo-llvm-cov JSON summary into a Markdown table and a badge file.

Usage:
  cargo llvm-cov report --json --summary-only --output-path summary.json
  scripts/coverage-summary.py summary.json [--badge coverage.json]

Prints a per-crate line-coverage table (Markdown) on stdout. With --badge it
also writes a shields.io endpoint file with the total line coverage, which
coverage.yml commits to the `badges` branch on pushes to main.

Files are grouped by the `crates/<name>/` directory they live in; anything
outside `crates/` is counted under "other". The total is llvm-cov's own.
"""
import argparse
import json
import re
import sys

CRATE = re.compile(r"(?:^|[/\\])crates[/\\]([^/\\]+)[/\\]")

# shields.io colours for the badge, highest threshold first. Only the colour
# depends on them: coverage is reported, not enforced.
COLOURS = [(90, "brightgreen"), (75, "green"), (60, "yellowgreen"), (45, "yellow"), (30, "orange")]


def percent(covered, count):
    return 100.0 * covered / count if count else 0.0


def per_crate(export):
    """{crate: (covered, count)} line counts from an llvm-cov JSON export."""
    crates = {}
    for data in export["data"]:
        for f in data["files"]:
            m = CRATE.search(f["filename"])
            name = m.group(1) if m else "other"
            lines = f["summary"]["lines"]
            covered, count = crates.get(name, (0, 0))
            crates[name] = (covered + lines["covered"], count + lines["count"])
    return crates


def total(export):
    """(covered, count) over every file, as llvm-cov sums it."""
    covered = count = 0
    for data in export["data"]:
        lines = data["totals"]["lines"]
        covered += lines["covered"]
        count += lines["count"]
    return covered, count


def markdown(export):
    rows = ["| crate | lines | covered | line coverage |", "| --- | ---: | ---: | ---: |"]
    for name, (covered, count) in sorted(per_crate(export).items()):
        rows.append(f"| `{name}` | {count} | {covered} | {percent(covered, count):.1f}% |")
    covered, count = total(export)
    rows.append(f"| **total** | {count} | {covered} | **{percent(covered, count):.1f}%** |")
    return "\n".join(rows) + "\n"


def badge(export):
    pct = percent(*total(export))
    colour = next((c for limit, c in COLOURS if pct >= limit), "red")
    return {"schemaVersion": 1, "label": "coverage", "message": f"{pct:.1f}%", "color": colour}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("summary", help="output of cargo llvm-cov report --json --summary-only")
    ap.add_argument("--badge", metavar="FILE", help="also write a shields.io endpoint file")
    args = ap.parse_args(argv)
    with open(args.summary, encoding="utf-8") as f:
        export = json.load(f)
    sys.stdout.write(markdown(export))
    if args.badge:
        with open(args.badge, "w", encoding="utf-8") as f:
            json.dump(badge(export), f)
            f.write("\n")


if __name__ == "__main__":
    main()
