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
