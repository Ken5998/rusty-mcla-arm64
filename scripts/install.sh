#!/usr/bin/env bash
# Sets up Rusty MCLA (Midnight Club: Los Angeles) on an ARM64 Linux handheld:
# gets this repository's ARM64 build of the game from its GitHub releases (or
# keeps the one already here when it is the newest), gets ffmpeg, builds the
# game data from the player's own disc image with the game's own install tool,
# writes the game's settings for a handheld and DIR/game/mcla.sh, which starts
# the game. Running it again updates the game and resumes or repairs the data;
# finished steps are skipped.
#
#   scripts/install.sh DISC [DIR]
#
# DISC  your Xbox 360 disc image of Midnight Club: Los Angeles, Complete
#       Edition (.iso), or - to keep the one set before
# DIR   where everything goes (default ~/.local/share/RustyMCLA-arm64)
#
# Environment:
#   MCLA_RESOLUTION=WxH         the game's window (default 1280x720; Steam's
#                               Game Mode scales it to the screen)
#   MCLA_WINDOW_MODE=windowed|borderless|fullscreen   (default windowed)
#   MCLA_FPS_CAP=N              0 = none (default 60)
#   MCLA_ADD_TO_STEAM=1         add the game to Steam at the end
#   MCLA_ARTWORK=0              skip the Steam artwork
#   MCLA_GAME_ARCHIVE=FILE      install this RustyMCLA-game-linux-*.tar.gz
#                               instead of the newest release's (tests, CI)
#   MCLA_FFMPEG=FILE            use this ffmpeg instead of downloading one
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
disc="${1:?usage: install.sh DISC [DIR]}"
dir="${2:-$HOME/.local/share/RustyMCLA-arm64}"
resolution="${MCLA_RESOLUTION:-1280x720}"
window_mode="${MCLA_WINDOW_MODE:-windowed}"
fps_cap="${MCLA_FPS_CAP:-60}"
mkdir -p "$dir"
dir="$(cd "$dir" && pwd)"
game="$dir/game"
build="$dir/build"
tools="$dir/tools"
artwork="$dir/artwork"
mkdir -p "$game" "$tools" "$artwork"

# The space the game data takes (upstream's manifest: 13.7 GB of data and
# 1.5 GB of headroom while it is made).
DATA_SPACE="${MCLA_DATA_SPACE:-$((15200 * 1000 * 1000))}"
RELEASES_API="https://api.github.com/repos/Ken5998/rusty-mcla-arm64/releases?per_page=20"
FFMPEG_BASE="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"

step() { echo "==> $*"; }
fail() { echo "error: $*" >&2; exit 1; }

case "$(uname -m)" in
    aarch64|arm64) arch=arm64 ;;
    x86_64|amd64) arch=x86_64 ;;
    *) fail "no build for $(uname -m)" ;;
esac

# --- The disc --------------------------------------------------------------
step "Checking the disc image"
if [ "$disc" != - ]; then
    [ -f "$disc" ] || fail "$disc is not a file"
    disc="$(cd "$(dirname "$disc")" && pwd)/$(basename "$disc")"
    case "${disc,,}" in
        *.iso) ;;
        *) fail "$(basename "$disc") is not a disc image: choose the .iso of your Xbox 360 disc" ;;
    esac
    printf '%s\n' "$disc" > "$dir/disc.txt"
fi
[ -s "$dir/disc.txt" ] || fail "choose your Midnight Club: Los Angeles disc image first"
disc="$(cat "$dir/disc.txt")"
[ -f "$disc" ] || fail "the disc image set before, $disc, is gone: choose it again"
echo "Disc: $disc"

# --- The game --------------------------------------------------------------
step "Getting the game"
installed="$(cat "$build/release" 2>/dev/null || true)"
if [ -n "${MCLA_GAME_ARCHIVE:-}" ]; then
    choice="local|file://$MCLA_GAME_ARCHIVE|"
else
    auth=()
    [ -z "${GH_TOKEN:-}" ] || auth=(-H "Authorization: Bearer $GH_TOKEN")
    releases="$(curl -fsSL --retry 3 -H 'Accept: application/vnd.github+json' "${auth[@]}" "$RELEASES_API" 2>/dev/null || true)"
    choice=""
    if [ -n "$releases" ]; then
        choice="$(printf '%s' "$releases" | python3 "$repo/scripts/pick-release.py" "RustyMCLA-game-linux-$arch.tar.gz")" ||
            fail "this repository has no Linux $arch build of the game in its releases yet"
    fi
fi
if [ -z "$choice" ]; then
    [ -n "$installed" ] && [ -x "$build/bin/mcla" ] ||
        fail "could not reach GitHub to download the game: connect to the internet and try again"
    echo "Could not reach GitHub: keeping the game of release $installed"
else
    IFS='|' read -r tag url sha256 <<< "$choice"
    if [ "$tag" = "$installed" ] && [ "$tag" != local ] && [ -x "$build/bin/mcla" ]; then
        echo "The game of release $tag is already here, the newest"
    else
        echo "Getting the game of release $tag"
        download="$dir/game-download.tar.gz"
        curl -fL --retry 3 -o "$download" "$url"
        if [ -n "$sha256" ]; then
            echo "$sha256  $download" | sha256sum -c --quiet - || fail "the game download is damaged: try again"
        fi
        rm -rf "$dir/build.new"
        mkdir -p "$dir/build.new"
        tar -xzf "$download" -C "$dir/build.new" --strip-components=1
        rm -f "$download"
        [ -x "$dir/build.new/bin/mcla" ] && [ -x "$dir/build.new/bin/mcla-tool" ] || fail "the game download has no game in it"
        printf '%s\n' "$tag" > "$dir/build.new/release"
        rm -rf "$build"
        mv "$dir/build.new" "$build"
    fi
fi
sed -n 's/^upstream=/Rusty MCLA /p' "$build/VERSION" 2>/dev/null || true

# --- ffmpeg ----------------------------------------------------------------
# The install converts the disc's audio (XMA) and films with ffmpeg, which is
# never shipped with the game: a static build from BtbN/FFmpeg-Builds,
# checked against its published SHA-256.
step "Getting ffmpeg"
ffmpeg="$tools/ffmpeg/ffmpeg"
if [ -n "${MCLA_FFMPEG:-}" ]; then
    ffmpeg="$MCLA_FFMPEG"
    echo "ffmpeg: $ffmpeg"
elif [ -x "$ffmpeg" ] && "$ffmpeg" -hide_banner -decoders 2>/dev/null | grep -q ' xma2 '; then
    echo "ffmpeg is already here: $("$ffmpeg" -version | head -1)"
else
    case "$arch" in arm64) platform=linuxarm64 ;; *) platform=linux64 ;; esac
    sums="$(curl -fsSL --retry 3 "$FFMPEG_BASE/checksums.sha256")" || fail "could not reach GitHub to download ffmpeg: connect to the internet and try again"
    # The newest release branch (ffmpeg-nX.Y-latest-...), not the master snapshot.
    line="$(printf '%s\n' "$sums" | grep -E " ffmpeg-n[0-9.]+-latest-$platform-gpl-[0-9.]+\.tar\.xz$" | sort -t- -k2,2V | tail -1)"
    [ -n "$line" ] || fail "BtbN/FFmpeg-Builds has no $platform build right now"
    sha256="${line%% *}" name="${line##* }"
    echo "Downloading $name"
    curl -fL --retry 3 -o "$tools/ffmpeg.tar.xz" "$FFMPEG_BASE/$name"
    echo "$sha256  $tools/ffmpeg.tar.xz" | sha256sum -c --quiet - || fail "the ffmpeg download is damaged: try again"
    rm -rf "$tools/ffmpeg" "$tools/ffmpeg.unpack"
    mkdir -p "$tools/ffmpeg.unpack" "$tools/ffmpeg"
    tar -xJf "$tools/ffmpeg.tar.xz" -C "$tools/ffmpeg.unpack" --wildcards '*/bin/ffmpeg' '*/LICENSE.txt'
    mv "$tools"/ffmpeg.unpack/*/bin/ffmpeg "$ffmpeg"
    mv "$tools"/ffmpeg.unpack/*/LICENSE.txt "$tools/ffmpeg/" 2>/dev/null || true
    rm -rf "$tools/ffmpeg.unpack" "$tools/ffmpeg.tar.xz"
    "$ffmpeg" -version | head -1
fi

# --- The game data ---------------------------------------------------------
step "Building the game data from your disc"
if [ ! -f "$dir/manifest.local.json" ]; then
    free="$(df -PB1 "$dir" | awk 'NR == 2 { print $4 }')"
    if [ -n "$free" ] && [ "$free" -lt "$DATA_SPACE" ]; then
        fail "the game data needs about $((DATA_SPACE / 1000000000)) GB free in $dir, there are $((free / 1000000000)) GB"
    fi
fi
# The tool resumes an interrupted install and checks what is already there.
"$build/bin/mcla-tool" install "$disc" "$dir" --ffmpeg "$ffmpeg" --no-download
[ -d "$dir/data/cache" ] || fail "the install made no game data in $dir/data/cache"
# Rusty MCLA 0.6 extracts the uncompressed scripts in their plain form too; a
# folder made before keeps them as they were on the disc, which the tool's
# plainscripts command rewrites (once: it reads every file of the data).
if [ ! -f "$dir/data/.scripts-plain" ]; then
    step "Converting the game's scripts"
    "$build/bin/mcla-tool" plainscripts "$disc" "$dir/data/cache" && : > "$dir/data/.scripts-plain" ||
        echo "The scripts could not be converted: the game may miss some of its events"
fi

# --- Settings and the start script -----------------------------------------
step "Writing the settings"
user_dir="${RUSTY_MCLA_USER_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/RustyMCLA}"
# (anti-aliasing off unless the file already says: the game's ORIGINAL is 2 - 4 samples, too much for a handheld)
aa=()
grep -qE '^\s*aa\s*=' "$user_dir/settings.toml" 2>/dev/null || aa=(display.aa=off)
python3 "$repo/scripts/configure.py" "$user_dir/settings.toml" \
    "display.window_mode=$window_mode" "display.resolution=$resolution" \
    display.vsync=true "display.fps_cap=$fps_cap" "paths.data_dir=$dir/data/cache" "${aa[@]}"
echo "Settings: $user_dir/settings.toml ($resolution $window_mode, fps cap $fps_cap)"

cat > "$game/mcla.sh.tmp" <<EOF
#!/usr/bin/env bash
# Starts Midnight Club: Los Angeles (Rusty MCLA, release $(cat "$build/release")).
# Written by rusty-mcla-arm64's scripts/install.sh; run it again to change anything.
dir='$dir'
cd "\$dir/game"
exec "\$dir/build/bin/mcla" --data "\$dir/data/cache" "\$@"
EOF
chmod +x "$game/mcla.sh.tmp"
mv "$game/mcla.sh.tmp" "$game/mcla.sh"
echo "Start script: $game/mcla.sh"

# --- Artwork (only with MCLA_ARTWORK=1: it takes a while, and SteamGridDB has better)
if [ "${MCLA_ARTWORK:-0}" = 1 ]; then
    step "Drawing the Steam artwork"
    bash "$repo/scripts/artwork.sh" "$dir" || echo "The artwork could not be drawn: the Steam entry gets none"
fi

if [ "${MCLA_ADD_TO_STEAM:-0}" = 1 ]; then
    step "Adding Midnight Club: Los Angeles to Steam"
    bash "$repo/scripts/add-to-steam.sh" "$dir"
fi
echo "Midnight Club: Los Angeles is ready: $game/mcla.sh"
