# Session 2026-11-01: parser v8, four levers against the real-sentence gap

Prompt: `docs/parser-v8-session-prompt.md`. Previous state: `docs/session-2026-11-01-parser-v7.md`. Numbers: `probes/real-eval/RESULT.md` (sections "Parser v8: selection rule" and "Parser v8"), `probes/parser-errors/RESULT.md` section 11.

## Result

Goal was 40 % exact and 75 % right or one part wrong on the eval part of Noa's sentences, from 31 % and 68 % (v7a). Not reached.

| arm | dev score (mean route_edit, 6 sets) | eval exact | eval right or one part wrong |
|---|---|---|---|
| v7a, 4 seeds | 0.788 to 0.795 | 26 to 35 % | 63 to 72 % |
| ensemble v7a s0+s1 (ships) | 0.802 | 37 % | 74 % |
| ensemble v7a s2+s3 | 0.802 | 29 % | 68 % |
| v8m, domain MLM, 2 seeds | 0.791 | 32 %, 27 % | 62 %, 67 % |
| v8t, clean teacher rows, 2 seeds | 0.785 | 20 %, 27 % | 59 %, 61 % |
| v8d, mDeBERTa-v3-base, 2 seeds | 0.798 | 28 %, 23 % | 55 %, 56 % |
| Spark (ceiling) | | 63 % | 83 % |

Shipping: the greedy ensemble of p-v7a-s0 and p-v7a-s1 (rule 3), f16 2 x 121 MB, 62 ms median and 138 ms p95 on one CPU thread. Its gain over single v7a seeds is about 2 points on eval over both pairs (17 won, 8 lost, p 0.11) and about 1 point on dev. p-v7a-s0 alone is the fallback if the plugin needs the RAM or the latency.

## What each lever taught (measured)

1. Ensemble: +0.9 and +1.1 points on dev with two different seed pairs; on eval +6.1 points for one pair and -1.5 for the other. Cheap, no training, doubles RAM and latency.
2. Beam search (width 4 and 8, with and without length normalisation) does not beat greedy on dev, alone or in the ensemble. The parser's log-probability does not rank whole frames well, so more search finds more confident wrong frames.
3. Domain MLM on 77k synthetic sentences drops the MLM loss from 5.31 to 1.20 and leaves the parser where it was on dev (0.791) and eval. The encoder already reads this text well enough; the gap is not vocabulary.
4. Clean teacher rows hurt (eval -7.6 points, p 0.004) although no calibration-derived rule shaped them. v7's teacher rows failed through label noise: the neutral prompt labels 59 of 100 synthetic sentences right, and where both passes agree only 52 of 81. Two passes of one model at temperature 0 mostly agree on the same errors.
5. mDeBERTa-v3-base is the best single run on dev (0.804) and among the weakest on eval (28 % and 23 %). Dev ranks encoder families badly; over the ten single runs the rank correlation of dev score and eval exact is 0.67. Its ensemble (0.8085 dev) needs about 1.1 GB.
6. Seed spread on eval is as large as most effects: four v7a seeds give 26 to 35 of 99 exact. A claim on 99 rows in 20 groups needs two seeds and a paired test; a single seed or a single ensemble can show 6 points either way.

## Where the remaining gap is

Spark with the same label spec reaches 63 %. Data levers stopped paying from v6 on, model size does not pay (mmBERT base, mDeBERTa), search does not pay. Candidates that are still untested:
- Noa's check of the gold routes (11 eval misses are switch against knob); costs no training and can move every number.
- A larger teacher with real agreement between different models (Spark plus DeepSeek reached 87 % precision in v7) for the dropped sentences, which needs Spark budget and Noa's OK.
- Runtime fallback: the chips already cover "one part wrong" (74 %), so the plugin can lean on them for long sentences.

## Money

- Modal: about 5.1 $ in the hours 14 to 17 UTC (`modal billing report --for today --resolution h`), covered by the monthly credits; 8 parser runs (v8m, v8t and v7a s2/s3 about 20 minutes each, v8d 55 minutes each) and one 4-minute MLM run.
- DeepSeek V4.1 Flash: 1.18 $ (check plus two passes over 11,846 pool sentences); ledger total 1.86 $.
- Baseten: untouched, 73.57 $.

## Files

- `tools/mapping/frame_parser.py`: `beam` (grammar-constrained beam over a seed ensemble), `evaluate(decode=...)`.
- `tools/mapping/decode_eval.py`: dev-set score of a decoder; `tools/mapping/predict.py --run a,b [--beam k]`.
- `tools/mapping/mlm_backbone.py` (corpus, train), `modal_parser.py mlm`.
- `tools/mapping/teacher_v8.py` (check, label, merge).
- `probes/real-eval/arms.py`: arm table and paired tests on the real set ("right or one part wrong" as defined in the selection rule).
- Data (git-ignored): runs `p-v7a-s2`, `p-v7a-s3`, `p-v8m-*`, `p-v8t-*`, `p-v8d-*`; `model.f16.pt` of p-v7a-s0 and p-v7a-s1; dev decodes in `data/mapping/train/decode/`; backbones `mmbert-small-dapt1`, `mdeberta-v3-base` (plus the raw download in `backbones/_dl/` and a smoke-test backbone `smoke-mlm`, both deletable); `data/mapping/train/ds-v8t/`, `data/mapping/train/mlm/corpus-v1.txt`; teacher labels `data/mapping/teach/v8-*`; predictions `data/mapping/eval/preds-{p-v7a-s2,p-v7a-s3,ens-v7a-01,ens-v7a-23,p-v8*,ens-v8d}.jsonl`.

## Open

- Noa checks `data/mapping/eval/gold-routes.json` (`review.md`).
- ModernBERT runtime for the plugin (Rust side), now for two encoders if the ensemble stays.
- FigJam row "Your real sentences" carries the v8 numbers.
