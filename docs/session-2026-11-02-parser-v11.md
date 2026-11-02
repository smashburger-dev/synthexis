# Session 2026-11-02: parser v11, four data gaps

Follows `docs/session-2026-11-02-parser-v10.md`. Plan and rule: `docs/parser-v11-plan.md`, `probes/real-eval/RESULT.md` ("Parser v11: selection rule", "Parser v11"), `probes/parser-errors/RESULT.md` section 14.

## What happened

1. A second read of the v10 four-seed misses had found four gaps in how the training data is made. The plan and the selection rule were committed (2c67731) before any v11 run.
2. Side check: `commands.py clause_side` accepts a side word elsewhere in the sentence when a body word follows it and no source of the other hand lies between. The stored side drops of stages 6 and 7 were validated again: 397 sentences pass, no new text.
3. Fold: `conventions.py` writes closing the fist as openness with the route's INV flipped (and a closed eye as an open one), on every split; `rules.py conv` does the same to predictions, so v10 is scored on the same labels.
4. Stage 8: the `instrumental` frame (the hand as subject, the movement as the means), targets said only by their parameter name, direction hints on every fan-out target. DeepSeek wrote 4,898 valid sentences for 0.77 $; the leak check dropped 113.
5. Four seeds on Modal, scored on dev, then the real set once.

## Result

| | dev, eight older sets | dev_s8 | real eval exact | route F1 | right or one part wrong | calibration |
|---|---|---|---|---|---|---|
| ensemble v10 s0 to s3, conv, pronoun | 0.776 | 0.434 | 56/102 | 0.643 | 85/102 | 35/51 |
| ensemble v11 s0+s1, conv, pronoun | 0.781 | 0.643 | 63/102 | 0.729 | 89/102 | 31/51 |
| ensemble v11 s0 to s3, conv, pronoun (ships) | 0.789 | 0.659 | 66/102 | 0.727 | 89/102 | 32/51 |

By the rule v11 replaces v10 (+0.037 on the nine dev sets, +0.013 on the eight older ones). Over four seeds the eval gain is established (+15 points, p < 0.0001); every v11 seed alone gets 62 to 64 of 102. Calibration holds over seeds and loses 3 rows for the ensemble.

The levers hit their groups: hand as subject 7 of 8 rows (before 0), fan-out targets in opposite directions 4 of 4 (2), "B warp" 6 of 6 (5). Two new misses are traceable:
- A finger showing a MIDI pattern now gets a value (group 10, 2 to 0 of 7).
- A later movement takes the side of the hand named before it (calibration 20, 4 to 1 of 5). The widened side check teaches that carry-over. Noa ruled afterwards that the carry-over is right; with the gold fixed, 20 goes from 1 to 4 of 5 and calibration to 35/51 (v10 32/51).

## Money

DeepSeek 0.77 $ (pilot and stage 8). Modal four runs on L4s, about 1.5 $ in credits. No Spark, no Baseten.

## Files

- `tools/mapping/commands.py` (`clause_side`, `--revalidate`, name-only slot line), `conventions.py` (`fold`), `rules.py`, `edits.py` (`name_only`, `fan_out_dir`), `edits-s8.yaml`, `prompts/commands-s8.md`, `dataset.py --stage8`, `train_parser.py` (dev_s8), `run_v11.sh`.
- Data (git-ignored): `data/mapping/commands/edits-s8`, `cmd-s8`, `cmd-s6r`, `cmd-s7r`; `data/mapping/train/ds-v11`, `ds-v11p`, `ds-v11c`; runs `p-v11-s0` to `p-v11-s3` with `model.f16.pt`; predictions `data/mapping/eval/preds-g2-*-v11-*-convpro.jsonl` and `preds-g2-p-v10-s*-convpro.jsonl`; dev scores `data/mapping/train/decode/v11dev-*.json`.

## Open

- Gold questions for Noa, unchanged: 17 (arp range and rate: Serum's parameter or synthexis' MIDI generator), 12 (pinch with both axes: XY pad or two routes).
- Calibration 20 settled by Noa: the side carries over unless another hand is named. Gold 20 and 34 changed (`probes/real-eval/RESULT.md`, "Gold fix: side carry-over"); calibration is now v11 four-seed 35/51 against v10 32/51. v11 does not yet carry the side to "my hand" (34).
- MIDI patterns read with a value (group 10): look at which stage-8 rows put VALUE next to a pattern.
- The four-seed ensemble costs 4 x 121 MB; the session parser-v10-lean has the v11 run names for the same compression it applies to v10.
- The Rust runtime still loads v9 and does not apply conv or the fold.
