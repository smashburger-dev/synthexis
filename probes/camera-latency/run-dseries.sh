#!/bin/bash
set -u
cd "$(dirname "$0")"
OUT=/Users/no8/Desktop/life/Lifemaxxing/synthexis/probes/camera-latency/out
run() {
  local label="$1" secs="$2"; shift 2
  echo "=== $label ($(sysctl -n vm.loadavg))"
  open -W -n CamLat.app --args --label "$label" --seconds "$secs" --device builtin --mode preset720 --fps auto --pix bgra --qos default --flash 0 --out "$OUT" "$@"
}
run D1-baseline 120
run D2-busyafter20 60 --inject-mode busy-after --inject-ms 20
run D3-sleep20 60 --inject-mode sleep --inject-ms 20
run D4a-busyafter2 60 --inject-mode busy-after --inject-ms 2
run D4b-busyafter5 60 --inject-mode busy-after --inject-ms 5
run D4c-busyafter10 60 --inject-mode busy-after --inject-ms 10
run D5-spinner20 60 --spinner-pct 20
run D6a-hold20 60 --hold-ms 20
run D6b-hold60 60 --hold-ms 60
echo "=== series done"
