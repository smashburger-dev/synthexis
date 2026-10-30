#!/bin/zsh
# Concatenation-rate sweep on this Mac (after run_s2.sh).
#   caffeinate -dimsu zsh tools/mapping/run_cat.sh
# ds-v2-c50 / ds-v2-c100 = ds-v2 with --cat 0.5 / 1.0; every set except
# train_cat is byte-identical to ds-v2, and the c50 train_cat starts with
# the ds-v2 one. Two seeds per rate, compared with p-v2-cat-s0/s1 (0.25).
# A failed step is logged and the queue goes on.
# Log: data/mapping/train/cat.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/cat.log
mkdir -p data/mapping/train

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

for seed in 0 1; do
  for c in 50 100; do
    step uv run tools/mapping/train_parser.py --ds ds-v2-c$c \
      --name p-v2-c$c-s$seed --train train_cat --seed $seed --steps $STEPS
  done
done
step uv run tools/mapping/report.py
print "[$(date '+%F %T')] ALL DONE" >> $LOG
