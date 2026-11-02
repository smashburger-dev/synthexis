# Session 2026-11-02: parser v10, direction phrasing

Follows `docs/session-2026-11-01-parser-v9.md`. Plan and rule: `docs/parser-v10-plan.md`, `probes/real-eval/RESULT.md` ("Parser v10: selection rule", "Parser v10"), `probes/parser-errors/RESULT.md` section 13.

## What happened

1. Noa asked for one more round after a search for levers no run had tried. The v9 misses on the changed real set were read by part. INV was the largest slice: 16 rows wrong only there.
2. Cause in the data: `edits.py` drew source phrases only in their 1 direction ("raising my hand") and falling target phrases only on inverted routes. 286 glossary phrases for the 0 direction ("Hand senken") were never used except for the fist pair.
3. Stage 7 says a moving source toward its 0 end and tells each parameter target whether it falls or rises, so INV follows from both; it also names both directions in one phrase, lets a second route name only the quality of the same effect, and adds parameter names. Labels from the edits. DeepSeek wrote 4,638 valid sentences for 0.87 $.
4. `rules.py conv` applies Noa's conventions to the prediction; it never lowers a dev set.
5. Plan and selection rule were committed (378df61) before the first v10 run. Four seeds trained on Modal.

## Result

| | dev, seven older sets | dev_s7 | real eval exact | route F1 | right or one part wrong | calibration |
|---|---|---|---|---|---|---|
| ensemble v9 (v9 ships) | 0.789 | 0.510 | 42/102 | 0.537 | 75/102 | 30/51 |
| ensemble v10 s0+s1, conv (ships) | 0.792 | 0.639 | 47/102 | 0.559 | 79/102 | 25/51 |
| ensemble v10 s0 to s3, conv (not chosen) | 0.791 | 0.644 | 54/102 | 0.617 | 84/102 | 30/51 |

Noa's goal was 43-44 % exact and 78-80 % right or one part wrong. The shipped ensemble reaches 46 % and 77 %. Over two seeds the eval gain is established (+11 points, p 0.004). Calibration falls by 6 to 7 points. The four-seed ensemble passes the goal on both numbers and keeps calibration at 30 of 51, but the rule picks on the dev sets, where it shows no gain, and the real set is seen.

Two regressions are traceable:
- "B warp" (group 07, 5 rows) lost its target because stage 7 listed "B Warp" as a parameter name, which the frame treats as a scope word next to the target.
- Closing the fist while a cutoff goes down (23) now gets an INV, and its mirror (24) loses one.

## After the result

- `score.py` derived 13-s2 and 29-s1 with the thumb tilt although the text says index finger; fixed. Re-scored: v9 ensemble 43, shipped v10 ensemble 49 (48 %), four-seed ensemble 56 of 102 (55 %); with the pronoun rule the four-seed ensemble reaches 85 of 102 right or one part wrong and 35 of 51 on calibration.
- Second slice of the four-seed misses: the hand as subject ("my right hand should control X by pinching and turning", 0 of 8; the validator drops that form), a parameter name as the only target ("macro 2", "B warp"), two encodings for closing the fist, direction hints missing on fan-out targets. Next levers in `docs/parser-v10-plan.md`, "After v10".
- Export: f16 equals f32 on dev (121 MB per run, 32 ms median on one CPU thread); torch dynamic int8 loses 2 to 4 dev points and is slower (43 to 47 ms). Making the four-seed ensemble cheap goes to its own session.

## Money

DeepSeek 0.88 $ (pilot and stage 7). Modal four runs of 22 to 26 minutes on L4s, about 1.5 $ in credits. Baseten untouched.

## Files

- `tools/mapping/edits.py` (`apply_direction`, `same_effect`, `MOVING`), `edits-s7.yaml`, `prompts/commands-s7.md`, `commands.py` (renders `dir`), `dataset.py --stage7`, `train_parser.py` (dev_s7), `rules.py`, `decode_eval.py --rules`, `predict.py --rules`, `run_v10.sh`.
- Data (git-ignored): `data/mapping/commands/edits-s7`, `cmd-s7`; `data/mapping/train/ds-v10`, `ds-v10p`, `ds-v10c`; runs `p-v10-s0` to `p-v10-s3` with `model.f16.pt`; predictions `data/mapping/eval/preds-g2-*-v10-*.jsonl` and `*-conv.jsonl`; dev scores `data/mapping/train/decode/v10dev-*.json`.

## Open

- Four seeds or two: Noa decides (484 MB in f16 against 242 MB, about twice the latency).
- The Rust runtime (`claude/parser-runtime-be3a3c`) loads v9 and does not apply conv. Switching means `tools/mapping/export_runtime.py` on the v10 runs, its parity test, and porting `conventions.py` (about 40 lines) to Rust.
- Drop "B Warp" and other names that are also targets from `param_names` before the next stage.
- A set Noa dictates fresh would settle how much of the gain holds on unseen sentences.
