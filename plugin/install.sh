#!/bin/bash
# Build (guarded), ad-hoc sign, install synthexis.vst3/.clap.
# Refuses only if FL Studio is running AND the synthexis bundle is already
# installed (replacing a loaded dylib can kill the host; first install is
# safe because FL has never loaded it).
set -euo pipefail
cd "$(dirname "$0")"

INSTALLED=0
[ -d "$HOME/Library/Audio/Plug-Ins/VST3/synthexis.vst3" ] && INSTALLED=1
if pgrep -x OsxFL >/dev/null && [ "$INSTALLED" = 1 ]; then
    echo "FL Studio is running and synthexis is already installed — refusing."
    echo "Quit FL, then re-run: $0"
    exit 1
fi

# Building from source needs Apple's Command Line Tools (clang and the
# linker Rust uses) and Rust (cargo). A missing one is offered through its
# official installer and installed only after a yes.
ask() { # prompt -> true on yes; false without a terminal
    local a
    [ -t 0 ] || return 1
    read -r -p "$1 [y/N] " a
    [[ "$a" =~ ^[YyJj] ]]
}
if ! xcode-select -p >/dev/null 2>&1; then
    echo "Apple's Command Line Tools are missing: Rust links with their clang and linker."
    echo "Apple's own installer is 'xcode-select --install' (a macOS dialog, no Apple ID)."
    if ask "Open Apple's installer now?"; then
        xcode-select --install || true
        echo "Re-run $0 when the installation has finished."
    fi
    exit 1
fi
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust is missing. The official installer is rustup from https://rustup.rs:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "It installs into ~/.rustup and ~/.cargo for your user only (no sudo) and"
    echo "shows its own menu first, where you can still cancel."
    if ask "Run the official rustup installer now?"; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
        export PATH="$HOME/.cargo/bin:$PATH"
        command -v cargo >/dev/null 2>&1 || { echo "cargo still not found; re-run $0 in a new terminal."; exit 1; }
    else
        exit 1
    fi
fi

# The target dir is shared by all worktrees (target-dir.sh). Hold its lock
# from the build to the last copy, so no other worktree's bundle lands in
# between.
TARGET=$(./target-dir.sh)
if [ "$TARGET" != "$PWD/target" ]; then
    . ./target-lock.sh
    target_lock "$TARGET"
fi
GUARD_MAX_LOAD=${GUARD_MAX_LOAD:-80} ./guarded-cargo.sh xtask bundle synthexis --release
BUNDLED=$TARGET/bundled

codesign --force --deep -s - "$BUNDLED"/synthexis.vst3
codesign --force --deep -s - "$BUNDLED"/synthexis.clap
codesign --force --deep -s - "$BUNDLED"/synthexis.app

rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/synthexis.vst3"
rm -rf "$HOME/Library/Audio/Plug-Ins/CLAP/synthexis.clap"
cp -R "$BUNDLED"/synthexis.vst3 "$HOME/Library/Audio/Plug-Ins/VST3/"
cp -R "$BUNDLED"/synthexis.clap "$HOME/Library/Audio/Plug-Ins/CLAP/"
echo "installed synthexis.vst3 + synthexis.clap"

# FL device script. With it in place FL sets up the plugin's "synthexis"
# MIDI ports itself (fl/device_synthexis_bridge.py, header). FL reads the
# folder only at start, and a port name it once saw without a matching
# script stays without one, so FL has to restart before synthexis is
# inserted for the first time (probes/fl-midi-autolink/RESULT.md).
FL_HW="$HOME/Documents/Image-Line/FL Studio/Settings/Hardware"
if [ -d "$FL_HW" ]; then
    mkdir -p "$FL_HW/synthexis bridge"
    cp fl/device_synthexis_bridge.py "$FL_HW/synthexis bridge/"
    echo "installed the FL device script"
    if pgrep -x OsxFL >/dev/null; then
        echo "FL Studio is running: quit and restart it before inserting synthexis."
    fi
else
    echo "no FL Studio settings folder at $FL_HW: FL device script not installed (plugin/README.md)"
fi

# Perception students: data/models (train-students' working copies, a dev
# override), else the tracked copies in plugin/models (a fresh clone has
# no data/), else the preflight stand-ins; compiled for this Mac into the
# model dir the plugin reads (engine/perception.rs).
DATA="${SYNTHEXIS_DATA:-$(cd .. && pwd)/data}"
MODELS="$HOME/Library/Application Support/synthexis/models"
mkdir -p "$MODELS"
pick() { # name, stand-in file
    local m
    m=$(ls -d "$DATA/models/$1"*.mlpackage 2>/dev/null | head -1 || true)
    [ -z "$m" ] && [ -d "models/$1.mlpackage" ] && m="$(pwd)/models/$1.mlpackage"
    [ -z "$m" ] && [ -n "$2" ] && [ -d "$DATA/preflight/$2" ] && m="$DATA/preflight/$2"
    echo "$m"
}
# CoreML caches what it prepares for the Neural Engine per app and model
# path; a recompiled model at the same path (even an identical one) makes
# FL's next load cold, seconds per model (probes/plugin-settings). So a
# compiled model whose source did not change is kept, by a fingerprint of
# the source (coremlcompiler output differs on every run).
fingerprint() { (cd "$1" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256) | shasum -a 256 | cut -d' ' -f1; }
for spec in "seg:seg_mnv4s_384x216_pm.mlpackage" "hand:hand_mnv4s_128.mlpackage" "face:"; do
    name=${spec%%:*}
    src=$(pick "$name" "${spec#*:}")
    if [ -z "$src" ]; then
        echo "perception: no $name model"
        continue
    fi
    fp=$(fingerprint "$src")
    if [ -d "$MODELS/$name.mlmodelc" ] && [ "$(cat "$MODELS/$name.source-sha256" 2>/dev/null)" = "$fp" ]; then
        echo "perception: $name unchanged, kept"
        continue
    fi
    rm -rf "${MODELS:?}/${name:?}.mlmodelc" "${MODELS:?}/${name:?}.mlpackage" "${MODELS:?}/${name:?}.source-sha256"
    tmp=$(mktemp -d)
    if xcrun coremlcompiler compile "$src" "$tmp" >/dev/null 2>&1; then
        mv "$tmp"/*.mlmodelc "$MODELS/$name.mlmodelc"
        echo "$fp" > "$MODELS/$name.source-sha256"
        echo "perception: $name <- $src"
    else
        # The plugin compiles an .mlpackage itself each time it loads (a few s).
        cp -R "$src" "$MODELS/$name.mlpackage"
        echo "perception: $name <- $src (copied; the plugin compiles it when it loads)"
    fi
    rm -rf "$tmp"
done
# hybrid by default: the students for hands and seg, Apple Vision face
# landmarks on the seg face box (probes/plugin-perception). Others:
# SYNTHEXIS_PERCEPTION=students (fastest, weak face), vision, off.
BACKEND=${SYNTHEXIS_PERCEPTION:-hybrid}
echo "$BACKEND" > "$MODELS/backend"
echo "perception backend: $BACKEND"

# Mapping assistant runtime (parser and completion): plugin/assistant/install.sh
# links data/mapping in a dev checkout, else copies the tracked completion data
# and downloads the parser release. Not fatal, so camera and macros install offline.
./assistant/install.sh || echo "assistant: runtime not installed, the sentence field stays off (see plugin/assistant/README.md)"
