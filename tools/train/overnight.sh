#!/bin/bash
# Overnight Mac chain (2026-11-03): seg MobileNetV3-Large + self-prev, then two
# more seeds of the hand-final recipe. Free (MPS). caffeinate keeps the Mac
# from idle sleep while it runs; each run restarts up to 4 times with --resume
# (the macOS GPU watchdog can kill a long MPS run), and counts as done when
# its summary.json exists. Log: data/train/overnight.log
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
run seg-v3l --student seg --epochs 10 --seed 0 --seg-backbone mobilenetv3_large_100 --self-prev 0.5 \
    --eval-prev sam --eval-batch 16 --workers 6
for sd in 1 2; do
    run hand-final-s$sd --student hand --epochs 20 --seed $sd --hand-backbone mobilenetv3_small_100 \
        --pseudo-fingers --workers 6
done
echo "$(date '+%F %T') chain done" >> "$LOG"
