#!/usr/bin/env python3
"""Converts `cargo audit --json` output to SARIF 2.1.0 for GitHub code scanning.

Usage: cargo audit --json | scripts/audit-sarif.py [deny.toml] > audit.sarif

Advisories listed in the `ignore` array of deny.toml (the accepted findings)
are left out. Each remaining advisory becomes a result on Cargo.lock:
vulnerabilities are errors; warnings (unmaintained, unsound, yanked) are
warnings. An empty report is still valid SARIF, so uploading it closes alerts
whose advisory has since been fixed or accepted.
"""
import json
import re
import sys


def accepted_ids(path):
    try:
        text = open(path, encoding="utf-8").read()
    except OSError:
        return set()
    match = re.search(r"^ignore\s*=\s*\[(.*?)^\]", text, re.S | re.M)
    if not match:
        return set()
    body = re.sub(r"#[^\n]*", "", match.group(1))
    return set(re.findall(r'"(RUSTSEC-[0-9]+-[0-9]+)"', body))


def main():
    report = json.load(sys.stdin)
    accepted = accepted_ids(sys.argv[1] if len(sys.argv) > 1 else "deny.toml")

    findings = [(f, "error") for f in report["vulnerabilities"]["list"]]
    for kind, items in report["warnings"].items():
        for item in items:
            if item.get("advisory"):  # a yanked crate has no advisory
                findings.append((item, "warning"))

    rules, results = {}, []
    for finding, level in findings:
        adv, pkg = finding["advisory"], finding["package"]
        if adv["id"] in accepted:
            continue
        rules.setdefault(adv["id"], {
            "id": adv["id"],
            "name": adv["id"],
            "shortDescription": {"text": adv["title"]},
            "fullDescription": {"text": adv["description"].strip()},
            "helpUri": adv.get("url") or "https://rustsec.org/advisories/" + adv["id"],
            "defaultConfiguration": {"level": level},
            "properties": {"tags": ["security", "dependency"]},
        })
        results.append({
            "ruleId": adv["id"],
            "level": level,
            "message": {"text": "%s %s: %s" % (pkg["name"], pkg["version"], adv["title"])},
            "locations": [{"physicalLocation": {
                "artifactLocation": {"uri": "Cargo.lock"},
                "region": {"startLine": 1},
            }}],
            # Stable across runs so GitHub tracks one alert per crate and advisory.
            "partialFingerprints": {
                "advisory/v1": "%s:%s:%s" % (adv["id"], pkg["name"], pkg["version"])
            },
        })

    json.dump({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "cargo-audit",
                "informationUri": "https://rustsec.org",
                "rules": list(rules.values()),
            }},
            "results": results,
        }],
    }, sys.stdout, indent=2)
    print()


main()
