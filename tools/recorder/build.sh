#!/usr/bin/env bash
# Builds, bundles, signs and self-tests "synthexis recorder.app".
# Usage: tools/recorder/build.sh   (SYNTHEXIS_SIGN_ID=<identity> for a stable signature)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP="$HERE/synthexis recorder.app"
VERSION="$(sed -n 's/^let APP_VERSION = "\(.*\)"/\1/p' "$HERE/recorder.swift")"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
swiftc -O -parse-as-library "$HERE/recorder.swift" -o "$APP/Contents/MacOS/recorder"
cp "$ROOT/spec/recording-protocol.json" "$APP/Contents/Resources/recording-protocol.json"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>recorder</string>
  <key>CFBundleIdentifier</key><string>com.synthexis.recorder</string>
  <key>CFBundleName</key><string>synthexis recorder</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSCameraUsageDescription</key><string>The recorder captures training video of your hands and face.</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign "${SYNTHEXIS_SIGN_ID:--}" --identifier com.synthexis.recorder "$APP"
codesign --verify --strict "$APP"

# Recordings live in ~/Movies (not guarded by macOS folder permissions);
# data/recordings in the repo points there so the label pipeline finds them.
REC="$HOME/Movies/synthexis/recordings"
mkdir -p "$REC"
LINK="$ROOT/data/recordings"
if [ -d "$LINK" ] && [ ! -L "$LINK" ]; then
  if [ -z "$(ls -A "$LINK")" ]; then rmdir "$LINK"; else
    echo "warning: $LINK is a non-empty folder; move its sessions to $REC" >&2; fi
fi
[ -e "$LINK" ] || ln -s "$REC" "$LINK"

"$APP/Contents/MacOS/recorder" --selftest
echo "built $APP ($VERSION)"
