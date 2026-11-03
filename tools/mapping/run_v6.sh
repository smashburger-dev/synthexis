#!/bin/zsh
# Parser v6: v5 recipe plus the stage-5 style batch (long both-ways and chained
# sentences, pinch held while the hand moves; probes/real-eval/RESULT.md).
#   caffeinate -dimsu zsh tools/mapping/run_v6.sh
# Leak filter on cmd-s5, ds-v6 = ds-v5 + cmd-s5 (dev_s5), projected to ds-v6p
# (no --drop-toggle, see RESULT.md), two seeds, gates against v5 and v3.
# Log: data/mapping/train/v6.log
set -u
cd "${0:A:h}/../.."
export UV_OFFLINE=1 PYTHONUNBUFFERED=1
STEPS=${STEPS:-16000}
LOG=data/mapping/train/v6.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

step uv run probes/real-eval/leak.py --runs cmd-s5
step uv run tools/mapping/dataset.py --out ds-v6 --stage2 cmd-s2,kw-s2 \
  --stage3 cmd-s3,kw-s3 --stage4 cmd-s4b --stage5 cmd-s5 \
  --exclude cmd-s4b,cmd-s5 --cat 0.5
step uv run tools/mapping/project_labels.py --src ds-v6 --out ds-v6p
for seed in 0 1; do
  step uv run tools/mapping/train_parser.py --name p-v6-s$seed --ds ds-v6p \
    --tok tok-v2 --train train_cat --seed $seed --steps $STEPS --whash 0
done
step uv run tools/mapping/gates.py --base p-v5-s0,p-v5-s1 --cand p-v6-s0,p-v6-s1
step uv run tools/mapping/gates.py --base p-v3-c50-s0,p-v3-c50-s1 --cand p-v6-s0,p-v6-s1
print "[$(date '+%F %T')] ALL DONE" >> $LOG
