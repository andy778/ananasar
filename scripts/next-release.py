#!/usr/bin/env python3
"""Works out the next release from the Conventional Commits since the last v* tag.

Usage:
  scripts/next-release.py [--dry-run] [--ref REF] [--repo OWNER/NAME]
  scripts/next-release.py --create [--ref REF] [--repo OWNER/NAME]

Bump rules (the highest one wins):
  major  a `!` after the type, or a `BREAKING CHANGE:` footer
  minor  feat
  patch  fix, perf
  none   chore, docs, ci, refactor, test, style, build (alone: no release)

The previous release is the highest v<major>.<minor>.<patch> tag reachable from
REF (default HEAD); with none, the first release is 1.0.0. Merge commits are
ignored. With no releasable commit nothing is printed and nothing is created.

Without --create the version and the Markdown release notes are printed and
nothing is changed (--dry-run is the same, spelled out). With --create the
release is made with `gh release create vX.Y.Z --target <sha>`; gh needs
GH_TOKEN, and the tag push it causes starts build.yml.
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile

MINOR = {"feat"}
PATCH = {"fix", "perf"}
LEVELS = {"none": 0, "patch": 1, "minor": 2, "major": 3}
SECTIONS = [("feat", "Features"), ("fix", "Fixes"), ("perf", "Performance")]

SUBJECT = re.compile(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:\s*(?P<text>.+)$")
FOOTER = re.compile(r"^BREAKING[ -]CHANGE:\s*(?P<text>.*)$", re.M)
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")


def git(*args):
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True, encoding="utf-8"
    ).stdout


def parse_commit(sha, subject, body):
    """Returns a dict for a Conventional Commit, or None for anything else."""
    m = SUBJECT.match(subject.strip())
    if not m:
        return None
    footer = FOOTER.search(body)
    breaking = bool(m.group("bang")) or footer is not None
    kind = m.group("type").lower()
    if breaking:
        level = "major"
    elif kind in MINOR:
        level = "minor"
    elif kind in PATCH:
        level = "patch"
    else:
        level = "none"
    return {
        "sha": sha,
        "type": kind,
        "scope": m.group("scope") or "",
        "text": m.group("text").strip(),
        "breaking": breaking,
        "note": footer.group("text").strip() if footer else "",
        "level": level,
    }


def parse_log(raw):
    commits = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        sha, subject, body = (record.split("\x1f") + ["", ""])[:3]
        commit = parse_commit(sha, subject, body)
        if commit:
            commits.append(commit)
    return commits


def last_tag(ref):
    best = None
    for tag in git("tag", "--merged", ref, "--list", "v*").split():
        m = TAG.match(tag)
        if m:
            key = tuple(int(n) for n in m.groups())
            if best is None or key > best[0]:
                best = (key, tag)
    return best[1] if best else None


def bump(version, level):
    major, minor, patch = version
    if level == "major":
        return (major + 1, 0, 0)
    if level == "minor":
        return (major, minor + 1, 0)
    return (major, minor, patch + 1)


def next_version(tag, commits):
    """The next (major, minor, patch), or None when nothing is releasable."""
    level = max((c["level"] for c in commits), key=LEVELS.get, default="none")
    if level == "none":
        return None
    if tag is None:
        return (1, 0, 0)
    return bump(tuple(int(n) for n in TAG.match(tag).groups()), level)


def line(commit, repo):
    scope = f"**{commit['scope']}:** " if commit["scope"] else ""
    short = commit["sha"][:7]
    return f"- {scope}{commit['text']} ([{short}](https://github.com/{repo}/commit/{commit['sha']}))"


def notes(version, tag, commits, repo):
    name = "v%d.%d.%d" % version
    out = []
    if tag:
        out.append(f"## [{name[1:]}](https://github.com/{repo}/compare/{tag}...{name})")
    else:
        out.append(f"## {name[1:]}")
    breaking = [c for c in commits if c["breaking"]]
    if breaking:
        out += ["", "### Breaking changes", ""]
        for c in breaking:
            out.append(line(c, repo))
            if c["note"]:
                out.append(f"  - {c['note']}")
    for kind, title in SECTIONS:
        items = [c for c in commits if c["type"] == kind and not c["breaking"]]
        if items:
            out += ["", f"### {title}", ""]
            out += [line(c, repo) for c in items]
    return "\n".join(out) + "\n"


def repo_name(arg):
    if arg:
        return arg
    if os.environ.get("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]
    url = git("remote", "get-url", "origin").strip()
    m = re.search(r"github\.com[:/](.+?)(?:\.git)?$", url)
    if not m:
        sys.exit(f"cannot work out the repository from {url}; pass --repo")
    return m.group(1)


def plan(ref, repo):
    """Returns (version string, tag, head sha, notes) or None."""
    tag = last_tag(ref)
    span = f"{tag}..{ref}" if tag else ref
    raw = git("log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", span)
    commits = parse_log(raw)
    version = next_version(tag, commits)
    if version is None:
        return None
    head = git("rev-parse", ref).strip()
    return "v%d.%d.%d" % version, tag, head, notes(version, tag, commits, repo)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--ref", default="HEAD")
    ap.add_argument("--repo", help="OWNER/NAME for the links (default: origin)")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="print only (the default)")
    mode.add_argument("--create", action="store_true", help="create the GitHub release")
    args = ap.parse_args()

    result = plan(args.ref, repo_name(args.repo))
    if result is None:
        print("No release: no feat, fix, perf or breaking commit since the last tag.")
        return
    version, tag, head, body = result
    print(f"Version: {version} (previous: {tag or 'none'}, target: {head[:7]})\n")
    print(body)
    if not args.create:
        return
    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False, encoding="utf-8") as f:
        f.write(body)
    try:
        subprocess.run(
            ["gh", "release", "create", version, "--target", head, "--title", version,
             "--notes-file", f.name],
            check=True,
        )
    finally:
        os.unlink(f.name)


if __name__ == "__main__":
    main()
