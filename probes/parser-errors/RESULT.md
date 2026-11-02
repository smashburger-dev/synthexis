# RESULT: parser v3 errors, English and German (2026-10-31)

Question (Noa): the ß bug alone is no reason to retrain. Which errors does the parser make in English, and what can be re-engineered, without fitting the dev sets, before one retrain bundles the fixes?

Run: `p-v3-c50-s0`, dev + dev_s2 + dev_s3 (2,601 sentences). The challenge set is only reported.

## 1. Where English fails

`uv run probes/parser-errors/analyze.py`

| | n | route_edit | targets | sources | modifiers/grouping | intent | scope | invert |
|---|---|---|---|---|---|---|---|---|
| en | 1,630 | 0.744 | 0.077 | 0.067 | 0.044 | 0.034 | 0.018 | 0.017 |
| de | 971 | 0.668 | 0.075 | 0.125 | 0.042 | 0.040 | 0.032 | 0.018 |

(Share of sentences by first failing field.) Weakest registers in both languages: keyword (0.50 / 0.47), layperson (0.66 / 0.59), polite (0.67 / 0.60).

Most frequent English confusions:
- momentary and toggle switch modes (about 30 of 418 errors)
- toggle against mix ("less reverb" as switch or as amount)
- synth pitch against pitch effect (osc.pitch / fx.pitch)
- missed INV (28)
- vague amounts ("a bit" as +0.1, +0.25 or 0.5)
- objects falling back to "ball"
- duplicate encodings (gate "fist" as hand.fist or as hand.shape:fist, side:either against no side)

## 2. Hand-read sample

45 random English errors, read by hand: about half cannot be derived from the sentence. Examples:
- the gold names a drumstick, the sentence says "the thing in my hand"
- INV without any cue in the text
- "a bit" labelled 0.5 in one sentence and nothing in another
- the track carried to a second route in some edits and not in others
- garbled keyword sentences

The other half are model errors:
- "closing my hand" without INV
- "300ms" read as a range
- "500 m" span cut inside "milliseconds"
- typos ("pus hand to camra", "smooting")
- self-correction ("no I mean vertically")
- "facing direction" read as height
- one route split in two

Rough estimate from 45 items.

## 3. Labels the text does not express (measured)

Over all 29,644 generated command sentences (cmd-s1/s2/s3):

| label | en | de |
|---|---|---|
| object named in the gold, never in the sentence | 330 of 1,126 (29 %) | 188 of 645 (29 %) |
| momentary mode without a cue (hold, while, as long as, solange, ...) | 414 of 551 (75 %) | 234 of 307 (76 %) |
| invert neither marked nor cued (less, lower, closing, umgekehrt, ...) | 314 of 3,807 (8 %) | 136 of 2,182 (6 %) |

Cause: `commands.validate_variant` treats invert slots as optional, and the object and mode attributes are never checked against the text. Spark writes a sentence without them, and the gold keeps them. The parser is trained to guess them.

Scoring both sides without what the text does not say and with duplicate encodings merged raises dev route_edit:

| | base | dup | object | mode | invert | pitch | all |
|---|---|---|---|---|---|---|---|
| en | 0.744 | +0.007 | +0.012 | +0.014 | +0.007 | +0.003 | +0.048 |
| de | 0.668 | +0.009 | +0.016 | +0.008 | +0.010 | +0.005 | +0.049 |

This changes the metric, not the model. It shows how much of the error rate is label artifacts that a rule can find. The hand-read sample suggests more (vague amounts, scope convention) that no clean rule captures.

## 4. Tokenizer

- Sentences with a letter word cut into >= 3 pieces: route_edit 0.543 (n 247) against 0.736 (n 2,902). Adjusted for length, language and digits: -0.114.
- Digits are always split per digit, by design. Known plugin names are single pieces (shaperbox, timeless, diffusion, granulizer).
- The fragmented words are mostly German words with ß, typos from the noise variants, and rare names.
- `ß` is not in the vocabulary (`character_coverage=0.9995` in `tools/mapping/tokenizer.py` dropped it although it appears 2,365 times in training). Every ß falls back to two raw bytes: "schließe" is 4 pieces, "schliesse" 1. The 58 sentences with ß: route_edit 0.397.

## 5. Decode-time changes (no retraining)

`uv run probes/parser-errors/decode_tests.py`

| change | result |
|---|---|
| replace ß by ss before tokenizing | 0.397 -> 0.328 on the 58 ß sentences (+3 / -7): worse, the model learned the byte form |
| widen pointer spans to whole words | dev 0.796 -> 0.796, dev_s2 0.708 -> 0.709 (+1 / -0), dev_s3 unchanged; fixes "500 m" -> "500 milliseconds", "valhallaub" -> "valhallaubermod" |
| give a track mentioned once to every route | not applied: the gold gives it to every route in 51 and to some routes in 307 multi-route sentences |

## 6. Plan: one retrain that bundles the fixes, with gates

Principle: the parser outputs what the sentence says. Context the sentence leaves open is bound at runtime by what synthexis knows anyway:
- the object in the camera view (with one object in view, "the object" means that one)
- the plugin type (instrument or effect decides synth pitch against pitch effect; AU type `aumu` / `aufx` from the harvest)
- the project inventory (names)
- a default plus a chip (switch mode)

This is the same split as the open-vocabulary resolver: language in the weights, the world in the runtime.

Changes, all without new Spark data:
1. Tokenizer tok-v2: `character_coverage=1.0`, nothing else changed.
2. Label projection in `dataset.py` (ds-v4p), the same rules as in `analyze.py`:
   - object -> `object:any` when the text never names it
   - mode only with a cue
   - INV dropped when neither marked nor cued
   - gate `hand.fist` -> `hand.shape:fist`
   - `side:either` -> none
   - osc.pitch / fx.pitch -> pitch, split again by the resolver
3. Pointer spans widened to whole words at decode time (Python and the Rust runtime).
4. For stage 4 and later batches: invert slots required with the inverting word marked ("less", "closing"); objects named or labelled `any`.
5. Optional, as an ablation: a per-word hash embedding of char 3-5-grams (the param-sense features, about 1M parameters) added to each subword embedding. It targets typos, ß-like splits and rare names.

Runs (local, 0 $, about 45 min each on MPS):
- A = v3 recipe with tok-v2 (isolates the tokenizer)
- B = A + projected labels
- C = B + word hash embedding

Two seeds each, about 4.5 h.

Gates against `p-v3-c50-s0/s1`:
- Paired McNemar on dev + dev_s2 + dev_s3, per slice: language x first failing field x register.
- Scored on the original labels and on the text-supported labels of `analyze.py`.
- A step is kept only if it improves dev overall and no slice, English included, drops with p < 0.05.
- Challenge: reported only. Noa's 50 calibration sentences: thresholds. Noa's 100 eval sentences: one final comparison between v3 and the winner.

## 7. Result of the bundled retrain (2026-10-31, `tools/mapping/run_v4.sh`)

Two seeds each, pooled paired gates on dev + dev_s2 + dev_s3 (5,202 paired sentences, `tools/mapping/gates.py`):

| step | labels | route_edit | won / lost | p | slices dropping (p < 0.05) |
|---|---|---|---|---|---|
| A (tok-v2) vs v3 | original | 0.716 -> 0.712 | 221 / 239 | 0.43 | none |
| B (A + projected labels) vs A | projected | 0.761 -> 0.766 | 242 / 217 | 0.26 | none |
| B vs A | original | 0.712 -> 0.666 | 178 / 422 | < 0.001 | many, by construction: B no longer guesses what the text does not say |
| C (B + word hash) vs B | projected | 0.766 -> 0.768 | 244 / 235 | 0.71 | none |

- Sentences with ß (58 in all dev sets and the challenge set): v3 23 and 17 right, A 21 and 25, B 24 and 26 (seeds 0 and 1).
- Seed-to-seed spread of v3 itself: 0.716 -> 0.716 (130 / 128).

Reading: no step moves the dev sets beyond seed noise, and none degrades a slice except by the label definition. The tokenizer fix helps the ß sentences a little. The projected labels match the runtime-binding principle at no cost. The word hash adds 1.07M parameters and a hashing step in the Rust runtime for no measured gain, so it is dropped.

Final recipe: B on ds-v5p (ds-v3 + stage 4 with ontology v1.1 and v1.2, near-copies of the real-sentence set removed), `tools/mapping/run_v5.sh`.

## 8. Result of the final run v5 (2026-10-31, `tools/mapping/run_v5.sh`)

Recipe B on ds-v5p: ds-v3 plus stage 4 (ontology v1.1 and v1.2, 8,691 sentences minus 59 near-copies of the real-sentence set), projected labels, tok-v2. Two seeds, 16,000 steps, 42 min each.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 (new types) |
|---|---|---|---|---|---|
| p-v5-s0 | 0.837 | 0.781 | 0.777 | 0.659 | 0.736 |
| p-v5-s1 | 0.827 | 0.790 | 0.788 | 0.625 | 0.738 |

Gates against v3, pooled over both seeds (dev + dev_s2 + dev_s3, 5,202 pairs):

| labels | route_edit | won / lost | p | slices dropping (p < 0.05) |
|---|---|---|---|---|
| projected | 0.766 -> 0.777 | 307 / 250 | 0.018 | none; de 0.715 -> 0.739 (p 0.003) |
| original | 0.716 -> 0.671 | 226 / 457 | < 0.001 | many, by construction as in section 7 |

On the 150 real sentences (`probes/real-eval/RESULT.md`, section "Parser runs") v5 is level with v3. The gain stays on synthetic text.

Export of p-v5-s0 (`--ds ds-v5p --no-audit`): int8 blob 9.4 MB, int8 costs 0.1 points dev route_edit; 10 ms median, 25 ms p95 on one CPU thread.

## 9. Result of the style run v6 (2026-11-01, `tools/mapping/run_v6.sh`)

Recipe B on ds-v6p: ds-v5 plus stage 5 (4,476 sentences in the style of the calibration findings: both directions of one movement, chained targets, pinch held while the hand moves, casual and rambling registers; 125 near-copies of the real set dropped; 6.77 $). Two seeds, 16,000 steps, 50 min each. This is the last data run of this line.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 | dev_s5 (new style) |
|---|---|---|---|---|---|---|
| p-v6-s0 | 0.829 | 0.772 | 0.783 | 0.632 | 0.758 | 0.659 |
| p-v6-s1 | 0.826 | 0.792 | 0.796 | 0.651 | 0.758 | 0.651 |

Gates against v5, pooled over both seeds (dev + dev_s2 + dev_s3, 5,202 pairs):

| labels | route_edit | won / lost | p | slices dropping (p < 0.05) |
|---|---|---|---|---|
| projected | 0.777 -> 0.779 | 242 / 229 | 0.58 | none |
| original | 0.671 -> 0.678 | 193 / 160 | 0.088 | none |

Against v3 on projected labels: 0.766 -> 0.779, 308 / 238, p 0.003, no slice drops.

On the 150 real sentences v6 beats v5 and v3 (`probes/real-eval/RESULT.md`, section "Parser run v6"). The synthetic dev sets stay level, so the gain comes from the new sentence style and not from more of the old one.

Export of p-v6-s0 (`--ds ds-v6p --no-audit`): int8 blob 9.4 MB, int8 costs 0.1 points dev route_edit; 10 ms median, 25 ms p95 on one CPU thread. p-v6-s0 replaces p-v5-s0. Seed 0 is taken by the same rule as v5; the eval split did not pick the seed.

## 10. Parser v7: pretrained encoder (2026-11-01, `tools/mapping/modal_parser.py`)

The from-scratch encoder (6 x d256, 9.3M parameters, tok-v2) is replaced by mmBERT small trimmed to German and English (ModernBERT, 22 layers, width 384, 32k vocabulary, from `kleinhirn-k24/models/k24/backbones/`). The frame decoder, grammar and labels stay. Decoder 2 layers, 6 heads, ff 1536; encoder at lr 5e-5, the rest at 5e-4; 10,000 steps, bf16, on a Modal L4 (20 min per run, about 125 ms per step against 600 ms on the Mac).

Arms: `v7a` mmBERT small on ds-v6p; `v7` the same on ds-v7p (ds-v6p plus 4,838 teacher rows x2, `tools/mapping/teacher_pool.py`); `v7b` mmBERT base (full vocabulary, 328M parameters) on ds-v7p.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 | dev_s5 |
|---|---|---|---|---|---|---|
| p-v6-s0 | 0.829 | 0.772 | 0.783 | 0.632 | 0.758 | 0.658 |
| p-v7a-s0 | 0.872 | 0.829 | 0.832 | 0.716 | 0.801 | 0.698 |
| p-v7a-s1 | 0.872 | 0.841 | 0.835 | 0.733 | 0.798 | 0.693 |
| p-v7-s0 | 0.881 | 0.830 | 0.818 | 0.714 | 0.807 | 0.685 |
| p-v7-s1 | 0.877 | 0.823 | 0.816 | 0.680 | 0.801 | 0.695 |
| p-v7b-s0 | 0.868 | 0.832 | 0.830 | 0.700 | 0.772 | 0.700 |

(route_edit; dev_s4 loses 5 of 828 sentences to the new tokenizer.)

Teacher data: the command batches of stages 2 to 5 dropped 13,950 sentences, mostly for an unmarked slot. 11,846 remain after dev edits, leftover slot ids, duplicates and 175 near-copies of the real set. Each has three readings (the edit, DeepSeek V4.1 Flash without thinking, Spark where those two disagree; 2,925 Spark rows within budget); a sentence is kept when two agree: 3,704 edit+DeepSeek, 320 edit+Spark, 814 DeepSeek+Spark. Cost 0.68 $ DeepSeek, 4.86 $ Spark.

On the real sentences (`probes/real-eval/RESULT.md`, section "Parser v7") the encoder lifts eval exact match from 22 % to 31 % (both seeds, p 0.004); the teacher rows do not. mmBERT base is not better than small.

Export of p-v7a-s0 (`tools/mapping/export_hf.py`): f16 state dict 121 MB, f16 costs nothing on dev (0.872); dynamic int8 costs 2.2 points (v7 with teacher rows: 5.6), so f16 ships. CPU, one thread, f32: 30 ms median, 68 ms p95 per sentence. The plugin runtime does not load backbone models yet (Rust side open). p-v7a-s0 replaces p-v6-s0.

## 11. Parser v8: ensemble, beam, domain MLM, clean teacher rows, mDeBERTa (2026-11-01)

Selection rule in `probes/real-eval/RESULT.md` ("Parser v8: selection rule"), written before scoring: mean route_edit over the six dev sets of ds-v6p, two seeds per arm, +0.005 over v7a (0.7933) to replace it. All training arms use the v7a recipe (decoder 2 x 6 heads, ff 1536, encoder lr 5e-5, 10,000 steps, bf16, Modal L4) and differ in one thing:

- `v8m`: mmBERT small after masked-language modelling on 77,044 in-domain sentences (ds-v6p train_all and the teacher pool, sentences of the dev splits left out, no real sentence), 2 epochs, lr 1e-4, 30 % masked, 4 minutes on an L4; held-out MLM loss 5.31 -> 1.20 (`tools/mapping/mlm_backbone.py`).
- `v8t`: ds-v6p plus 6,224 teacher rows x2 (`tools/mapping/teacher_v8.py`). Neutral DeepSeek prompt built from the ontology and the frame grammar, examples drawn per intent and target kind; two passes with the examples in two orders. Kept when both passes agree: 3,441 with the edit agreeing too (edit frame), 2,783 without (teacher frame); 2,438 disagree, 3,182 unusable (mostly parameter ids outside the ontology such as `T:param:pan`). Check on 100 dev_s5 sentences: 59 and 57 right per pass, 52 of the 81 agreed. Cost 1.18 $.
- `v8d`: mDeBERTa-v3-base (`microsoft/mdeberta-v3-base` at a0484667, 278M parameters, width 768, full 250k vocabulary), 295 ms per step, 55 minutes per run.

Decoding (`tools/mapping/decode_eval.py`, `frame_parser.beam`): mean of the members' masked log-softmax per step, renormalised; beam with early stop.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 | dev_s5 | mean |
|---|---|---|---|---|---|---|---|
| p-v7a-s0 | 0.872 | 0.829 | 0.832 | 0.716 | 0.801 | 0.698 | 0.7912 |
| p-v7a-s1 | 0.872 | 0.841 | 0.835 | 0.733 | 0.798 | 0.693 | 0.7954 |
| p-v7a-s2 | 0.875 | 0.843 | 0.841 | 0.692 | 0.814 | 0.700 | 0.7941 |
| p-v7a-s3 | 0.878 | 0.825 | 0.819 | 0.712 | 0.801 | 0.695 | 0.7882 |
| p-v8m-s0 | 0.879 | 0.825 | 0.833 | 0.731 | 0.798 | 0.705 | 0.7952 |
| p-v8m-s1 | 0.873 | 0.830 | 0.826 | 0.704 | 0.801 | 0.683 | 0.7863 |
| p-v8t-s0 | 0.873 | 0.845 | 0.834 | 0.700 | 0.772 | 0.685 | 0.7848 |
| p-v8t-s1 | 0.870 | 0.834 | 0.831 | 0.697 | 0.792 | 0.688 | 0.7853 |
| p-v8d-s0 | 0.877 | 0.845 | 0.851 | 0.731 | 0.819 | 0.700 | 0.8037 |
| p-v8d-s1 | 0.871 | 0.834 | 0.841 | 0.719 | 0.796 | 0.693 | 0.7921 |
| ensemble v8d s0+s1, greedy | 0.884 | 0.852 | 0.851 | 0.736 | 0.825 | 0.702 | 0.8085 |
| ensemble v7a s0+s1, greedy | 0.882 | 0.852 | 0.844 | 0.721 | 0.814 | 0.700 | 0.8022 |
| ensemble v7a s2+s3, greedy | 0.879 | 0.858 | 0.842 | 0.712 | 0.820 | 0.700 | 0.8018 |
| ensemble s0+s1, beam 4 | 0.878 | 0.849 | 0.841 | 0.719 | 0.813 | 0.698 | 0.7994 |
| ensemble s0+s1, beam 8 | 0.879 | 0.850 | 0.844 | 0.721 | 0.813 | 0.698 | 0.8010 |
| ensemble s0+s1, beam 4, length norm 1 | 0.878 | 0.849 | 0.841 | 0.719 | 0.813 | 0.698 | 0.7994 |
| v7a s0, beam 4 | 0.868 | 0.823 | 0.831 | 0.716 | 0.796 | 0.695 | 0.7883 |

(route_edit on the dev sets of ds-v6p; mean over the six.)

The ensemble is the only lever that clears the rule on dev, twice. Beam never beats greedy: it finds frames with a higher log-probability that are wrong more often. On the real sentences none of it reaches the 40 % and 75 % goal; the ensemble s0+s1 gets 37 % and 74 %, the second pair 29 % and 68 %; the training arms lose against v7a (`probes/real-eval/RESULT.md`, section "Parser v8").

Export of the chosen ensemble: p-v7a-s0 and p-v7a-s1 as f16 state dicts (2 x 121 MB, `export_hf.py` per run); with f16-rounded weights the ensemble keeps 0.8022 on dev. CPU, one thread: 62 ms median, 138 ms p95 per sentence (two encoders, grammar-constrained greedy over the mean). The plugin still has no runtime for the backbone.

## 12. Parser v9: Noa's conventions and stage 6 (2026-11-01, `tools/mapping/run_v9.sh`)

Plan and the checks behind it: `docs/parser-v9-plan.md`. Noa decided: a pinch next to a hand movement is a hold, parameters that move in different directions are separate routes, fingers shown together are one finger state, switching words mean a switch; an effect word ("opens up") gets no fixed parameter class.

- Stage 6 (`edits-s6.yaml`, prompt `prompts/commands-s6.md`): 6,430 edits with two new forms in `edits.py` (one movement fanning out to 2 to 4 parameters with opposite directions; each finger setting one parameter to its own value, the parameter named once), pinch holds at 0.5, finger masks at 0.7 of shape sources, switches at 22 % of targets with a list of switching words (bypass, disable, engage, kick in, einschalten, zuschalten ...; FL Studio's own slot switch is "disable"). Written by DeepSeek V4.1 Flash (`commands.py --backend deepseek`), 4785 of 12860 variants pass the mark validation (37%; Spark passed 48 % on stage 5 and 60 % on stage 4), 0.87 $. 179 near-copies of the real set dropped by `leak.py`.
- `conventions.py` relabels all splits (pinch next to a movement: 43 training rows of the old data; fingers shown together: 2), so v7a is re-scored on the same labels. Stage-6 rows go twice into train_cat. v7a recipe otherwise, two seeds on a Modal L4.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 | dev_s5 | dev_s6 | mean of six older | mean of seven |
|---|---|---|---|---|---|---|---|---|---|
| ensemble v7a s0+s1 | 0.882 | 0.849 | 0.843 | 0.721 | 0.814 | 0.700 | 0.351 | 0.8014 | 0.7371 |
| p-v9-s0 | 0.876 | 0.841 | 0.834 | 0.728 | 0.820 | 0.712 | 0.610 | 0.8020 | 0.7745 |
| p-v9-s1 | 0.878 | 0.829 | 0.833 | 0.721 | 0.825 | 0.702 | 0.593 | 0.7980 | 0.7687 |
| ensemble v9 s0+s1 | 0.886 | 0.849 | 0.840 | 0.757 | 0.840 | 0.732 | 0.621 | 0.8171 | 0.7890 |

(route_edit on ds-v9c; greedy.)

v7a reads the new stage-6 sentences at 0.351; v9 at 0.62, and it gains 1.6 points on the older sets too. On the changed real set the v9 ensemble gets 42 of 102 exact (v7a ensemble 37) and route F1 0.537 (0.425); the route F1 gain holds over two seeds, the exact gain does not (`probes/real-eval/RESULT.md`, section "Parser v9").

Export: p-v9-s0 and p-v9-s1 as f16 (2 x 121 MB), f16 equal to f32 on dev, 31 ms median per run on one CPU thread. The Rust runtime of `claude/parser-runtime-be3a3c` loads v7a; v9 needs `tools/mapping/export_runtime.py` re-run on the v9 runs there.

## 13. Parser v10: direction phrasing, stage 7 (2026-11-02, `tools/mapping/run_v10.sh`)

Plan: `docs/parser-v10-plan.md`. The v9 misses on the changed real set were read by part; INV was the largest slice (16 rows wrong only there). `edits.py` had drawn glossary phrases for sources only in their 1 direction and falling target phrases only on inverted routes, so "goes down" meant invert to the parser.

- Stage 7 (`edits-s7.yaml`, `prompts/commands-s7.md`): 6,479 edits. A moving source is said toward its 0 end in 45 % of edits (glossary phrase for the 0 direction where one exists) and every parameter target of its routes is told `dir: falls` or `dir: rises`, so the INV label is the product of the two; 15 % name both directions in one phrase; 35 % of two-route edits put another control of the first route's effect on the second route; 20 % carry a parameter name. Written by DeepSeek V4.1 Flash, 4,638 of 12,958 variants valid (36 %), 0.87 $; 199 near-copies of the real set dropped by `leak.py` (closest remaining ratio 0.60). A falling-direction word appears in 622 of 653 sentences whose targets all fall, a rising word in 123 of 139 whose targets all rise.
- `rules.py conv`: `conventions.py` applied to the prediction.
- v9 recipe otherwise (ds-v10c, stage-6 and stage-7 rows twice in train_cat), four seeds on Modal L4s, 22 to 26 minutes each.

| run | dev | challenge | dev_s2 | dev_s3 | dev_s4 | dev_s5 | dev_s6 | dev_s7 | mean of seven older | mean of eight |
|---|---|---|---|---|---|---|---|---|---|---|
| p-v9-s0 | 0.876 | 0.841 | 0.834 | 0.728 | 0.820 | 0.712 | 0.610 | 0.506 | 0.7745 | 0.7409 |
| p-v9-s1 | 0.878 | 0.829 | 0.833 | 0.721 | 0.825 | 0.702 | 0.593 | 0.479 | 0.7687 | 0.7324 |
| ensemble v9 s0+s1 | 0.886 | 0.849 | 0.840 | 0.757 | 0.840 | 0.732 | 0.621 | 0.510 | 0.7890 | 0.7542 |
| p-v10-s0 | 0.876 | 0.829 | 0.836 | 0.702 | 0.809 | 0.705 | 0.603 | 0.606 | 0.7656 | 0.7456 |
| p-v10-s1 | 0.877 | 0.838 | 0.830 | 0.707 | 0.802 | 0.710 | 0.653 | 0.626 | 0.7738 | 0.7553 |
| ensemble v10 s0+s1 | 0.887 | 0.850 | 0.846 | 0.731 | 0.826 | 0.739 | 0.666 | 0.639 | 0.7921 | 0.7730 |
| ensemble v10 s2+s3 | 0.876 | 0.849 | 0.832 | 0.728 | 0.823 | 0.712 | 0.636 | 0.613 | 0.7793 | 0.7585 |
| ensemble v10 s0 to s3 | 0.888 | 0.858 | 0.842 | 0.736 | 0.828 | 0.729 | 0.659 | 0.644 | 0.7914 | 0.7729 |

(route_edit on ds-v10c, greedy, without conv; conv adds 0.0004 to 0.0008 to every mean and lowers no set.)

v9 reads the new stage-7 sentences at 0.51, v10 at 0.64. On the older sets the first v10 pair holds level (+0.3 points), the second pair loses 1 point; dev_s3 and dev_s4 fall for every v10 run. On the real set the shipped ensemble gets 47 of 102 eval rows exact and 79 right or one part wrong, with calibration down from 30 to 25 of 51 (`probes/real-eval/RESULT.md`, section "Parser v10").
