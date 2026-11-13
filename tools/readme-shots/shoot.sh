#!/bin/bash
# Re-shoots the README screenshots in docs/img from the standalone.
#
#   cd plugin && ./guarded-cargo.sh build -p synthexis --bin synthexis \
#     --config 'profile.dev.package.baseview.debug-assertions=false'
#   tools/readme-shots/shoot.sh   # DST=<dir> writes elsewhere than docs/img
#
# Each state is a SYNTHEXIS_UI_SHOT hook in plugin/synthexis/src/editor.rs
# (`apply_shot`): it draws the stand-in camera picture instead of the real
# camera. Only the synthexis window is captured (`screencapture -l`), never
# the full screen. Every standalone is quit after its capture.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
BIN=$("$REPO/plugin/target-dir.sh")/debug/synthexis
OUT=${OUT:-$(mktemp -d)}
DST=${DST:-$REPO/docs/img}
mkdir -p "$OUT/env/parts"
WINID=$OUT/winid
swiftc -O -o "$WINID" "$HERE/winid.swift"
echo '{"lang":"en","completion":true}' > "$OUT/env/settings.json"

# shoot <state> <name>: start the standalone in that state, capture its window, quit it
shoot() {
  SYNTHEXIS_SETTINGS=$OUT/env/settings.json SYNTHEXIS_PARTS=$OUT/env/parts \
    SYNTHEXIS_UI_SHOT=$1 SYNTHEXIS_FL_PORTS="synthexis-shot-$1-$$" \
    "$BIN" -b dummy > "$OUT/$2.log" 2>&1 &
  local pid=$! id=""
  for _ in $(seq 40); do
    sleep 0.5
    id=$("$WINID" $pid | awk '$6==0 {print $1; exit}')
    [ -n "$id" ] && break
  done
  sleep 6 # let the projectiles and trails of state v fly
  [ -n "$id" ] && screencapture -x -o -l "$id" "$OUT/$2.png" && echo "shot $2"
  kill $pid 2>/dev/null || true
  sleep 1
  kill -9 $pid 2>/dev/null || true
  sleep 2 # let CoreMIDI drop the ports before the next start
}

shoot v hero   # Pong with three projectiles and their trails
shoot b hands  # Hands section, dither hands
shoot u routes # Hands routes on Serum 2, target search open
shoot o head   # Head section with a kept mouth route, face tracking boxes

python3 "$HERE/resize.py" "$OUT" "$DST"
