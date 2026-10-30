#!/bin/zsh
# Stage-2 training on this Mac (ds-v2 = stage 1 + weighted stage 2).
#   caffeinate -dimsu zsh tools/mapping/run_s2.sh
# Order: all sentences vs all plus concatenations (same steps), then seed 1
# and 1.5x steps of the better set, export of the best run, REPORT.md.
# dev.jsonl is the stage-1 dev set, so dev and challenge compare with v1.
# A failed step is logged and the queue goes on.
# Log: data/mapping/train/s2.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/s2.log
mkdir -p data/mapping/train

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}
parser() { step uv run tools/mapping/train_parser.py --ds ds-v2 "$@"; }

parser --name p-v2-all-s0 --train train_all --steps $STEPS
parser --name p-v2-cat-s0 --train train_cat --steps $STEPS
BEST=$(uv run tools/mapping/report.py --pick p-v2-all-s0 p-v2-cat-s0)
TAG=all
[[ $BEST == p-v2-cat-s0 ]] && TAG=cat
print "[$(date '+%F %T')] better: $BEST" >> $LOG
step uv run tools/mapping/report.py --main $BEST
parser --name p-v2-$TAG-s1 --train train_$TAG --seed 1 --steps $STEPS
parser --name p-v2-$TAG-s0-24k --train train_$TAG --steps $((STEPS * 3 / 2))
TOP=$(uv run tools/mapping/report.py --pick $BEST p-v2-$TAG-s0-24k)
print "[$(date '+%F %T')] export: $TOP" >> $LOG
step uv run tools/mapping/export_parser.py --run $TOP --ds ds-v2 --no-audit
step uv run tools/mapping/report.py --main $TOP
print "[$(date '+%F %T')] ALL DONE" >> $LOG
