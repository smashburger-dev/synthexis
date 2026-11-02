#!/bin/zsh
# Overnight mapping training on this Mac (see docs/mapping-train-session-prompt.md).
#   caffeinate -dimsu zsh tools/mapping/run_overnight.sh
# Order: param-sense, filter ablation (filtered vs all, same steps), learning
# curve 50/25 % and seeds 1/2 of the better set, export/latency/audit of the
# main run, a 2x-steps run of the better set, REPORT.md after each stage.
# A failed step is logged and the queue goes on.
# Log: data/mapping/train/overnight.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/overnight.log
mkdir -p data/mapping/train

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}
parser() { step uv run tools/mapping/train_parser.py --steps $STEPS "$@"; }

step uv run tools/mapping/train_paramsense.py --name ps-v1 --epochs 20
parser --name p-filt-100-s0 --train train_filtered
parser --name p-all-100-s0 --train train_all
BETTER=$(uv run tools/mapping/report.py --pick-better)
TAG=all
[[ $BETTER == train_filtered ]] && TAG=filt
print "[$(date '+%F %T')] better set: $BETTER" >> $LOG
step uv run tools/mapping/report.py
parser --name p-$TAG-50-s0 --train $BETTER --frac 0.5
parser --name p-$TAG-25-s0 --train $BETTER --frac 0.25
parser --name p-$TAG-100-s1 --train $BETTER --seed 1
parser --name p-$TAG-100-s2 --train $BETTER --seed 2
step uv run tools/mapping/export_parser.py --run p-$TAG-100-s0
step uv run tools/mapping/report.py
STEPS=$((STEPS * 2)) parser --name p-$TAG-100-s0-long --train $BETTER
step uv run tools/mapping/report.py
print "[$(date '+%F %T')] ALL DONE" >> $LOG
