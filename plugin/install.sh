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

GUARD_MAX_LOAD=${GUARD_MAX_LOAD:-80} ./guarded-cargo.sh xtask bundle synthexis --release

codesign --force --deep -s - target/bundled/synthexis.vst3
codesign --force --deep -s - target/bundled/synthexis.clap
codesign --force --deep -s - target/bundled/synthexis.app

rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/synthexis.vst3"
rm -rf "$HOME/Library/Audio/Plug-Ins/CLAP/synthexis.clap"
cp -R target/bundled/synthexis.vst3 "$HOME/Library/Audio/Plug-Ins/VST3/"
cp -R target/bundled/synthexis.clap "$HOME/Library/Audio/Plug-Ins/CLAP/"
echo "installed synthexis.vst3 + synthexis.clap"

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
for spec in "seg:seg_mnv4s_384x216_pm.mlpackage" "hand:hand_mnv4s_128.mlpackage" "face:"; do
    name=${spec%%:*}
    src=$(pick "$name" "${spec#*:}")
    if [ -z "$src" ]; then
        echo "perception: no $name model"
        continue
    fi
    tmp=$(mktemp -d)
    xcrun coremlcompiler compile "$src" "$tmp" >/dev/null
    rm -rf "${MODELS:?}/${name:?}.mlmodelc"
    mv "$tmp"/*.mlmodelc "$MODELS/$name.mlmodelc"
    rmdir "$tmp"
    echo "perception: $name <- $src"
done
# hybrid by default: the students for hands and seg, Apple Vision face
# landmarks on the seg face box (probes/plugin-perception). Others:
# SYNTHEXIS_PERCEPTION=students (fastest, weak face), vision, off.
BACKEND=${SYNTHEXIS_PERCEPTION:-hybrid}
echo "$BACKEND" > "$MODELS/backend"
echo "perception backend: $BACKEND"
