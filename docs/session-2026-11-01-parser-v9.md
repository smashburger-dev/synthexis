# Session 2026-11-01 (evening): parser v9, Noa's conventions

Follows `docs/session-2026-11-01-parser-v8.md` and `docs/parser-v9-plan.md`. Numbers: `probes/real-eval/RESULT.md` (sections "Parser v9: changed set and selection rule" and "Parser v9"), `probes/parser-errors/RESULT.md` section 12.

## What happened

1. Noa read the gold routes as diagrams (https://claude.ai/artifact/UuASRSfAqEbYaviStxhYr7) and named his doubts per sentence. Each was checked against training labels, dev sets and probes (`docs/parser-v9-plan.md`). Punctuation and the word "and" were not the cause; ellipsis, effect words, switching words, finger combinations and the pinch label habit were.
2. Noa decided four conventions: a pinch next to a movement is a hold; parameters moving in different directions are separate routes; fingers shown together are one finger state; switching words mean a switch. An effect word ("opens up") does not get a fixed parameter class.
3. The real set changed: 19 accepts the chord-hand preset, 18 accepts stepping synthexis' arp style, 28 was replaced by two Claude-written sentences where one movement drives two and four Serum 2 parameters in opposite directions (35, 36, with derived rows), 29 and 30 point to ShaperBox 3. Old files kept as `expanded.v8.jsonl`, `gold-routes.v8.json`.
4. Stage 6: 6,430 edits, written by DeepSeek V4.1 Flash, 4,785 valid sentences for 0.87 $. Labels come from the edits, not from a teacher.
5. v9 = v7a recipe on ds-v9c (stages 1 to 6, conventions applied to every split, stage 6 twice in train_cat), two seeds on Modal.

## Result

| | dev, six older sets | dev_s6 (new style) | real eval exact | real eval route F1 | right or one part wrong |
|---|---|---|---|---|---|
| ensemble v7a (v8) | 0.801 | 0.351 | 37/102 | 0.425 | 77/102 |
| ensemble v9 (ships) | 0.817 | 0.621 | 42/102 | 0.537 | 75/102 |

Over two seeds the route F1 gain (+10 points) holds, the exact gain (+4.9 points, p 0.18) does not. The real set was seen while the conventions were written, so it is weaker evidence than in v7 and v8. Noa's originals now exactly right that were not: 6, 26, 35.

## Money

DeepSeek 0.89 $ in this part (ledger total about 2.75 $). Modal about 1.0 $ in the hour of the two v9 runs (credits). Baseten untouched (73.57 $).

## Files

- `tools/mapping/edits.py` (`gen_fan_out`, `gen_finger_values`), `edits-s6.yaml`, `prompts/commands-s6.md`, `commands.py --backend deepseek --prompt`, `dataset.py --stage6`, `conventions.py`, `run_v9.sh`.
- `probes/real-eval/score.py` counts a preset as a route; `decode_eval.py` scores gold tokens the runs never saw as misses.
- Data (git-ignored): `data/mapping/commands/edits-s6`, `cmd-s6`; `data/mapping/train/ds-v9`, `ds-v9p`, `ds-v9c`; runs `p-v9-s0`, `p-v9-s1` with `model.f16.pt`; predictions `data/mapping/eval/preds-g2-*.jsonl`; `ds-v6c` is a test output of `conventions.py` and can go.

## Open

- The Rust runtime (`claude/parser-runtime-be3a3c`) loads v7a. Switching to v9 means re-running `tools/mapping/export_runtime.py` on p-v9-s0 and p-v9-s1 and its parity test.
- A fresh set Noa dictates would show whether the exact gain is real.
- Finger combinations stayed thin (108 stage-6 sentences, some only say "that shape"), and four-parameter fan-outs (36) are still wrong.
- Below confidence 0.5 the plugin should show chips and a hint (plan step 4, plugin line).
