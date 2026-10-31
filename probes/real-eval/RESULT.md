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

## Open

- Noa reviews the derived rows: do they sound like him, mean the original plus the listed change, and name the right plugin and parameter? Plugin context is inferred for all 34 originals; 5 are unknown, and their rows note the candidates.
- Label the 34 originals (Noa checks). Derived labels follow from the delta.

## Rules

- No row is used for training or for writing prompts.
- Stage-4 prompts come from calibration findings only. Before training, every generated sentence is compared with `expanded.jsonl` (word 5-grams, closest-pair ratio), and hits are dropped.
- Needs first seen in eval groups are scored separately if stage 4 covers them.
