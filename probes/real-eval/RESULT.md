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

## Open

- Noa reviews the derived rows: do they sound like him, mean the original plus the listed change, and name the right plugin and parameter? Plugin context is inferred for all 34 originals; 5 are unknown, and their rows note the candidates.
- Label the 34 originals (Noa checks). Derived labels follow from the delta.

## Rules

- No row is used for training or for writing prompts.
- Stage-4 and stage-5 prompts come from calibration findings only. Before training, every generated sentence is compared with `expanded.jsonl` (word 5-grams, closest-pair ratio), and hits are dropped.
- Needs first seen in eval groups are scored separately if stage 4 covers them.
