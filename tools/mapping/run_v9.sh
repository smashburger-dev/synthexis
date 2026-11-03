#!/bin/zsh
# Parser v9: v7a recipe on ds-v9c = stages 1 to 5 + stage 6 (DeepSeek, Noa's conventions),
# stage-6 training rows twice in train_cat, labels passed through conventions.py.
#   zsh tools/mapping/run_v9.sh        (after commands.py --run cmd-s6 has finished)
# Log: data/mapping/train/v9.log. Training runs on Modal; fetch the runs afterwards with
#   modal volume get synthexis-parser /train/runs/p-v9-s0 data/mapping/train/runs/
set -eu
cd "${0:A:h}/../.."
export PYTHONUNBUFFERED=1
LOG=data/mapping/train/v9.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

step uv run probes/real-eval/leak.py --runs cmd-s6
step uv run tools/mapping/dataset.py --out ds-v9 --stage2 cmd-s2,kw-s2 \
  --stage3 cmd-s3,kw-s3 --stage4 cmd-s4b --stage5 cmd-s5 --stage6 cmd-s6 \
  --exclude cmd-s4b,cmd-s5,cmd-s6 --cat 0.5
step uv run tools/mapping/project_labels.py --src ds-v9 --out ds-v9p
step uv run tools/mapping/conventions.py --src ds-v9p --out ds-v9c
# stage-6 rows (edit ids q...) a second time in train_cat
step python3 -c "
import json, random
p = 'data/mapping/train/ds-v9c/train_cat.jsonl'
rows = [json.loads(l) for l in open(p)]
extra = [r for r in rows if r['edit_id'].startswith('q')]
rows += extra
random.Random(0).shuffle(rows)
open(p, 'w').write(''.join(json.dumps(r, ensure_ascii=False) + '\n' for r in rows))
print('stage-6 rows added again:', len(extra))
"
step uvx modal volume put --force synthexis-parser data/mapping/train/ds-v9c /train/ds-v9c
R="--ds ds-v9c --train train_cat --steps 10000 --backbone mmbert-small-de-en-32k --enc-lr 5e-5 --heads 6 --ff 1536 --amp 1"
step uv run --with modal python tools/mapping/modal_parser.py spawn \
  p-v9-s0 "$R --seed 0" p-v9-s1 "$R --seed 1"
print "[$(date '+%F %T')] ALL DONE" >> $LOG
