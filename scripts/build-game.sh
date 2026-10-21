#!/usr/bin/env bash
# Builds the game (mcla), its install tool (mcla-tool) and the Steam artwork
# tool (tools/steam-art) for this machine, the game from the pinned upstream
# commit with patches/, and packs them as
# OUT/RustyMCLA-game-linux-<arch>.tar.gz, plus the patched source as
# OUT/RustyMCLA-source.tar.gz (the GPL's corresponding source). The release
# workflow runs it on an ARM64 runner.
#
#   scripts/build-game.sh [OUT]
#
# Environment: SRC (default ./src/rusty-mcla), CARGO_TARGET_DIR as usual.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$repo/out}"
src="${SRC:-$repo/src/rusty-mcla}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
arch="$(uname -m)"
case "$arch" in aarch64|arm64) arch=arm64 ;; x86_64|amd64) arch=x86_64 ;; esac

bash "$repo/scripts/prepare-source.sh" "$src"
target="${CARGO_TARGET_DIR:-$src/mcla-rs/target}"
(cd "$src/mcla-rs" && CARGO_TARGET_DIR="$target" cargo build --release --locked -p mcla -p mcla-tool)
cargo build --release --locked --manifest-path "$repo/tools/steam-art/Cargo.toml"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
game="$stage/RustyMCLA-game"
mkdir -p "$game/bin"
cp "$target/release/mcla" "$target/release/mcla-tool" "$game/bin/"
strip "$game/bin/mcla" "$game/bin/mcla-tool" 2>/dev/null || true
cp "$repo/tools/steam-art/target/release/steam-art" "$game/bin/"
mkdir -p "$game/licenses"
cp "$repo/tools/steam-art/fonts/OFL.txt" "$game/licenses/Anton-OFL.txt"
cp "$src/LICENSE" "$src/THIRD-PARTY-NOTICES.md" "$game/"
version="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$repo/upstream.json")"
{
    echo "upstream=$version"
    echo "commit=$(git -C "$src" rev-parse HEAD)"
    echo "overlay=$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$game/VERSION"
tar -C "$stage" --owner=0 --group=0 -czf "$out/RustyMCLA-game-linux-$arch.tar.gz" RustyMCLA-game
git -C "$src" archive --format=tar.gz --prefix=rusty-mcla/ -o "$out/RustyMCLA-source.tar.gz" HEAD
echo "$out/RustyMCLA-game-linux-$arch.tar.gz"
