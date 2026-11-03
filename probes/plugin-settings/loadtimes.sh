#!/bin/bash
# Load-time probe (RESULT.md, section 2): the plugin binary's
# SYNTHEXIS_LOADPROBE mode in fresh processes, N runs warm and N cold.
#   probes/plugin-settings/loadtimes.sh <synthexis binary> <label> [N]
# warm: the installed models, loaded once before the runs;
# cold: a fresh copy of them per run, as after install.sh compiled them.
# Appends one JSON line per run to probes/plugin-settings/loadtimes.jsonl.
set -euo pipefail
BIN=$1; LABEL=$2; N=${3:-5}
MODELS="$HOME/Library/Application Support/synthexis/models"
OUT="$(dirname "$0")/loadtimes.jsonl"
run() { # cache run [env...]
    local c=$1 i=$2; shift 2
    local j; j=$(env SYNTHEXIS_LOADPROBE=1 "$@" "$BIN")
    echo "{\"label\":\"$LABEL\",\"cache\":\"$c\",\"run\":$i,${j#\{}" | tee -a "$OUT"
}
SYNTHEXIS_LOADPROBE=1 "$BIN" >/dev/null
for i in $(seq "$N"); do run warm "$i"; done
for i in $(seq "$N"); do
    tmp=$(mktemp -d); cp -R "$MODELS"/. "$tmp"/
    run cold "$i" SYNTHEXIS_MODEL_DIR="$tmp"
    rm -rf "$tmp"
done
