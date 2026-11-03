#!/bin/zsh
# Bundled retrain on this Mac, no new Spark data (probes/parser-errors/RESULT.md section 6).
#   caffeinate -dimsu zsh tools/mapping/run_v4.sh
# A = v3 recipe with tok-v2 (ß gets a piece), B = A on ds-v4p (labels projected
# onto what the text says), C = B + word hash embedding. Seed 0 of all three
# first, then seed 1. Gates: A vs v3 on the original labels, B vs A and C vs B
# on both label sets (tools/mapping/gates.py).
# A failed step is logged and the queue goes on. Log: data/mapping/train/v4.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/v4.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

train() {   # name ds whash seed
  step uv run tools/mapping/train_parser.py --name p-v4$1-s$4 --ds $2 \
    --tok tok-v2 --train train_cat --seed $4 --steps $STEPS --whash $3
}

for seed in 0 1; do
  train a ds-v3 0 $seed
  train b ds-v4p 0 $seed
  train c ds-v4p 1 $seed
done
G="uv run tools/mapping/gates.py"
step ${=G} --base p-v3-c50-s0,p-v3-c50-s1 --cand p-v4a-s0,p-v4a-s1
step ${=G} --base p-v4a-s0,p-v4a-s1 --cand p-v4b-s0,p-v4b-s1
step ${=G} --base p-v4b-s0,p-v4b-s1 --cand p-v4c-s0,p-v4c-s1
step ${=G} --base p-v3-c50-s0,p-v3-c50-s1 --cand p-v4c-s0,p-v4c-s1
print "[$(date '+%F %T')] ALL DONE" >> $LOG
