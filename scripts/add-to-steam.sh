#!/usr/bin/env bash
# Adds Midnight Club: Los Angeles to the Steam library, as a non-Steam game
# that runs natively (no Proton), with the artwork install.sh drew if any.
# install.sh runs it at the end when asked; run it by hand after an earlier
# install, or again to redo the entry.
#
#   scripts/add-to-steam.sh [DIR]
#
# DIR  install.sh's folder (default ~/.local/share/RustyMCLA-arm64)
#
# A running Steam is closed first, since it writes its own copy of the library
# back when it closes, and started again afterwards.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
dir="$(cd "${1:-$HOME/.local/share/RustyMCLA-arm64}" && pwd)"
[ -f "$dir/game/mcla.sh" ] || { echo "error: no game in $dir/game: run install.sh first" >&2; exit 1; }
mkdir -p "$dir/artwork"
python3 "$repo/scripts/add-to-steam.py" "$dir/game" "$dir/artwork"
