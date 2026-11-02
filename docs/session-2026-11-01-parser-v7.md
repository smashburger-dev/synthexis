# Session 2026-10-31 to 2026-11-01: parser v4 to v7, real sentences as the judge

Entry point for the next parser session (prompt: `docs/parser-v8-session-prompt.md`). Branch `claude/mapping-assistant-local-training-9b23fe`, last commit of this line in `git log`. Numbers and commands: `probes/real-eval/RESULT.md` (real sentences), `probes/parser-errors/RESULT.md` sections 7 to 10 (runs), `docs/roadmap.md` (mapping section).

## State

- Shipping parser: `p-v7a-s0`, mmBERT small de-en (pretrained, 22 layers, width 384, 60M parameters) as encoder plus the frame decoder of v1 to v6. f16 121 MB, 30 ms median and 68 ms p95 per sentence on one CPU thread (`tools/mapping/export_hf.py`). Not loadable by the plugin yet: the Rust side needs a ModernBERT runtime.
- Real-sentence eval (99 rows, 20 groups, gold drafted by Claude, not yet checked by Noa), both seeds:

| parser | eval exact | right or one part wrong | dev route_edit |
|---|---|---|---|
| v3 | 15 % | 51 % | 0.80 |
| v5 | 15 % | 51 % | 0.83 |
| v6 | 22 % | 50 % | 0.83 |
| v7a (shipping) | 31 % | 68 % | 0.87 |
| Spark, same label spec in the prompt | 63 % | 83 % | |

- Money: Baseten ledger 73.57 $ of 100 $ (26.43 $ left, the motion.labels reserve; `probes/budget-motion-labels/RESULT.md` puts motion.labels at 3 to 13 $). DeepSeek V4.1 Flash 0.75 $ so far (key in Keychain `DEEPSEEK_API_KEY`, topped up by Noa). Modal about 2 $ estimated for 2.5 L4 hours (not read from billing), workspace `smashburger-dev`, app and volume `synthexis-parser`.
- Size budget: Noa allows 500 to 800 MB RAM for the plugin including the image segmenter (2026-11-01), so f16 and larger encoders are fine.

## What each run taught (measured)

1. More synthetic text of the same kind stops helping. Stage 4 (8,691 sentences, new request types) raised dev, the real-sentence score stayed at 15 % (v3 and v5 level).
2. Text in the style of real users helps. Stage 5 (4,476 sentences: long, both directions of one movement, pinch held while moving, chained targets) lifted real eval from 15 % to 22 % with dev flat.
3. Length is not the gap, language is. v6 got 7 of 21 real sentences under 13 words right.
4. The task is learnable from our label spec. Spark with the frame language and 22 synthetic examples in the prompt reaches 63 % exact on eval (`probes/real-eval/spark_parse.py`, 0.29 $).
5. A pretrained encoder is the biggest single lever so far: +9 points exact, +18 points "one fix away" (v7a against v6, p 0.004). mmBERT base (328M) is not better than small.
6. Teacher labels are cheap: DeepSeek V4.1 Flash without thinking labels 67 of 100 synthetic sentences right for 0.004 $ (Spark 71 for 0.185 $); thinking mode is worse (48) and breaks the output format. Where two different teachers agree, 87 % of labels are right.
7. The command validity filter throws away the realistic sentences. 13,950 sentences were dropped in stages 2 to 5, mostly for an unmarked slot ("get really bright" instead of naming the tone knob). After filters 11,846 remain in `data/mapping/teach/pool.jsonl`.
8. A selection set that shaped the training labels cannot select. The teacher prompt carried rules written from calibration errors; v7 with 4,838 teacher rows gained 10 calibration rows and nothing on eval. No real sentence reached training (0 exact matches, shared 6-grams are generic gesture phrases from stages 1 to 3).
9. Smaller findings: projected labels +1.1 points (v5); dropping toggle-noise rows hurt (calibration 16 to 14); decode-time rules gain 0 to 3 calibration rows; dynamic int8 costs 2.2 points with the backbone (5.6 with teacher rows), f16 costs nothing.

## Practical lessons

- Modal: spawn from a deployed app (a `modal run` job dies with the client); L4 capacity can be gone, so the function takes `["L4", "A10G", "L40S"]`; a reused warm container sees an old volume view (`vol.reload()` at the start) and can run old code right after a redeploy (renaming the function forced a fresh one). Data and backbone are uploaded once to the volume.
- Pointer spans: compute them on the final text; never normalise whitespace afterwards (cost one failed launch).
- Every tool that takes a split must say which split it reads; eval is scored once per decision, after the selection rule is written down.

## Files of this session

- `tools/mapping/teacher_pool.py` (pool, ds, spark, merge), `tools/mapping/modal_parser.py`, `tools/mapping/export_hf.py`; backbone support in `frame_parser.py`, `train_parser.py`, `predict.py`.
- `probes/real-eval/spark_parse.py` (ceiling), `probes/real-eval/teacher_check.py` (teacher comparison).
- Data (git-ignored): `data/mapping/teach/` (pool, DeepSeek and Spark labels, kept rows, logs), `data/mapping/train/ds-v7p/`, runs `p-v7*` with `model.pt`, `model.f16.pt`, `export_hf.json`; predictions on the real set `data/mapping/eval/preds-*.jsonl`; backbone links in `data/mapping/backbones/`.

## Open

- Noa checks the gold routes of his 34 sentences (`data/mapping/eval/gold-routes.json`, `review.md`); 11 eval misses are switch against knob and may be gold questions.
- ModernBERT runtime for the plugin (Rust): candle or ONNX via ort, to be checked; plugin line.
- Levers towards 40 % exact and 75 % one-fix: `docs/parser-v8-session-prompt.md`.
- motion.labels with the Baseten reserve: main data session, not this line.
