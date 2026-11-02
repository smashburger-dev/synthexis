#!/bin/zsh
# Parser v11: v10 recipe on ds-v11c = stages 1 to 7 + stage 8 (instrumental frame, name-only
# targets, fan-out direction hints; DeepSeek) + the stage-6 and stage-7 side drops accepted
# again by the widened side check (cmd-s6r, cmd-s7r). Labels pass project_labels.py and
# conventions.py (now with the fist and eye fold); stage-6 to stage-8 rows twice in train_cat.
#   zsh tools/mapping/run_v11.sh        (after commands.py --run cmd-s8 has finished)
# Log: data/mapping/train/v11.log. Fetch the runs afterwards with
#   modal volume get synthexis-parser /train/runs/p-v11-s0 data/mapping/train/runs/
set -eu
cd "${0:A:h}/../.."
export PYTHONUNBUFFERED=1
LOG=data/mapping/train/v11.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

step uv run probes/real-eval/leak.py --runs cmd-s8,cmd-s6r,cmd-s7r
step uv run tools/mapping/dataset.py --out ds-v11 --stage2 cmd-s2,kw-s2 \
  --stage3 cmd-s3,kw-s3 --stage4 cmd-s4b --stage5 cmd-s5 --stage6 cmd-s6 --stage7 cmd-s7 \
  --stage8 cmd-s8,cmd-s6r,cmd-s7r \
  --exclude cmd-s4b,cmd-s5,cmd-s6,cmd-s7,cmd-s8,cmd-s6r,cmd-s7r --cat 0.5
step uv run tools/mapping/project_labels.py --src ds-v11 --out ds-v11p
step uv run tools/mapping/conventions.py --src ds-v11p --out ds-v11c
# stage-6 to stage-8 rows (edit ids q..., v..., w...) a second time in train_cat
step python3 -c "
import json, random
p = 'data/mapping/train/ds-v11c/train_cat.jsonl'
rows = [json.loads(l) for l in open(p)]
extra = [r for r in rows if r['edit_id'][0] in 'qvw']
rows += extra
random.Random(0).shuffle(rows)
open(p, 'w').write(''.join(json.dumps(r, ensure_ascii=False) + '\n' for r in rows))
print('stage-6 to stage-8 rows added again:', len(extra))
"
step uvx modal volume put --force synthexis-parser data/mapping/train/ds-v11c /train/ds-v11c
step uvx modal deploy tools/mapping/modal_parser.py
R="--ds ds-v11c --train train_cat --steps 10000 --backbone mmbert-small-de-en-32k --enc-lr 5e-5 --heads 6 --ff 1536 --amp 1"
step uv run --with modal python tools/mapping/modal_parser.py spawn \
  p-v11-s0 "$R --seed 0" p-v11-s1 "$R --seed 1" p-v11-s2 "$R --seed 2" p-v11-s3 "$R --seed 3"
print "[$(date '+%F %T')] ALL DONE" >> $LOG
