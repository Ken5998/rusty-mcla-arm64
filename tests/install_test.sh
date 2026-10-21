#!/usr/bin/env bash
# Runs scripts/install.sh against a made-up game build (stand-ins for mcla,
# mcla-tool and steam-art that print what they are asked), a fake disc and
# a made-up Steam library, then checks the settings, the start script, the
# artwork and a second run that keeps everything.
#
#   tests/install_test.sh
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL $*"; exit 1; }
ok() { echo "ok   $*"; }

game="$work/stage/RustyMCLA-game"
mkdir -p "$game/bin" "$work/discs" "$work/home/.local/share/Steam/userdata/7/config"
cat > "$game/bin/mcla-tool" <<'TOOL'
#!/usr/bin/env bash
# install <iso> <folder> ...
[ "$1" = install ] || exit 2
echo "[extract] 100 / 400 MB (25%)" >&2
mkdir -p "$3/data/cache"
echo "$@" > "$3/tool-args"
echo '{}' > "$3/manifest.local.json"
TOOL
printf '#!/usr/bin/env bash\necho "mcla $*"\n' > "$game/bin/mcla"
cat > "$game/bin/steam-art" <<'ART'
#!/usr/bin/env bash
out="$1"; shift
mkdir -p "$out"
# (a 1x1 PNG for each picture)
for n in hero wide capsule logo icon; do
    printf '\x89PNG\r\n\x1a\n' > "$out/$n.png"
done
echo "artwork in $out ($# screenshots)"
ART
chmod +x "$game/bin"/*
echo "upstream=0.3.0" > "$game/VERSION"
tar -C "$work/stage" -czf "$work/game.tar.gz" RustyMCLA-game
printf '#!/usr/bin/env bash\necho "ffmpeg version test"\n' > "$work/ffmpeg"
chmod +x "$work/ffmpeg"
disc="$work/discs/Midnight Club - Los Angeles - Complete Edition (USA, Europe).iso"
truncate -s 10M "$disc"

export HOME="$work/home" XDG_DATA_HOME="$work/home/.local/share" MCLA_GAME_ARCHIVE="$work/game.tar.gz"
export MCLA_FFMPEG="$work/ffmpeg" MCLA_ADD_TO_STEAM=1 MCLA_DATA_SPACE=1000
unset DISPLAY WAYLAND_DISPLAY
dir="$work/home/.local/share/RustyMCLA-arm64"
bash "$repo/scripts/install.sh" "$disc" > "$work/first.log" 2>&1 || { cat "$work/first.log"; fail "the install"; }
ok "the install runs"
[ -x "$dir/build/bin/mcla" ] && [ "$(cat "$dir/build/release")" = local ] || fail "the game build is unpacked"
ok "the game build is unpacked"
grep -qF "install $disc $dir --ffmpeg $work/ffmpeg --no-download" "$dir/tool-args" || fail "mcla-tool gets the disc, the folder and ffmpeg"
ok "mcla-tool gets the disc, the folder and ffmpeg"
settings="$work/home/.local/share/RustyMCLA/settings.toml"
grep -qx 'resolution = "1280x720"' "$settings" && grep -qx 'window_mode = "windowed"' "$settings" &&
    grep -qx 'fps_cap = 60' "$settings" && grep -qx "data_dir = \"$dir/data/cache\"" "$settings" ||
    { cat "$settings"; fail "the settings are 1280x720, windowed, 60 fps and point at the data"; }
ok "the settings are 1280x720, windowed, 60 fps and point at the data"
[ "$("$dir/game/mcla.sh" --x)" = "mcla --data $dir/data/cache --x" ] || fail "mcla.sh starts the game with its data"
ok "mcla.sh starts the game with its data"
[ "$(ls "$dir/artwork"/*.png | wc -l)" = 5 ] || fail "five pictures of artwork"
grep -q "artwork in $dir/artwork (0 screenshots)" "$work/first.log" || fail "no screenshots without a screen"
ok "the artwork is drawn plain without a screen"
grid="$work/home/.local/share/Steam/userdata/7/config/grid"
[ "$(ls "$grid" | wc -l)" = 5 ] || fail "the artwork is in Steam's grid folder"
ok "the game is in Steam with its artwork"

# A second run with "-" keeps the disc; a settings change is written, other keys stay.
printf '[game]\nintro = "fast"\n' >> "$settings"
MCLA_RESOLUTION=1920x1080 MCLA_WINDOW_MODE=borderless MCLA_FPS_CAP=30 MCLA_ADD_TO_STEAM=0 \
    bash "$repo/scripts/install.sh" - > "$work/second.log" 2>&1 || { cat "$work/second.log"; fail "the second run"; }
grep -qF "Disc: $disc" "$work/second.log" || fail "the second run keeps the disc"
grep -qx 'resolution = "1920x1080"' "$settings" && grep -qx 'window_mode = "borderless"' "$settings" &&
    grep -qx 'fps_cap = 30' "$settings" && grep -qx 'intro = "fast"' "$settings" || { cat "$settings"; fail "the settings change"; }
ok "a second run keeps the disc and changes only the settings it sets"

bash "$repo/scripts/install.sh" "$work/discs/nothing.iso" > "$work/third.log" 2>&1 && fail "a missing disc stops the install"
grep -q "error: .*nothing.iso is not a file" "$work/third.log" || fail "a missing disc is named"
ok "a missing disc stops the install with its name"
echo "all checks passed"
