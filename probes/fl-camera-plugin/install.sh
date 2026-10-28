#!/bin/bash
# Installs the staged probe bundles. Refuses while FL Studio is running:
# replacing a loaded signed dylib can kill the host.
set -euo pipefail
cd "$(dirname "$0")"

if pgrep -x OsxFL >/dev/null; then
    echo "FL Studio (OsxFL) is running — refusing to install."
    echo "Quit FL, then re-run: $0"
    exit 1
fi

codesign --force --deep -s - target/bundled/synthexis-probe.vst3
codesign --force --deep -s - target/bundled/synthexis-probe.clap
rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/synthexis-probe.vst3"
rm -rf "$HOME/Library/Audio/Plug-Ins/CLAP/synthexis-probe.clap"
cp -R target/bundled/synthexis-probe.vst3 "$HOME/Library/Audio/Plug-Ins/VST3/"
cp -R target/bundled/synthexis-probe.clap "$HOME/Library/Audio/Plug-Ins/CLAP/"

cp target/release/synthexis-probe synthexis-probe-standalone.app/Contents/MacOS/synthexis-probe
codesign --force --deep -s - synthexis-probe-standalone.app

echo "installed VST3 + CLAP + standalone"
