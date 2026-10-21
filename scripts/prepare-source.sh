#!/usr/bin/env bash
# Gets Rusty MCLA at the commit upstream.json pins and applies patches/ on top.
#
#   scripts/prepare-source.sh [DIR]   (default ./src/rusty-mcla)
#
# Running it again resets DIR to the pinned commit and applies the patches anew.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
dir="${1:-$repo/src/rusty-mcla}"
field() { python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo/upstream.json" "$1"; }
url="$(field repository)"
commit="$(field commit)"

if [ ! -d "$dir/.git" ]; then
    mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" remote add origin "$url"
fi
git -C "$dir" fetch -q --depth 1 origin "$commit"
git -C "$dir" -c advice.detachedHead=false checkout -q -f FETCH_HEAD
git -C "$dir" clean -qfdx -e target/
shopt -s nullglob
patches=("$repo"/patches/*.patch)
if [ ${#patches[@]} -gt 0 ]; then
    git -C "$dir" -c user.name=overlay -c user.email=overlay@localhost am -q --keep-cr "${patches[@]}"
fi
echo "Rusty MCLA ${commit:0:7} with ${#patches[@]} patches in $dir"
