#!/bin/bash
# Second overnight chain (2026-11-03): waits for overnight.sh, then two more
# seeds of the shipped hand recipe with the handedness output (hand-chir), for
# seed spread and a dev-chosen seed. Same restart rule and log as overnight.sh.
cd "$(dirname "$0")/../.." || exit 1
LOG=data/train/overnight.log

run() {  # name, args...
    local name=$1; shift
    for attempt in 1 2 3 4; do
        [ -f "data/train/runs/$name/summary.json" ] && return 0
        echo "$(date '+%F %T') start $name attempt $attempt" >> "$LOG"
        uv run tools/train/train.py --name "$name" --resume "$@" >> "$LOG" 2>&1
        echo "$(date '+%F %T') exit $name code $?" >> "$LOG"
    done
    [ -f "data/train/runs/$name/summary.json" ]
}

caffeinate -i -w $$ &
while pgrep -f "tools/train/overnight.sh" > /dev/null; do sleep 60; done
for sd in 1 2; do
    run hand-chir-s$sd --student hand --epochs 20 --seed $sd --hand-backbone mobilenetv3_small_100 \
        --pseudo-fingers --chirality --workers 6
done
echo "$(date '+%F %T') chain 2 done" >> "$LOG"
