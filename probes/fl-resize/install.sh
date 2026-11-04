#!/bin/bash
# Builds (guarded), ad-hoc signs and installs fl-resize-probe.vst3/.clap into
# the user's plug-in folders. Refuses while FL Studio is running: replacing a
# loaded signed dylib can kill the host.
set -euo pipefail
cd "$(dirname "$0")"

if pgrep -x OsxFL >/dev/null; then
    echo "FL Studio (OsxFL) is running, refusing to install."
    echo "Quit FL, then re-run: $0"
    exit 1
fi

GUARD_MAX_LOAD=${GUARD_MAX_LOAD:-80} ./guarded-cargo.sh xtask bundle fl-resize-probe --release

codesign --force --deep -s - target/bundled/fl-resize-probe.vst3
codesign --force --deep -s - target/bundled/fl-resize-probe.clap
codesign --force --deep -s - target/bundled/fl-resize-probe.app
rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/fl-resize-probe.vst3"
rm -rf "$HOME/Library/Audio/Plug-Ins/CLAP/fl-resize-probe.clap"
cp -R target/bundled/fl-resize-probe.vst3 "$HOME/Library/Audio/Plug-Ins/VST3/"
cp -R target/bundled/fl-resize-probe.clap "$HOME/Library/Audio/Plug-Ins/CLAP/"

echo "installed fl-resize-probe VST3 + CLAP"
