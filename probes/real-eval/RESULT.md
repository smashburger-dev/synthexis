# RESULT: real-sentence set, Noa's sentences plus derived ones (2026-10-31)

Question (Noa): he wrote 34 sentences with the plugins open (Serum 2 and six effects, many routes per sentence, plugin-specific names). Can they be extended to 100-150 in his style, by translating them, swapping single variables and reusing his request forms on other routes?

Answer: 150 rows, 34 by Noa and 116 derived. Labels are needed for the 34 originals only. Each derived row gets its label from its original plus the recorded change. The sentences stay under `data/mapping/eval/` (not committed).

## What was built

`data/mapping/eval/expanded.jsonl` has one row per sentence. Fields:
- id, src (line in `eval.txt`), group, split
- kind: orig, translate, swap, style
- lang, by (noa or claude), text
- delta: what changed against the original
- plugin and params: the intended target, inferred and checked against the harvest
- note

| kind | rows | rule |
|---|---|---|
| orig | 34 | Noa's lines, verbatim with typos |
| translate | 34 | each original in the other language; German in Noa's Denglisch (der Delay, pinchen, bypassen) |
| swap | 65 | one slot changed, wording kept: finger, hand side, source gesture, target parameter, values, direction, track |
| style | 17 | the content of one of Noa's sentences in the request form of another, within one split only |

`uv run probes/real-eval/check.py` verifies:
- the originals
- the group and split of every row
- that every named parameter exists in the harvest of its plugin
- that no text is duplicated

All checks pass. It also writes `data/mapping/eval/review.md` for Noa.

## Split

- One group per original. Near-duplicate originals share a group: hi-hat rolls, cut filters, bit reducer. That gives 31 groups.
- The split is stratified by the original's language and seeded (`random.Random(0)`). A third of the groups go to calibration:
  - calibration: 10 groups, 51 rows
  - eval: 20 groups, 99 rows

  The split was fixed before any parser run on these sentences.
- All rows of a group stay in one split.
- Border check:
  - 18 of 99 eval rows share a word 5-gram with calibration, mostly gesture phrases.
  - The closest pair has a word-sequence ratio of 0.55.
  - Two calibration style rows reached 0.69 against eval rows. They were moved to calibration-only request forms.

## How to score

1. route_edit per kind and language. The originals are the headline. Derived rows measure robustness to the changed slot.
2. Intervals come from a bootstrap over groups, not rows. Rows of one group are correlated, so the effective sample is closer to 20 eval groups than to 99 rows.
3. Consistency, which needs no labels: compare parse(derived) against parse(original) with the delta applied.
   - A translation must give the same frame.
   - A side swap may change only the side.

   This is a metamorphic test set in the sense of CheckList (Ribeiro et al., ACL 2020).
4. End to end: parser plus resolver against the row's params, given the plugin's parameter list.

## Measured on the harvest while building

- XY pads named as parameters exist in 6 of 405 VST3 plugins. Portal's pad is not named and needs a table.
- Value-to-text varies by parameter:
  - Serum 2 A WT Pos reads 1 .. 256 and A Warp Mode reads its mode names, so numbers and names convert.
  - Arp Range and Arp Rate read -100.0 .. 100.0, so octaves and note values do not convert from the harvest.
- Serum 2 has no arpeggiator pattern parameter.

These findings and the request types missing from the ontology are in `probes/mapping-pipeline/SPEC_PROPOSAL.md`, section "demo requests".

## Parser runs (2026-10-31)

Exact route match per row (`score.py`). Claude drafted the gold for the 34 originals and Noa has not checked it yet, so some misses may be gold errors (for example switch or plain parameter for a bypass).

| run | calibration all | calibration orig | eval all | eval orig | eval route F1 |
|---|---|---|---|---|---|
| v3 s0 | 12/51 | 4/12 | 16/99 | 5/22 | 0.242 |
| v3 s1 | 18/51 | 5/12 | 14/99 | 4/22 | 0.213 |
| v5 s0 | 16/51 | 5/12 | 16/99 | 4/22 | 0.273 |
| v5 s1 | 16/51 | 5/12 | 14/99 | 6/22 | 0.253 |

v5 and v3 are level. Two seeds of one recipe differ as much as the two recipes do, and the eval split has only 20 groups.

Needs first seen in eval groups (any finger mask, values, step, ramp toggle): none of their originals is exactly right with either v5 seed. Seed 1 gets 2 of 4 rows of the two-value request. v5 emits the new pieces (VALUE pointers, a finger pair where v3 had none, one finger per value) but not the whole route.

Error types on the 51 calibration rows (v5 s0, v3 s0 in brackets):
- target kind wrong: 27 (29)
- sources wrong: 28 (26)
- pinch read as a source where it holds another movement: 8 of 9 rows (8)
- number of routes wrong: 15 (20)
- invert wrong: 6 (13)
- no target at all: 10 (6)

Reading: dev route_edit is 0.83, exact match on real sentences is 0.14 to 0.35. Stage 4 put the missing types into the synthetic text, and the real-set score did not move. The real sentences are longer, often describe both directions of one movement, and use pinch as a hold condition. A next batch may be shaped by calibration findings (rule below). More synthetic text of the same kind is not expected to close the gap.

## Decode-time rules (2026-11-01)

`tools/mapping/normalize.py`, measured with `uv run probes/decode-rules/measure.py` on the saved dev predictions of p-v5-s0 and p-v5-s1 and on the calibration rows only:

| rule | dev sets, both seeds (won / lost) | calibration s0 | calibration s1 |
|---|---|---|---|
| none | | 16/51 | 16/51 |
| pinch (S:hand.pinch next to another source becomes G:hand.pinch) | +0 / -13 | 17/51 | 16/51 |
| pronoun (a later route without a target restates an earlier one or takes its target) | +0 / -0 | 16/51 | 19/51 |

The pinch rule costs dev sentences where pinch and a movement both drive the target, so it stays off. The pronoun rule never fires on synthetic text and gains 0 and 3 calibration rows; it is kept for the plugin runtime. Neither closes the gap, so the next step changes the data.

## Training-label noise found on the way

Of 3,891 training sentences with a toggle target, 1,569 have no switching word ("my fist does the distortion"). They come from stages 1 to 3, where toggle targets reached Spark as "midi cc <family>" (fixed for stage 4). They teach the parser that a plain effect name is a switch, the error behind "reverb bigger" read as a reverb switch. `project_labels.py --drop-toggle` drops them from the training files; dev sets stay as they are.

Measured with one seed (p-v5t-s0 on ds-v5t against p-v5-s0): rows predicted with a toggle on calibration fall from 5 to 2, but calibration drops from 16 to 14 of 51, and dev route_edit on the 755 dev sentences without a toggle gold falls from 0.861 to 0.842. The dropped rows carry more than the wrong label, so the drop is not used for v6.

## Parser run v6 (2026-11-01)

v6 adds stage 5, shaped by the calibration findings above (`probes/parser-errors/RESULT.md`, section 9). Scored once on eval.

| run | calibration all | calibration orig | eval all | eval orig | eval route F1 | eval orig route F1 |
|---|---|---|---|---|---|---|
| v3 s0 | 12/51 | 4/12 | 16/99 | 5/22 | 0.242 | 0.295 |
| v3 s1 | 18/51 | 5/12 | 14/99 | 4/22 | 0.213 | 0.254 |
| v5 s0 | 16/51 | 5/12 | 16/99 | 4/22 | 0.273 | 0.286 |
| v5 s1 | 16/51 | 5/12 | 14/99 | 6/22 | 0.253 | 0.312 |
| v6 s0 | 19/51 | 5/12 | 25/99 | 7/22 | 0.347 | 0.419 |
| v6 s1 | 21/51 | 6/12 | 18/99 | 5/22 | 0.339 | 0.444 |

Paired exact match, both seeds pooled (McNemar on rows, 95 % interval from a bootstrap over groups):

| comparison | split | won / lost | p | exact match diff | group bootstrap |
|---|---|---|---|---|---|
| v6 vs v5 | calibration | 10 / 2 | 0.039 | +7.8 points | [-0.8, +20.0] |
| v6 vs v5 | eval | 22 / 9 | 0.029 | +6.6 points | [+0.5, +14.7] |
| v6 vs v3 | calibration | 12 / 2 | 0.013 | +9.8 points | [+2.2, +19.0] |
| v6 vs v3 | eval | 24 / 11 | 0.041 | +6.6 points | [-3.2, +18.6] |

Error types on the calibration rows (v6 s0, v5 s0 in brackets):
- target kind wrong: 22 (27)
- sources wrong: 20 (28)
- pinch read as a source where it holds another movement: 6 of 9 rows (8)
- number of routes wrong: 11 (15)
- invert wrong: 8 (6)
- no target at all: 7 (10)

Needs first seen in eval groups (finger mask, values, step, ramp; 5 groups, 21 rows): v6 gets 3 of 21 rows with either seed and 1 of 5 originals (the two-value request), v5 got 0 and 2 of 21. The routes that need these pieces are still mostly wrong.

Reading: the style batch moved the real-sentence score where stage 4 did not. Eval exact match goes from 15 % to 22 % (seed mean), route F1 from 0.26 to 0.34. Exact match on real sentences stays far below dev (0.83), so most of Noa's long sentences still need the chips to fix one or two routes. Gold is still Claude's draft; Noa's check can move every number in this section.

## Ceiling: Spark parses the set directly (2026-11-01)

`probes/real-eval/spark_parse.py`: Muse Spark Open (reasoning high) gets the frame language, the ontology sources and families, and 22 few-shot examples from the synthetic training set (ds-v6p train_cat; toggle examples only with a switching word). No real sentence is in the prompt. Batches of 25 sentences, 9 requests, 0.29 $. The replies are a measurement only and never go into training.

| run | calibration all | calibration orig | eval all | eval orig | eval route F1 | eval orig route F1 |
|---|---|---|---|---|---|---|
| Spark | 40/51 | 9/12 | 62/99 | 14/22 | 0.667 | 0.636 |
| v6 s0 | 19/51 | 5/12 | 25/99 | 7/22 | 0.347 | 0.419 |
| v6 s1 | 21/51 | 6/12 | 18/99 | 5/22 | 0.339 | 0.444 |

Against both v6 seeds on eval: 91 won, 10 lost, exact match +41 points, group bootstrap +19 to +61. The prompt rules on pinch and on both directions came from calibration errors, so eval is the fair number: 63 % exact.

Error types on eval (Spark): target kind wrong 20 of 99, sources wrong 10, pinch read as a source 8 of 12, route count wrong 0, no target 0. Needs first seen in eval groups: 7 of 21 rows, 1 of 5 originals (the finger mask).

Reading: the gold and the frame language are learnable from the same spec the parser trains on; a large model gets almost two thirds of Noa's sentences exactly right and two thirds of the routes. The gap between v6 (22 %) and Spark (63 %) is the small model trained from scratch, not the task or the labels.

## Teacher check: Spark against DeepSeek V4.1 Flash (2026-11-01)

`probes/real-eval/teacher_check.py`, same prompt as the ceiling check, 100 map sentences from the synthetic dev_s5 split (DeepSeek processes in China and only sees synthetic text). Spark wrote dev_s5 itself, which may favour it.

| teacher | exact | cost |
|---|---|---|
| Spark, reasoning high | 71/100 | 0.185 $ |
| DeepSeek, thinking off | 67/100 | 0.004 $ |
| DeepSeek, thinking high | 48/100 (one batch of 25 unparsed) | 0.061 $ |

Where Spark and DeepSeek without thinking agree (68 of 100 sentences), 59 match the gold: 87 % against 71 % for Spark alone. Agreement of two different teachers is the label filter for teacher data.

## Parser v7 (2026-11-01)

Runs in `probes/parser-errors/RESULT.md`, section 10. Selection rule fixed before eval: best calibration, ties to the smaller model. It picked p-v7-s0 (31/51); mmBERT base got 25/51. Then eval was scored once for every arm:

| run | calibration | eval all | eval orig | eval route F1 | eval right or one part wrong |
|---|---|---|---|---|---|
| v6 s0 | 19/51 | 25/99 | 7/22 | 0.347 | 49/99 |
| v6 s1 | 21/51 | 18/99 | 5/22 | 0.339 | 51/99 |
| v7 s0 (encoder + teacher rows) | 31/51 | 23/99 | 5/22 | 0.360 | 54/99 |
| v7 s1 | 30/51 | 22/99 | 7/22 | 0.319 | 57/99 |
| v7a s0 (encoder only) | 26/51 | 30/99 | 6/22 | 0.371 | 72/99 |
| v7a s1 | 25/51 | 32/99 | 8/22 | 0.377 | 64/99 |
| v7b s0 (mmBERT base + teacher rows) | 25/51 | 29/99 | 5/22 | 0.436 | 67/99 |
| Spark (ceiling) | 40/51 | 62/99 | 14/22 | 0.667 | 82/99 |

"One part wrong": only the gestures or only the targets of the sentence differ from the gold.

v7 with teacher rows gains 10 calibration rows and nothing on eval (v7 against v6, both seeds: 21 won, 19 lost, p 0.87). The cause is the teacher prompt: its rules on pinch, both directions and chained targets were written from calibration errors, and the teacher labels carry them, so calibration stopped being an independent selection set for these runs. No real sentence reached training (0 exact matches; the 267 training sentences that share a word 6-gram with the real set share gesture phrases such as "when i open my left hand", almost all from stages 1 to 3, written before the real set; teacher rows reach a ratio of 0.55 at most).

The encoder alone carries the gain. v7a seed 0 was looked at on eval before choosing it, so seed 1 was trained afterwards as a check of the recipe: 32/99. Both v7a seeds against both v6 seeds on eval: 30 won, 11 lost, p 0.004, exact match 22 % -> 31 %, group bootstrap -1.6 to +22 points. Right or one part wrong rises from 50 % to 68 %. p-v7a-s0 ships (seed 0 by the usual rule).

Needs first seen in eval groups (finger mask, values, step, ramp): v7 gets 2 and 3 of 21 rows, no original besides the ramp one with seed 1.

Reading: a pretrained encoder narrows the gap to Spark from 41 to 31 points exact and from 32 to 14 points on "right or one fix away". Teacher labels need a prompt that no selection set has shaped.

## Parser v8: selection rule (written 2026-11-01, before any v8 arm was scored on the real set)

Arms (`docs/parser-v8-session-prompt.md`): lever 1 decodes p-v7a-s0 and p-v7a-s1 differently (seed ensemble, grammar-constrained beam); lever 2 trains the v7a recipe on an encoder after domain MLM; lever 3 trains the v7a recipe on ds-v6p plus teacher rows from a neutral DeepSeek prompt; lever 4 (another encoder family) runs only if 1 to 3 leave room.

1. Selection score: mean route_edit over the six synthetic dev sets of ds-v6p (dev, challenge, dev_s2 to dev_s5). A training arm's score is the mean of its two seeds. Calibration does not select: every arm trains on ds-v6p, whose stage 5 was written from calibration findings, so calibration rows are reported next to eval and nothing more.
2. A training arm replaces v7a when its two-seed score is at least 0.005 above v7a's (0.7933; the two v7a seeds differ by 0.004). Between arms that pass, the higher score wins; within 0.003, the one without extra data or parameters.
3. Decoding of the chosen arm: the seed ensemble when it lifts the score by at least 0.005 over the mean of its single seeds; beam width and length penalty only where they lift it over greedy of the same members.
4. Eval is scored once, at the end, for every arm, with `probes/real-eval/score.py`. A gain is claimed only from two seeds: two single runs against both v7a seeds, or for an ensemble a second ensemble of two more seeds against the first's baseline. Paired McNemar on rows and a bootstrap over groups.
5. "Right or one part wrong" (the 68 % of v7a): the route multiset equals the gold after removing all targets, or after removing all sources, gates and INV. This reproduces every number of the v7 table above.

## Parser v8 (2026-11-01)

Runs and dev numbers in `probes/parser-errors/RESULT.md`, section 11. What the rule picked on the dev sets:

- Lever 1: the greedy ensemble of two v7a seeds lifts the dev score from 0.7933 to 0.8022, and a second pair (s2+s3) from 0.7912 to 0.8018. Beam (width 4 or 8, with or without length normalisation) does not beat greedy, alone or in the ensemble.
- Lever 2 (domain MLM, `v8m`): 0.7908, below v7a.
- Lever 3 (clean teacher rows, `v8t`): 0.7851, below v7a.
- Lever 4 (mDeBERTa-v3-base, `v8d`): 0.7979, +0.0046 against v7a, under the 0.005 bar of rule 2. Its ensemble reaches 0.8085 on dev but needs about 1.1 GB in f16.

The rule chose the v7a ensemble s0+s1, greedy. Then eval was scored once for every arm (`uv run probes/real-eval/arms.py table ...`):

| run | calibration | eval all | eval orig | eval route F1 | eval right or one part wrong |
|---|---|---|---|---|---|
| v7a s0 (shipping until now) | 26/51 | 30/99 | 6/22 | 0.371 | 72/99 |
| v7a s1 | 25/51 | 32/99 | 8/22 | 0.377 | 64/99 |
| v7a s2 | 25/51 | 35/99 | 7/22 | 0.418 | 67/99 |
| v7a s3 | 27/51 | 26/99 | 5/22 | 0.390 | 63/99 |
| ensemble v7a s0+s1 (chosen) | 22/51 | 37/99 | 7/22 | 0.431 | 73/99 |
| ensemble v7a s2+s3 (second pair) | 28/51 | 29/99 | 6/22 | 0.401 | 68/99 |
| v8m s0 (domain MLM) | 22/51 | 32/99 | 6/22 | 0.360 | 62/99 |
| v8m s1 | 27/51 | 27/99 | 6/22 | 0.345 | 67/99 |
| v8t s0 (clean teacher rows) | 26/51 | 20/99 | 6/22 | 0.286 | 59/99 |
| v8t s1 | 29/51 | 27/99 | 6/22 | 0.331 | 61/99 |
| v8d s0 (mDeBERTa-v3-base) | 25/51 | 28/99 | 7/22 | 0.358 | 55/99 |
| v8d s1 | 24/51 | 23/99 | 5/22 | 0.359 | 56/99 |
| ensemble v8d s0+s1 | 25/51 | 25/99 | 6/22 | 0.358 | 56/99 |
| Spark (ceiling) | 40/51 | 62/99 | 14/22 | 0.667 | 82/99 |

Paired exact match on eval (`arms.py paired`):

| comparison | won / lost | p | exact match diff | group bootstrap |
|---|---|---|---|---|
| both ensembles against their own two seeds | 17 / 8 | 0.11 | +2.3 points | [+0.2, +4.9] |
| ensemble s0+s1 against v7a s0 and s1 | 13 / 1 | 0.002 | +6.1 points | [+1.5, +11.3] |
| v8m against v7a (s0-s0, s1-s1) | 15 / 18 | 0.73 | -1.5 points | [-11.5, +7.4] |
| v8t against v7a | 5 / 20 | 0.004 | -7.6 points | [-16.8, +0.5] |
| v8d against v7a | 10 / 21 | 0.07 | -5.6 points | [-13.3, +1.5] |

Reading: none of the four levers reaches 40 % exact and 75 % right or one part wrong. The chosen ensemble gets 37 % and 74 %, but the second pair of seeds gets 29 % and 68 %, so the ensemble gain on real sentences is about 2 points and not established (p 0.11); on the dev sets it repeats (+0.9 and +1.1 points). Calibration moves the other way (22/51 against 25 and 26). The three training levers lose on eval. Clean teacher rows cost 7.6 points (p 0.004): teacher labels hurt even without calibration-shaped rules, so v7's failure was label noise, not only the prompt (the neutral prompt labels 59 of 100 dev_s5 sentences right, and where both passes agree 52 of 81). mDeBERTa is the best single run on dev and among the weakest on eval: the synthetic dev sets rank encoder families badly (rank correlation of dev score and eval exact over the ten single runs 0.67). Seed spread on eval is large: four v7a seeds give 26 to 35 of 99.

The ensemble ships by the rule (f16, 2 x 121 MB, 62 ms median and 138 ms p95 per sentence on one CPU thread, dev score unchanged in f16). p-v7a-s0 alone stays the fallback when RAM or latency count more than about 2 points.

## Parser v9: changed set and selection rule (written 2026-11-01, before any v9 run exists)

Noa reviewed the gold as diagrams (https://claude.ai/artifact/UuASRSfAqEbYaviStxhYr7) and decided label conventions (`docs/parser-v9-plan.md`). The real set changes accordingly; the v8 files stay as `expanded.v8.jsonl` and `gold-routes.v8.json`:
- 19 also accepts the chord-hand preset on the right hand (`score.py` counts a preset as one route `P:<preset>`); 18 also accepts stepping synthexis' arp style (Serum exposes no arp pattern); 9 and 18 point to synthexis' arpeggiator.
- 28 is removed. 35 and 36 replace it (written by Claude, eval): one movement drives two and four Serum 2 parameters in opposite directions, each with a translation and two swaps. Eval now has 102 rows, calibration 51.
- 29 and 30 meant ShaperBox 3 (CrushShaper), not Portal; route gold unchanged.

This set was looked at closely to write the conventions, so it is a seen set for v9: its numbers are reported, they do not select, and a gain on it is weaker evidence than v7 and v8 had. Noa has no time to dictate a fresh set.

Rule:
1. Labels: every dataset passes `tools/mapping/conventions.py` (pinch next to a movement is a hold; fingers shown together are one finger state), so v7a is re-scored on the same labels.
2. Selection score: mean route_edit over the seven synthetic dev sets of ds-v9c (dev, challenge, dev_s2 to dev_s6), for the greedy two-seed ensemble of each arm. v9 replaces the v7a ensemble when it is at least 0.005 higher, and it may not lose more than 0.005 on the mean of the six older sets (dev_s6 is written in the new style and favours v9).
3. Real set: every arm scored once at the end on the changed set, with the paired test against the v7a ensemble and both single seeds.

## Parser v9 (2026-11-01)

Runs in `probes/parser-errors/RESULT.md`, section 12. By the rule above v9 replaces the v7a ensemble: on the seven dev sets of ds-v9c its ensemble scores 0.789 against 0.737, on the six older sets 0.817 against 0.801, on the new stage-6 set 0.621 against 0.351. Then the changed real set was scored once (`arms.py`, files `preds-g2-*.jsonl`; v7a re-predicted on the changed set, Spark not re-run):

| run | calibration | eval all | eval orig | eval route F1 | eval right or one part wrong |
|---|---|---|---|---|---|
| v7a s0 | 30/51 | 30/102 | 6/23 | 0.386 | 75/102 |
| v7a s1 | 25/51 | 32/102 | 8/23 | 0.388 | 68/102 |
| ensemble v7a s0+s1 (v8, shipping until now) | 26/51 | 37/102 | 7/23 | 0.425 | 77/102 |
| ensemble v7a s2+s3 | 29/51 | 29/102 | 6/23 | 0.391 | 70/102 |
| v9 s0 | 31/51 | 41/102 | 8/23 | 0.545 | 78/102 |
| v9 s1 | 30/51 | 31/102 | 6/23 | 0.439 | 71/102 |
| ensemble v9 s0+s1 (ships) | 30/51 | 42/102 | 8/23 | 0.537 | 75/102 |

| comparison | won / lost | p | exact match diff | group bootstrap | route F1 diff | group bootstrap |
|---|---|---|---|---|---|---|
| v9 seeds against v7a seeds | 28 / 18 | 0.18 | +4.9 points | [-3.3, +13.4] | +0.104 | [+0.009, +0.213] |
| v9 ensemble against v7a ensemble | 12 / 7 | 0.36 | +4.9 points | [-8.3, +17.9] | +0.111 | [-0.005, +0.227] |

Reading: the v9 ensemble passes 40 % exact on this set (42 of 102, 41 %), and right or one part wrong stays at 74 % (75 of 102, v7a 77). The exact gain is not established (p 0.18 over two seeds); the route F1 gain of 10 points is, over two seeds. Of Noa's originals, 6 (each finger its own value on one parameter), 26 ("turn on" read as a switch) and 35 (one movement, two parameters in opposite directions) are now exactly right, 1 is now read as a command; 8 and 12 got worse, 36 (four parameters) is still wrong. The set was looked at to write the conventions, so these numbers are weaker evidence than v7's; a set Noa dictates fresh would settle it.

## Parser v10: selection rule (written 2026-11-02, before any v10 run exists)

Plan and the slices behind it: `docs/parser-v10-plan.md`. The failure slices of the v9 ensemble on this set were read to write stage 7, so the set is seen for v10: its numbers are reported and do not select.

1. Labels: ds-v10c = ds-v9 plus stage 7, through `project_labels.py` and `conventions.py`; stage-6 and stage-7 rows twice in train_cat. The seven older dev sets are the same sentences as in ds-v9c.
2. Selection score: mean route_edit over the eight synthetic dev sets of ds-v10c (dev, challenge, dev_s2 to dev_s7), greedy two-seed ensemble per arm. v10 replaces v9 when its score is at least 0.005 higher and it loses no more than 0.005 on the mean of the seven older sets (dev_s7 is written in the new style and favours v10).
3. Decoding: the conv rule (`rules.py`) applies to the chosen ensemble when it lowers no dev set. Four seeds replace two when the four-seed ensemble scores at least 0.005 higher on the eight-set mean.
4. Real set: scored once at the end for every arm (v9 ensemble with and without conv, v10 seeds, v10 ensembles), paired against v9: seeds against seeds, ensemble against ensemble, McNemar and group bootstrap as before.

## Parser v10 (2026-11-02)

Runs in `probes/parser-errors/RESULT.md`, section 13. By the rule above v10 replaces v9: on the eight dev sets of ds-v10c the ensemble of v10 s0+s1 scores 0.7730 against 0.7542, on the seven older sets 0.7921 against 0.7890. The conv rule lowers no dev set and applies. The four-seed ensemble scores 0.7729 on dev, no gain over two seeds, so it is not chosen. Then the real set was scored once (`arms.py`, files `preds-g2-*.jsonl`, `-conv` with the rule):

| run | calibration | eval all | eval orig | eval route F1 | eval right or one part wrong |
|---|---|---|---|---|---|
| ensemble v9 s0+s1 (shipping until now) | 30/51 | 42/102 | 8/23 | 0.537 | 75/102 |
| ensemble v9 s0+s1, conv | 30/51 | 45/102 | 9/23 | 0.556 | 76/102 |
| v10 s0 | 27/51 | 49/102 | 13/23 | 0.580 | 82/102 |
| v10 s1 | 27/51 | 46/102 | 10/23 | 0.532 | 79/102 |
| v10 s2 | 26/51 | 37/102 | 7/23 | 0.476 | 71/102 |
| v10 s3 | 30/51 | 46/102 | 10/23 | 0.595 | 73/102 |
| ensemble v10 s0+s1 | 25/51 | 46/102 | 10/23 | 0.553 | 78/102 |
| ensemble v10 s0+s1, conv (ships) | 25/51 | 47/102 | 11/23 | 0.559 | 79/102 |
| ensemble v10 s2+s3, conv | 31/51 | 50/102 | 10/23 | 0.624 | 79/102 |
| ensemble v10 s0 to s3, conv (not chosen) | 30/51 | 54/102 | 13/23 | 0.617 | 84/102 |

| comparison | split | won / lost | p | exact match diff | group bootstrap |
|---|---|---|---|---|---|
| v10 s0, s1 against v9 s0, s1 | eval | 42 / 19 | 0.004 | +11.3 points | [+1.0, +23.3] |
| v10 s0, s1 against v9 s0, s1 | calibration | 12 / 19 | 0.28 | -6.9 points | [-17.0, +4.4] |
| four v10 seeds against both v9 seeds | calibration | 49 / 73 | 0.04 | -5.9 points | [-12.9, +3.4] |
| v10 ensemble, conv, against v9 ensemble | eval | 18 / 13 | 0.47 | +4.9 points | [-14.1, +23.8] |
| v9 ensemble, conv, against v9 ensemble | eval | 3 / 0 | 0.25 | +2.9 points | [0.0, +9.6] |
| four-seed against two-seed v10 ensemble, both conv | eval | 9 / 2 | 0.07 | +6.9 points | [0.0, +15.9] |
| four-seed against two-seed v10 ensemble, both conv | calibration | 5 / 0 | 0.06 | +9.8 points | [+2.3, +15.5] |

Reading: the shipped ensemble gets 47 of 102 eval rows exact (46 %, v9 41 %) and 79 right or one part wrong (77 %, v9 74 %). Over two seeds the eval gain is established (+11 points, p 0.004); between the two ensembles it is not (p 0.47). Stage 7 fixed what it aimed at: sources said in their low direction with a falling target (29, 30, 13: 8 rows) and both directions named (08: 3 rows). It broke two things. Calibration falls by 6 to 7 points (closing the fist while a cutoff goes down now gets an INV, 23; the opposite misses, 24; drag across the screen with a pinch now adds a route, 21). And "B warp" lost its target in all five rows of group 07: stage 7 put "B Warp" into the parameter-name list, where the name is a scope word next to a target, so the parser learned it is not the target. The four-seed ensemble is the best reading of the real set on both splits (54 of 102, 84 right or one part wrong, calibration 30), but the dev sets show no gain and this set is seen, so it does not ship by the rule.

## Gold fix and second slice (2026-11-02, after the v10 scoring)

`score.py derive` swapped fingers only in `F:` tokens, so the derived rows 13-s2 and 29-s1 ("Zeigefinger kippen") kept the thumb tilt of their originals. Fixed; every number above this section was scored before the fix. Re-scored (pronoun = `normalize.py` pronoun rule after conv; it lowers no dev set, 0.7897 with and without, but it was looked at on this set, so it does not count as selected):

| run | calibration | eval all | eval orig | eval route F1 | eval right or one part wrong |
|---|---|---|---|---|---|
| ensemble v9 s0+s1 | 30/51 | 43/102 | 8/23 | 0.543 | 75/102 |
| ensemble v9 s0+s1, conv, pronoun | 35/51 | 46/102 | 9/23 | 0.579 | 78/102 |
| v10 s0 / s1 / s2 / s3 | 27 / 27 / 26 / 30 | 51 / 47 / 38 / 48 | 13 / 10 / 7 / 10 | 0.592 / 0.538 / 0.482 / 0.608 | 82 / 79 / 71 / 73 |
| ensemble v10 s0+s1, conv (ships) | 25/51 | 49/102 | 11/23 | 0.572 | 79/102 |
| ensemble v10 s0+s1, conv, pronoun | 28/51 | 49/102 | 11/23 | 0.584 | 80/102 |
| ensemble v10 s0 to s3, conv | 30/51 | 56/102 | 13/23 | 0.630 | 84/102 |
| ensemble v10 s0 to s3, conv, pronoun | 35/51 | 56/102 | 13/23 | 0.643 | 85/102 |

v10 s0, s1 against v9 s0, s1 on eval: 43 won, 19 lost, p 0.003, +11.8 points, bootstrap [+1.0, +24.6]. The pronoun rule wins 3 and 5 calibration rows (group 20, "and when I spread my fingers it should get longer") and changes no eval row.

Misses of the four-seed ensemble (conv), read by part as for v10 (calibration 21, eval 46 after the fix), point to four gaps in how the training data is made:

1. The hand as subject: "my right hand should control the reverb size by pinching and turning". 0 of 8 rows right (groups 27, 32). The command validator drops a sentence when a source's side word is not inside its mark or among the 7 words before it; in stage 7, 558 of 1,251 such drops had the side word in the sentence, after the movement ("Finger spreizen mit der linken Hand") or further before it. The form Noa uses is filtered out of every stage.
2. A parameter name as the only target ("macro 2 sinkt", "B warp"). In all 858 training rows that name a macro by number, the name is a `PNAME` pointer next to a family target; never the target alone. The parser then leaves the route without a target (16-o, 16-s1, 07-t1) or drops the route.
3. Two encodings for one movement: closing the fist is labelled `S:hand.fist` 693 times and `S:hand.openness` (low direction) 297 times. `score.py` folds them (fist = openness with INV flipped), training does not, so INV has to be learnt twice; the remaining fist-closing INV misses are 23 and 24 (4 calibration rows).
4. A target that rises while another falls on one movement (16, 35: 4 eval rows miss only the INV). Stage 7 told targets their direction only when the source was said in its low direction.

Smaller: a preset described by what it does ("every finger should be a chord", 19, 5 calibration rows; training names presets only by name); arpeggiator range and rate read as synthexis' MIDI generator where the gold has Serum's parameter (17), and pinch with both axes read as two routes where the gold has the XY pad (12): both are label questions for Noa.

## Open

- Noa reviews the derived rows: do they sound like him, mean the original plus the listed change, and name the right plugin and parameter? Plugin context is inferred for all 34 originals; 5 are unknown, and their rows note the candidates.
- Label the 34 originals (Noa checks). Derived labels follow from the delta.

## Rules

- No row is used for training or for writing prompts.
- Stage-4 and stage-5 prompts come from calibration findings only. A run trained on labels from such a prompt is not selected on calibration (section "Parser v7"). Before training, every generated sentence is compared with `expanded.jsonl` (word 5-grams, closest-pair ratio), and hits are dropped.
- Needs first seen in eval groups are scored separately if stage 4 covers them.

## Parser v11: selection rule (written 2026-11-02, before any v11 run exists)

Plan and the gaps behind it: `docs/parser-v11-plan.md`. The four gaps come from the second slice of the v10 four-seed misses on this set, so the set is seen for v11: its numbers are reported and do not select.

1. Labels: ds-v11c = the ds-v10 sources plus stage 8 (`cmd-s8`, and the stage-6 and stage-7 side drops accepted again under the widened side check, `cmd-s6r`, `cmd-s7r`), through `project_labels.py` and `conventions.py`, which now also folds `S:hand.fist` into `S:hand.openness` and `S:eye.closed` into `S:eye.open` with the route's INV flipped. Stage 6 to 8 rows twice in train_cat. The eight older dev sets are the same sentences as in ds-v10c; v10 is re-scored on the new labels.
2. Selection score: mean route_edit over the nine dev sets of ds-v11c (dev, challenge, dev_s2 to dev_s8), greedy four-seed ensemble, decoded with `conv,pronoun`. v11 replaces v10 when its score is at least 0.005 higher than the v10 four-seed ensemble under the same decoding and it loses no more than 0.005 on the mean of the eight older sets (dev_s8 is written in the new style and favours v11).
3. Real set: scored once at the end for the v11 seeds and the v11 four-seed ensemble (`predict.py --rules conv,pronoun`), `arms.py table`, and `arms.py paired` against `g2-ens-v10-4-conv` (ensemble against ensemble) and the v10 seeds (seeds against seeds), McNemar and group bootstrap as before. Gold questions 17 and 12 stay open for Noa and are not changed.
