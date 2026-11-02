# Parser v10 plan (2026-11-02)

Noa wants one more round after the v9 result (41 % exact, 74 % right or one part wrong on the changed real set) and sees a way to 43-44 % exact and 78-80 % right or one part wrong. This plan collects what v3 to v9 taught, reads the v9 misses by part, and lists what no run has tried. Written before any v10 run exists.

## What v3 to v9 tried

| run | lever | real set (exact, eval) | what it taught |
|---|---|---|---|
| v3, v5 | parser from scratch, stage 4 adds missing request types | 14-16 of 99 | more synthetic text of the same kind does not move real sentences |
| decode rules | pinch, pronoun on the prediction | +0 to +3 calibration rows | pinch costs dev sentences; pronoun never fires on synthetic text |
| label drop | drop toggle rows without a switching word | calibration 16 -> 14 | the dropped rows carried more than the wrong label |
| v6 | stage 5, shaped by calibration findings | 18-25 of 99 | text written in Noa's style moves real sentences |
| Spark parses directly | ceiling | 62 of 99 | the task is learnable from the spec |
| v7 | pretrained encoder (mmBERT small) | 30-32 of 99 | the encoder carries the gain; teacher rows from a calibration-shaped prompt carry its rules |
| v8 | seed ensemble, beam, domain MLM, clean DeepSeek teacher rows, mDeBERTa | 37 (ensemble) | ensemble +1 dev point twice; beam never beats greedy; teacher labels cost 7.6 points; dev ranks encoder families badly |
| v9 | Noa's conventions, stage 6 written by DeepSeek with labels from the edits | 42 of 102 (changed set) | labels from edits work; route F1 +10 points over two seeds, exact gain not established |

Two things worked: text in Noa's style with labels taken from the edits (v6, v9) and the pretrained encoder (v7). Teacher labels hurt three times (v7, v8t, and the label drop). Decoding changes moved little, except the seed ensemble.

## Where v9 still fails

The shipped ensemble on the changed set misses 21 of 51 calibration rows and 60 of 102 eval rows. By the part that differs from the closest gold (`probes/real-eval`, read on 2026-11-02 at Noa's request, so the set is seen for v10):

| part wrong | calibration | eval |
|---|---|---|
| only INV | 6 | 10 |
| only the target kind | 7 | 6 |
| only the sources | 2 | 10 |
| only mode, VALUE or side | 1 | 5 |
| two parts or more | 5 | 29 |

INV is the largest single slice, and it fails the same way in three shapes:
- A movement said in its low direction with a falling target ("turn my thumb down, the bits go down", "push my hand down, the resample goes down"): v9 writes INV, gold has none.
- Both directions named ("opening and closing my fist", "and when I close it, it goes back down"): v9 writes INV, gold has none.
- A raised source with a falling target ("opening my fist, the low cut goes down"; one of two targets falls): v9 misses the INV.

The training data explains it. `edits.py` draws glossary phrases for sources only in their 1 direction ("raising my hand"), and phrases for targets in their falling direction only on inverted routes. The glossary holds 286 source phrases for the 0 direction ("Hand senken", "closing my hand"), and only the fist pair ever used them. So "goes down" taught invert, and a source in its low direction with a falling target, where the two cancel, almost never occurs.

The other slices: source verbs ("push down" read as depth, "drag across the screen" as depth, "turn" as height or openness, "turn the thumb down" as the thumb finger); two fingers shown together still read as two fingers (05, 3 rows, although the labels say otherwise); a later clause that names only the quality ("and longer when I spread my fingers") left without a target; numbered or lettered parameter names ("macro 2", "oscillator C level") read as MIDI chords.

## What no run has tried

1. Direction phrasing in the data (stage 7). A moving source is said toward its 0 end in 45 % of edits and with both directions in 15 %; each parameter target is told whether it rises or falls, so the INV label stays the product of the two. Labels come from the edits, as in v6 and v9. New code: `edits.py` (`apply_direction`, `same_effect`), `edits-s7.yaml`, `prompts/commands-s7.md`.
2. Same effect named once: the second route's target is another control of the first route's effect, and the later clause names only the quality.
3. More numbered and lettered parameter names in the `n` slot (param_name scope 20 %).
4. A movement-word list per source in the prompt. It was written after reading the slices, so it is the part of stage 7 closest to the real set; the leak check still runs on every sentence.
5. Noa's conventions applied to the prediction (`rules.py conv`). The labels already pass `conventions.py`, so applying it to the prediction can only rewrite a reading the labels would rewrite too. On the seven dev sets of ds-v9c it lifts the v9 ensemble from 0.7890 to 0.7897 and lowers no set.
6. Four seeds in the ensemble instead of two. 4 x 121 MB in f16 fits the 500 to 800 MB budget; latency doubles to about 125 ms median.

Not tried again: teacher labels (lost three times), beam (never beat greedy), domain MLM and mDeBERTa (lost on eval). Not in this round: plugin context as parser input (the runtime would have to pass the focused plugin; no training row carries one yet) and MBR selection between candidates (needs more members first; the oracle over the two v9 seeds and their ensemble is 51 of 102 against 42).

## Expected gain

On eval (102 rows), from the slices: the conv rule fixes the three finger-combination rows that miss nothing else (05-o, 05-t1, 05-s1). Stage 7 can reach the 10 INV-only rows and the 7 rows where INV and the source both differ; if it fixes half of the first group, that is about 5 rows more. Source verbs and named parameters may add 1 to 3. Expected: 46 to 50 of 102 exact (45 to 49 %), right or one part wrong 77 to 80 of 102. The set is seen, so this number is weaker evidence than v7's.

## Cost

DeepSeek for stage 7, about 1 $. Modal for four runs on an L4, about 2 $ (credits). Baseten: none planned. Spark wrote stages 1 to 5, and its teacher labels lost three times; stage 7 needs a writer that follows `dir` hints, and the DeepSeek pilot did (40 sentences, 0.01 $).

## Selection rule

Copied into `probes/real-eval/RESULT.md` ("Parser v10: selection rule") before training.

## After v10: next levers (2026-11-02)

From the second slice (`probes/real-eval/RESULT.md`, "Gold fix and second slice"), in order of rows at stake per unit of work:

1. Side words anywhere in the clause: `commands.py side_check` accepts the side word after the movement and in a hand-as-subject clause ("my left hand should control X by spreading the fingers"); a stage-8 frame `instrumental` ("X by Y-ing", "X steuern, indem ich Y") with the hand as subject. 8 rows at 0 of 8.
2. One encoding per movement: `conventions.py` folds `S:hand.fist` into `S:hand.openness` with INV flipped (and `eye.closed` into `eye.open`), as `score.py` already does. Needs no new text, only relabelling and a retrain.
3. Parameter name as the only target: an edit form whose only target word is a parameter name, labelled `T:param:other` (or `T:param:macro` for macros) plus the `PNAME` pointer; drop names that are also targets elsewhere ("B Warp") from `param_names`.
4. Direction hints for every parameter target in fan-out edits, not only for low-direction sources.
5. Decoding: conv plus pronoun (lowers no dev set; +3 to +5 calibration rows).

The four-seed ensemble is the strongest reading of the real set (56 of 102 exact, 85 right or one part wrong with pronoun, calibration 35 of 51) and is moved to a separate session to make it as cheap as two seeds (distillation into one student, shared encoder, quantisation).
