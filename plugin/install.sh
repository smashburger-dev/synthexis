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
