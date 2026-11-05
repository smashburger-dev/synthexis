#!/bin/bash
# Installs the synthexis release on this Mac: the VST3 and CLAP plug-ins,
# the perception models, the FL Studio device script, and the sentence
# parser (downloaded once from the GitHub release, about 450 MB).
# Run it with FL Studio closed. Double-click, or: bash install.command
set -euo pipefail
cd "$(dirname "$0")"
if pgrep -x OsxFL >/dev/null; then
    echo "FL Studio is running. Quit it, then run this again."
    exit 1
fi
# Downloaded files carry Apple's quarantine flag; the ad-hoc signed
# plug-ins would not load with it.
xattr -dr com.apple.quarantine . 2>/dev/null || true

VST3="$HOME/Library/Audio/Plug-Ins/VST3"
CLAP="$HOME/Library/Audio/Plug-Ins/CLAP"
APP="$HOME/Library/Application Support/synthexis"
mkdir -p "$VST3" "$CLAP" "$APP/models"
rm -rf "$VST3/synthexis.vst3" "$CLAP/synthexis.clap"
cp -R synthexis.vst3 "$VST3/"
cp -R synthexis.clap "$CLAP/"
echo "installed synthexis.vst3 and synthexis.clap"

for name in seg hand face; do
    rm -rf "$APP/models/$name.mlmodelc" "$APP/models/$name.mlpackage" "$APP/models/$name.source-sha256"
    tmp=$(mktemp -d)
    if xcrun coremlcompiler compile "models/$name.mlpackage" "$tmp" >/dev/null 2>&1; then
        mv "$tmp"/*.mlmodelc "$APP/models/$name.mlmodelc"
    else
        # without Xcode tools the plug-in compiles the model when it loads
        cp -R "models/$name.mlpackage" "$APP/models/"
    fi
    rm -rf "$tmp"
done
echo hybrid > "$APP/models/backend"
echo "installed the perception models"

FL_HW="$HOME/Documents/Image-Line/FL Studio/Settings/Hardware"
if [ -d "$FL_HW" ]; then
    mkdir -p "$FL_HW/synthexis bridge"
    cp fl/device_synthexis_bridge.py "$FL_HW/synthexis bridge/"
    echo "installed the FL Studio device script"
else
    echo "no FL Studio settings folder found: copy fl/device_synthexis_bridge.py into"
    echo "  Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/"
fi

SYNTHEXIS_ASSISTANT=release SYNTHEXIS_DATA=/nonexistent ./assistant/install.sh \
    || echo "sentence parser not installed (no internet?). Run install.command again later."
echo
echo "Done. Start FL Studio and add synthexis from the plug-in list."
