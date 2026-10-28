#!/bin/bash
# Builds CamLat.app (ad-hoc signed) from main.swift.
set -euo pipefail
cd "$(dirname "$0")"
xcrun swiftc -O -parse-as-library main.swift -o CamLat
mkdir -p CamLat.app/Contents/MacOS
mv -f CamLat CamLat.app/Contents/MacOS/CamLat
cp Info.plist CamLat.app/Contents/Info.plist
codesign --force --deep -s - CamLat.app
echo "built CamLat.app"
