#!/bin/bash
set -u
cd "$(dirname "$0")"
OUT=/Users/no8/Desktop/life/Lifemaxxing/synthexis/probes/camera-latency/out
run() {
  local label="$1"; shift
  echo "=== $label ($(sysctl -n vm.loadavg))"
  open -W -n CamLat.app --args --label "$label" --seconds 60 --device builtin --mode preset720 --fps auto --pix bgra --qos default --flash 0 --out "$OUT" "$@"
}
for i in 1 2 3; do
  run "R$i-base" 
  run "R$i-ba20" --inject-mode busy-after --inject-ms 20
  run "R$i-sleep20" --inject-mode sleep --inject-ms 20
  run "R$i-ba3" --inject-mode busy-after --inject-ms 3
done
echo "=== rseries done"
