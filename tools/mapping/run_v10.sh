#!/bin/zsh
# Parser v10: v9 recipe on ds-v10c = stages 1 to 6 + stage 7 (direction phrasing, DeepSeek),
# stage-6 and stage-7 training rows twice in train_cat, labels passed through conventions.py.
# Four seeds: the selection rule compares the two- and the four-seed ensemble.
#   zsh tools/mapping/run_v10.sh        (after commands.py --run cmd-s7 has finished)
# Log: data/mapping/train/v10.log. Fetch the runs afterwards with
#   modal volume get synthexis-parser /train/runs/p-v10-s0 data/mapping/train/runs/
set -eu
cd "${0:A:h}/../.."
export PYTHONUNBUFFERED=1
LOG=data/mapping/train/v10.log

step() {
  print "[$(date '+%F %T')] START $*" >> $LOG
  "$@" >> $LOG 2>&1
  print "[$(date '+%F %T')] END rc=$? $*" >> $LOG
}

step uv run probes/real-eval/leak.py --runs cmd-s7
step uv run tools/mapping/dataset.py --out ds-v10 --stage2 cmd-s2,kw-s2 \
  --stage3 cmd-s3,kw-s3 --stage4 cmd-s4b --stage5 cmd-s5 --stage6 cmd-s6 --stage7 cmd-s7 \
  --exclude cmd-s4b,cmd-s5,cmd-s6,cmd-s7 --cat 0.5
step uv run tools/mapping/project_labels.py --src ds-v10 --out ds-v10p
step uv run tools/mapping/conventions.py --src ds-v10p --out ds-v10c
# stage-6 and stage-7 rows (edit ids q..., v...) a second time in train_cat
step python3 -c "
import json, random
p = 'data/mapping/train/ds-v10c/train_cat.jsonl'
rows = [json.loads(l) for l in open(p)]
extra = [r for r in rows if r['edit_id'][0] in 'qv']
rows += extra
random.Random(0).shuffle(rows)
open(p, 'w').write(''.join(json.dumps(r, ensure_ascii=False) + '\n' for r in rows))
print('stage-6 and stage-7 rows added again:', len(extra))
"
step uvx modal volume put --force synthexis-parser data/mapping/train/ds-v10c /train/ds-v10c
step uvx modal deploy tools/mapping/modal_parser.py
R="--ds ds-v10c --train train_cat --steps 10000 --backbone mmbert-small-de-en-32k --enc-lr 5e-5 --heads 6 --ff 1536 --amp 1"
step uv run --with modal python tools/mapping/modal_parser.py spawn \
  p-v10-s0 "$R --seed 0" p-v10-s1 "$R --seed 1" p-v10-s2 "$R --seed 2" p-v10-s3 "$R --seed 3"
print "[$(date '+%F %T')] ALL DONE" >> $LOG
