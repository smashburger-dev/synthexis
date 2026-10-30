#!/bin/zsh
# Stage-3 training on this Mac (after run_cat.sh).
#   caffeinate -dimsu zsh tools/mapping/run_s3.sh
# ds-v3 = ds-v2 plus the targeted cmd-s3/kw-s3 batch, --cat 0.5; dev,
# dev_s2 and challenge are byte-identical to ds-v2 and the stage-3 dev
# edits go to dev_s3. Two seeds, compared with p-v2-c50-s0/s1.
# A failed step is logged and the queue goes on.
# Log: data/mapping/train/s3.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/s3.log
mkdir -p data/mapping/train

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

for seed in 0 1; do
  step uv run tools/mapping/train_parser.py --ds ds-v3 \
    --name p-v3-c50-s$seed --train train_cat --seed $seed --steps $STEPS
done
step uv run tools/mapping/report.py
print "[$(date '+%F %T')] ALL DONE" >> $LOG
