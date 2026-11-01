#!/usr/bin/env bash
# Moves patches/ onto a newer Rusty MCLA release: rebases them from the commit
# upstream.json pins onto TAG, writes them back to patches/ and records TAG in
# upstream.json (its commit, its version and the Rust version upstream's own CI
# builds it with). A patch that no longer applies stops it with an error that
# names the patch; nothing in the repository is changed then.
#
#   scripts/rebase-upstream.sh TAG [WORKDIR]   (TAG such as v0.9.6)
set -euo pipefail

tag="$1"
repo="$(cd "$(dirname "$0")/.." && pwd)"
work="${2:-$(mktemp -d)}"
field() { python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo/upstream.json" "$1"; }
url="$(field repository)"
old="$(field commit)"
git_() { git -C "$work/rusty-mcla" -c user.name=overlay -c user.email=overlay@localhost -c advice.detachedHead=false "$@"; }

rm -rf "$work/rusty-mcla"
git clone -q --filter=blob:none "$url" "$work/rusty-mcla"
new="$(git_ rev-parse "$tag^{commit}")"
git_ checkout -q -b port "$old"
shopt -s nullglob
patches=("$repo"/patches/*.patch)
[ ${#patches[@]} -gt 0 ] && git_ am -q --keep-cr "${patches[@]}"

if ! git_ rebase -q "$new" > "$work/rebase.log" 2>&1; then
    cat "$work/rebase.log" >&2
    echo "::error::The patch \"$(git_ log -1 --format=%s REBASE_HEAD 2>/dev/null || echo '?')\" does not apply to Rusty MCLA $tag (conflicts in: $(git_ diff --name-only --diff-filter=U | tr '\n' ' '))" >&2
    git_ rebase --abort || true
    exit 1
fi

rm -f "$repo"/patches/*.patch
git_ format-patch -q -o "$repo/patches" "$new..port"
rust="$(git_ show "$new:.github/workflows/ci.yml" 2>/dev/null | sed -n 's/^ *RUST_VERSION: *"\{0,1\}\([0-9.]*\)"\{0,1\}.*/\1/p' | head -1)"
python3 - "$repo/upstream.json" "$new" "${tag#v}" "$rust" <<'PY'
import datetime, json, sys
path, commit, version, rust = sys.argv[1:]
d = json.load(open(path))
d.update(commit=commit, version=version, verified_utc=datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
if rust:
    d["rust"] = rust
open(path, "w").write(json.dumps(d, indent=2) + "\n")
PY
n=(patches/*.patch); cd "$repo"; n=(patches/*.patch)
echo "Rusty MCLA $tag (${new:0:7}) with ${#n[@]} patches, Rust $(field rust)"
