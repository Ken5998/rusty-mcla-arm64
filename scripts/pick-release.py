#!/usr/bin/env python3
"""Picks the newest published release of this repository that has ASSET, from
GitHub's release list (JSON on standard input), and prints TAG|URL|SHA256
(the SHA-256 GitHub records for the asset, empty when there is none).

    curl .../releases | scripts/pick-release.py RustyMCLA-game-linux-arm64.tar.gz

Exits with 1 when no release has the asset. Python's standard library only.
"""
import json
import sys


def pick(releases: list, asset: str) -> str | None:
    for release in releases:
        if release.get("draft") or release.get("prerelease"):
            continue
        for a in release.get("assets", []):
            if a.get("name") == asset:
                digest = a.get("digest") or ""
                sha256 = digest.removeprefix("sha256:") if digest.startswith("sha256:") else ""
                return f"{release['tag_name']}|{a['browser_download_url']}|{sha256}"
    return None


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        sys.exit(__doc__)
    releases = json.load(sys.stdin)
    # (the newest first, as GitHub lists them, but by publishing date to be sure)
    releases.sort(key=lambda r: r.get("published_at") or "", reverse=True)
    choice = pick(releases, argv[1])
    if choice is None:
        return 1
    print(choice)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
