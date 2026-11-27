#!/usr/bin/env python3
"""Attaches files to an existing GitHub Release, retrying a failed upload.

Usage: scripts/upload-release-assets.py TAG FILE...

Needs `gh`, with GH_TOKEN and GH_REPO set. `gh release upload` only adds
assets: it does not update the release, whose title and notes belong to
scripts/next-release.py.
"""
import os
import subprocess
import sys
import time

# Seconds to wait after each failed attempt; one more attempt than there are
# pauses.
PAUSES = (10, 30, 60, 120)


def upload(tag, files, run=subprocess.run, sleep=time.sleep):
    """Returns 0 once an upload succeeds, 1 if a file is missing or all fail."""
    # A release without one of its files is worse than a failed job.
    missing = [f for f in files if not os.path.isfile(f)]
    if missing:
        for f in missing:
            print(f"::error::missing release asset: {f}", flush=True)
        return 1
    # --clobber replaces what an earlier, partly successful attempt left behind.
    command = ["gh", "release", "upload", tag, "--clobber", "--", *files]
    attempts = len(PAUSES) + 1
    for attempt in range(1, attempts + 1):
        if run(command).returncode == 0:
            return 0
        print(f"::warning::upload attempt {attempt} of {attempts} failed", flush=True)
        if attempt < attempts:
            sleep(PAUSES[attempt - 1])
    print(f"::error::could not attach the files to release {tag}", flush=True)
    return 1


def main(argv):
    if len(argv) < 2:
        print("usage: upload-release-assets.py TAG FILE...", file=sys.stderr)
        return 2
    return upload(argv[0], argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
