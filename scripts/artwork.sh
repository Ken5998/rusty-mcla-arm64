#!/usr/bin/env bash
# Draws the Steam artwork of Midnight Club: Los Angeles into DIR/artwork: the
# game takes two screenshots of itself from your own game data (the car at the
# Hollywood garage, at night and at dusk), and steam-art lays the title over
# them. Without a screen (or when the game cannot start) the artwork is drawn
# plain. Nothing of the game is downloaded or shipped for it.
#
#   scripts/artwork.sh [DIR]
#
# DIR  install.sh's folder (default ~/.local/share/RustyMCLA-arm64)
set -euo pipefail

dir="$(cd "${1:-$HOME/.local/share/RustyMCLA-arm64}" && pwd)"
build="$dir/build"
artwork="$dir/artwork"
[ -x "$build/bin/steam-art" ] || { echo "error: no steam-art in $build/bin: run install.sh first" >&2; exit 1; }
mkdir -p "$artwork"

shots=()
if [ -x "$build/bin/mcla" ] && [ -d "$dir/data/cache" ] && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] &&
    [ "${MCLA_ART_SHOTS:-1}" != 0 ]; then
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    for at in 21.5:night 19.2:dusk; do
        time="${at%%:*}" name="${at#*:}"
        echo "Taking a screenshot ($name)"
        # (a user folder of its own: the screenshot run never touches your
        # settings, saves and logs)
        if RUSTY_MCLA_USER_DIR="$work/user" timeout 180 "$build/bin/mcla" --data "$dir/data/cache" \
            --screenshot "$work/$name.png" --frames 360 --drive --time "$time" > "$work/$name.log" 2>&1 &&
            [ -s "$work/$name.png" ]; then
            shots+=("$work/$name.png")
        else
            echo "No $name screenshot ($(tail -1 "$work/$name.log" 2>/dev/null))"
        fi
    done
fi
"$build/bin/steam-art" "$artwork" "${shots[@]}"
