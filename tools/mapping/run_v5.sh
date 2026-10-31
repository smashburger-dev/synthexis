#!/bin/zsh
# Final parser runs with the stage-4 data (ontology v1.1 + v1.2 demo requests).
#   DS=ds-v5p WHASH=0 caffeinate -dimsu zsh tools/mapping/run_v5.sh
# DS and WHASH follow the winner of run_v4.sh (ds-v5 = ds-v3 + cmd-s4b minus
# near-copies of the real-sentence set; ds-v5p = projected). Two seeds, then
# gates against v3 on dev, dev_s2, dev_s3. Log: data/mapping/train/v5.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
DS=${DS:-ds-v5p}
WHASH=${WHASH:-0}
LOG=data/mapping/train/v5.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

for seed in 0 1; do
  step uv run tools/mapping/train_parser.py --name p-v5-s$seed --ds $DS \
    --tok tok-v2 --train train_cat --seed $seed --steps $STEPS --whash $WHASH
done
step uv run tools/mapping/gates.py --base p-v3-c50-s0,p-v3-c50-s1 --cand p-v5-s0,p-v5-s1
print "[$(date '+%F %T')] ALL DONE" >> $LOG
