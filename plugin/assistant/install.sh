#!/bin/bash
# Install the mapping assistant's runtime files where the plugin reads them
# (synthexis/src/assistant.rs and sentence.rs):
#   ~/Library/Application Support/synthexis/parser      parser ensemble
#   ~/Library/Application Support/synthexis/completion  completion data
#
# Dev checkout: data/mapping/runtime and data/mapping/completion exist, so
# both dirs become symlinks to them (no copy, exports show up at once).
# Fresh clone: completion is copied from plugin/assistant/completion; the
# parser (446 MB) is downloaded from the GitHub release pinned in
# parser.release, checked against its sha256 and swapped in atomically.
# Re-running is cheap: an installed parser with the pinned sha is kept.
#
# Env: SYNTHEXIS_ASSISTANT=release forces the fresh-clone path,
#      SYNTHEXIS_APP_DIR overrides the target dir (tests),
#      SYNTHEXIS_DATA overrides the data dir.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=parser.release
. "$here/parser.release"
# -P: a worktree's data/ is often a symlink; link to the real dir instead
DATA="${SYNTHEXIS_DATA:-$(cd -P "$here/../../data" 2>/dev/null && pwd || echo /nonexistent)}"
APP="${SYNTHEXIS_APP_DIR:-$HOME/Library/Application Support/synthexis}"
MODE="${SYNTHEXIS_ASSISTANT:-auto}"
mkdir -p "$APP"

tmp=""
cleanup() { [ -n "$tmp" ] && rm -rf "$tmp"; return 0; }
trap cleanup EXIT

# Replace $2 by $1 (a finished dir) in two renames; the old one goes last.
swap_in() {
    rm -rf "${2:?}.old"
    if [ -e "$2" ] || [ -L "$2" ]; then mv "$2" "$2.old"; fi
    mv "$1" "$2"
    rm -rf "${2:?}.old"
}

link() { # src dest
    if [ -L "$2" ] && [ "$(readlink "$2")" = "$1" ]; then return; fi
    rm -rf "${2:?}"
    ln -s "$1" "$2"
}

free_kb() { df -Pk "$APP" | awk 'NR==2 {print $4}'; }

fetch_parser() { # archive path
    local part="$1.part" url
    url="https://github.com/$REPO/releases/download/$TAG/$ASSET"
    # gh works for a private repo too; curl resumes a broken download.
    if ! { command -v gh >/dev/null && gh auth status >/dev/null 2>&1 &&
           gh release download "$TAG" -R "$REPO" -p "$ASSET" -O "$part" \
              --clobber 2>/dev/null; }; then
        curl -fsSL --retry 3 --retry-delay 2 -C - -o "$part" "$url" || {
            echo "assistant: download failed: $url" >&2
            echo "assistant: private repo? run 'gh auth login' and retry" >&2
            return 1; }
    fi
    if [ "$(shasum -a 256 "$part" | cut -d' ' -f1)" != "$SHA256" ]; then
        rm -f "$part"
        echo "assistant: sha256 mismatch for $ASSET, removed it" >&2
        return 1
    fi
    mv "$part" "$1"
}

install_parser() {
    local dest="$APP/parser" archive="$APP/.$ASSET" need run
    if [ ! -L "$dest" ] && [ "$(cat "$dest/.release" 2>/dev/null)" = "$SHA256" ]; then
        echo "assistant: parser $TAG already installed"
        return
    fi
    # archive + extracted copy, with 10 % slack
    need=$(( (BYTES * 2 + BYTES / 10) / 1024 ))
    if [ -f "$archive" ]; then need=$(( (BYTES + BYTES / 10) / 1024 )); fi
    if [ "$(free_kb)" -lt "$need" ]; then
        echo "assistant: need $((need / 1024)) MB free in $APP, have $(( $(free_kb) / 1024 )) MB" >&2
        return 1
    fi
    [ -f "$archive" ] || fetch_parser "$archive"
    tmp=$(mktemp -d "$APP/.parser.XXXXXX")
    tar -xzf "$archive" -C "$tmp"
    local files="config.json tokenizer.json" f
    for run in $RUNS; do files="$files $run/model.safetensors $run/parser.json"; done
    for f in $files; do
        [ -s "$tmp/$f" ] || { echo "assistant: $ASSET lacks $f" >&2; return 1; }
    done
    echo "$SHA256" > "$tmp/.release"
    swap_in "$tmp" "$dest"
    tmp=""
    rm -f "$archive"
    echo "assistant: parser <- release $TAG"
}

install_completion() {
    tmp=$(mktemp -d "$APP/.completion.XXXXXX")
    cp -R "$here/completion/." "$tmp/"
    swap_in "$tmp" "$APP/completion"
    tmp=""
    echo "assistant: completion <- plugin/assistant/completion"
}

if [ "$MODE" != release ] && [ -d "$DATA/mapping/runtime" ] &&
   [ -d "$DATA/mapping/completion" ]; then
    link "$DATA/mapping/runtime" "$APP/parser"
    link "$DATA/mapping/completion" "$APP/completion"
    echo "assistant: parser, completion -> $DATA/mapping (dev)"
else
    install_completion
    install_parser
fi
