#!/usr/bin/env python3
"""List why a pull request needs the maintainer's review.

Usage: risky_paths.py .github/CODEOWNERS < files.json
files.json is the output of GET /repos/{owner}/{repo}/pulls/{n}/files (a JSON
array; `gh api --paginate` output with several arrays concatenated is accepted).
Prints one reason per line and always exits 0 (an error in the input exits 1).

The risky paths come from CODEOWNERS, so there is one list. Diff content is
checked too: an added `unsafe` block/fn/impl or `allow`/`expect` attribute in a
.rs file.
"""
import fnmatch
import json
import re
import sys

UNSAFE = re.compile(r"\bunsafe\s*\{|\bunsafe\s+(fn|impl)\b")
ALLOW = re.compile(r"#!?\[\s*(allow|expect)\s*\(")


def parse_patterns(text):
    """Return the CODEOWNERS patterns as (kind, value) pairs.

    Raises ValueError for any pattern form this checker does not implement, so
    a new form in CODEOWNERS is never silently ignored.
    """
    patterns = []
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        pat = line.split()[0]
        if pat.startswith("/") and pat.endswith("/") and not _has_glob(pat):
            patterns.append(("dir", pat.strip("/")))
        elif pat.startswith("/") and not pat.endswith("/") and not _has_glob(pat):
            patterns.append(("file", pat[1:]))
        elif pat.startswith("**/") and "/" not in pat[3:] and not _has_glob(pat[3:].replace("*", "").replace("?", "")):
            patterns.append(("name", pat[3:]))
        elif "/" not in pat and pat != "*" and not pat.startswith("!"):
            patterns.append(("name", pat))
        else:
            raise ValueError(f"unsupported CODEOWNERS pattern: {pat}")
    return patterns


def _has_glob(s):
    return any(c in s for c in "*?[!")


def matches(pattern, path):
    kind, value = pattern
    if kind == "dir":
        return path.startswith(value + "/")
    if kind == "file":
        return path == value
    return fnmatch.fnmatchcase(path.rsplit("/", 1)[-1], value)


def reasons(codeowners_text, files):
    out = []
    patterns = parse_patterns(codeowners_text)
    for f in files:
        names = [f.get("filename", "")]
        if f.get("previous_filename"):
            names.append(f["previous_filename"])
        for name in names:
            for p in patterns:
                if matches(p, name):
                    out.append(f"{name}: matches risky path {_show(p)}")
                    break
        name = names[0]
        if name.endswith(".rs"):
            patch = f.get("patch")
            if patch is None:
                out.append(f"{name}: patch not available, check by hand")
                continue
            for line in patch.splitlines():
                if not line.startswith("+"):
                    continue
                if UNSAFE.search(line):
                    out.append(f"{name}: added unsafe: {line[1:].strip()}")
                if ALLOW.search(line):
                    out.append(f"{name}: added allow/expect attribute: {line[1:].strip()}")
    return out


def _show(p):
    kind, value = p
    return {"dir": f"/{value}/", "file": f"/{value}", "name": f"**/{value}"}[kind]


def load_files(text):
    """Parse one JSON array, or several concatenated ones (gh api --paginate)."""
    dec = json.JSONDecoder()
    files, i, text = [], 0, text.strip()
    while i < len(text):
        value, end = dec.raw_decode(text, i)
        files.extend(value)
        i = end
        while i < len(text) and text[i].isspace():
            i += 1
    return files


def main(argv):
    if len(argv) != 2:
        print("usage: risky_paths.py CODEOWNERS < files.json", file=sys.stderr)
        return 1
    with open(argv[1], encoding="utf-8") as fh:
        codeowners = fh.read()
    try:
        found = reasons(codeowners, load_files(sys.stdin.read()))
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    for r in found:
        print(r)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
