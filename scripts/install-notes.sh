#!/usr/bin/env bash
# Prints the install section of a release's notes (after the "What's new"
# part from scripts/release-notes.sh). release.yml uses it for new releases,
# release-notes.yml when asked to rewrite an older release's section.
#
# Usage: scripts/install-notes.sh VERSION UPSTREAM LAUNCHER_SHA256
set -euo pipefail

version="$1" upstream="$2" sha256="$3"
cat <<NOTES
## Hitting the streets of LA

Launcher $version comes with [Rusty MCLA $upstream](https://github.com/holdmysocks/rusty-mcla), the open source Midnight Club: Los Angeles engine, already compiled for ARM64 (RustyMCLA-game-linux-arm64.tar.gz; the launcher downloads it for you).

1. In Desktop Mode, unpack RustyMCLA-Launcher-linux-arm64.tar.gz and start RustyMCLA/rusty-mcla-launcher.
2. Point it at your Xbox 360 ISO of Midnight Club: Los Angeles, Complete Edition, pick resolution, frame cap and render scale, and press **Install**. The game data is built from the disc: about 16 GB free, on the charger.
3. Play from Steam. The entry has no artwork: set it with SteamGridDB, and later updates keep it.

1280 x 720 capped at 60 fps is the default; Shadows are the first setting to lower if the city drops frames. New Rusty MCLA versions land here every week or so, and **Update** picks them up.

RustyMCLA-source.tar.gz is the exact source of this build (GPL-3.0). Launcher SHA-256: \`$sha256\`; no game data included.
NOTES
