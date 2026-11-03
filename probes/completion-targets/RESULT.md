# Target completion: catch-all classes and MIDI targets (2026-11-03)

Noa, 11-03, after trying the list in FL: lower-scored alternatives are in now (163aa26), and two things should get better before the Figma pass. Typing "res" on Serum 2 lists Swing and A Phase (the glossary phrase "the gap before the reverb starts" belongs to the class `other`, and the engine offers every `other` parameter for it). MIDI targets (note, chord, pattern, CC) and macros never show, although the ontology has them and the glossary has 360 target phrases.

## Measurement (written before any run)

Data: the nine synthetic dev sets of ds-v11c (dev, challenge, dev_s2 to dev_s8). Never the real set (`data/mapping/eval/`) or anything derived from it.

Simulated user (`plugin/complete/examples/sim_targets.rs`, rows from `targets.py`): every gold TGT span whose target token is known (TGT spans and `T:` tokens in the same count, else one per route) is typed character by character after the real text before it, with the 8-item list of the sentence field. Parameter spans run against one of the ten demo plugins that has a parameter of the gold class (round robin, so every such plugin gets spans); toggle spans against one with a `ctl.switch` parameter; every other span against the ten in turn.

Measures, as the mean over the nine sets:
- `R8_P`, `R8_M`, `R8_C`: share of parameter and toggle spans (P), MIDI note, chord, pattern and CC spans (M), macro spans (C) whose list holds an item of the gold kind at some prefix (the gold class for parameters, a switch for toggles, the target kind otherwise).
- `R1_P`, `R1_M`: the same for the ghost text (first item, ghost-worthy).
- `K_P`: characters typed until the first hit, over the P hits.
- `J`: share of the shown rows (lists of 2 or more items from 2 typed characters, the ones the field opens) while typing P spans that are parameters of `other`, `system` or `unsafe` reached through a glossary phrase, not by name.
- `FG`: share of P spans where a MIDI or macro item is the ghost text at some prefix.

Rule:
1. D1 (no phrase expansion for the catch-all classes `other`, `system`, `unsafe`) is kept when `J` falls and `R8_P` falls by at most 0.5 points.
2. M1 (MIDI note, chord, pattern, CC and macro items from the glossary's target phrases) is kept when `R8_M` rises by at least 10 points, `R8_P` and `R1_P` fall by at most 0.5 points each, and `FG` stays at or below 1 %.
3. M2 (pinning those items: the chip's target kind replaces the kind of the target token it covers or sits nearest to) is kept when the mean route_edit with gesture, parameter and target chips (`probes/completion-v11` harness, dumped v11 frames) is at least the one with gesture and parameter chips only, and no set loses more than 0.01.
4. Changes are tried in this order; a rejected change is reverted before the next.

## Runs

Added after M1 failed on `FG`, before measuring them: M1b offers the target items in the list only, never as ghost text (like the weak gesture positions). M1c is M1b with the target items 0.1 below their match score. Same rule 2; M1c is only tried when M1b fails.

Added after M1c failed on `R1_P`, before measuring it: M1d is M1c plus one rule: when the first item is a target item and a later item could be ghost text (not weak, a prefix of 2 or more characters, score at least 0.8), that item moves to the first row, so the ghost text is the one without target items. Same rule 2.

Added after M2 was measured, before measuring it: with target items the gesture recall of the v11 harness (top 3 while typing a gesture span) is 0.5948 against 0.5982 without them. M1e offers no target items where a gesture is named (the strong gesture positions: sentence start, "when I", "wenn ich" ...). Kept when that recall is back at 0.5982 or higher and `R8_M` falls by at most 1 point.

Added after M1e failed (the recall stayed at 0.5948), before measuring it: M1f offers no target items at any gesture position, strong or weak (after a possessive, "and", "und", "dann", "indem ich"). Same rule as M1e.

## Runs

Means over the nine dev sets. P: 5,255 parameter and toggle spans; M: 1,072 MIDI spans; C: 292 macro spans; X: 196 spans whose gold class is a catch-all class; 503 spans of classes no demo plugin has are skipped.

| step | R8_P | R1_P | K_P | J | FG | R8_M | R8_C | decision |
|---|---|---|---|---|---|---|---|---|
| B0 (163aa26) | 0.9321 | 0.7078 | 2.75 | 0.0852 | 0 | 0 | 0 | |
| D1 catch-all phrases off | 0.9340 | 0.7128 | 2.67 | 0 | 0 | 0 | 0 | kept |
| M1 target items | 0.9280 | 0.6781 | 2.79 | 0 | 0.3864 | 0.9727 | 1.0 | rejected (FG) |
| M1b list only | 0.9280 | 0.6781 | 2.79 | 0 | 0 | 0.9727 | 1.0 | rejected (R1_P -3.5) |
| M1c list only, -0.1 | 0.9319 | 0.6877 | 2.77 | 0 | 0 | 0.9659 | 1.0 | rejected (R1_P -2.5) |
| M1d M1c + ghost row | 0.9319 | 0.7138 | 2.77 | 0 | 0 | 0.9659 | 1.0 | kept |

D1 first ran with the catch-all gold spans inside P: R8_P fell from 0.9295 to 0.9101, which rule 1 rejects. Those spans counted any parameter of `other` as a hit (Swing for "predelay"), so they were split off as X (R8_X 0.8392 -> 0.2462 with D1) after that run, and B0 and D1 were measured again. The split was made after seeing the result; with it D1 passes.

Pinning on the dumped v11 frames (four seeds, Q8_0, `rules.py conv,pronoun`, dumped again with `probes/completion-v11/dump.py`; without target items the numbers equal that probe's final state: pin_GP 0.8021, R3 0.5982). T8: share of gold MIDI and macro spans whose 8-item list offers the gold kind.

| step | pin_G | pin_GP | pin_GPT | T8 | R3 (top 3) | decision |
|---|---|---|---|---|---|---|
| D1 | 0.7828 | 0.8021 | | | 0.5982 | |
| M1d + M2 target pins | 0.7827 | 0.8019 | 0.8024 | 0.979 | 0.5948 | M2 kept (+0.0005, no set loses) |
| M1e no targets at strong positions | 0.7827 | 0.8019 | 0.8024 | 0.9776 | 0.5948 | rejected (R3 not back) |
| M1f no targets at any gesture position | 0.7827 | 0.8019 | 0.8024 | 0.9155 | 0.5950 | rejected (R3 not back, R8_M -7.7) |

pin_GPT per set against pin_GP: challenge +0.0018, dev_s6 +0.0022, the other seven equal.

The R3 loss does not come from gesture positions: target items reach the merged list through the other query spans of the same keystroke. R3 measures the old 3-row list. With the 8-row list the field shows now, gesture recall is 0.6224 without and 0.6215 with target items (-0.09 points), pin_GP 0.8030 either way and pin_GPT 0.8035.

## What this says

- Kept: D1, M1d, M2. On the synthetic sentences the 8-item list now offers the right MIDI kind for 97 % of the MIDI spans and a macro for 100 % of the macro spans (0 before); parameter recall and the ghost text stay where they were, and no list shows catch-all rows reached through a phrase.
- Target words must not take the ghost text: short glossary phrases ("fire chords", "pad chords", "Moll Akkord") start like parameter words, and as ghost text they would have replaced the parameter on 39 % of the parameter spans.
- Pinning the target kind barely moves route_edit (+0.0005): v11 already writes the MIDI kind right in most rows; the chip mainly fixes the rows where it read a MIDI phrase as a parameter.
- Limits: synthetic sentences only, the simulated user picks as soon as the gold kind shows, parameters run against the ten demo plugins only. Real sentences may name MIDI targets in words the glossary lacks, so R8_M is likely optimistic.

## Files

- `targets.py`: gold TGT spans with their target kind, from the dev sets.
- `plugin/complete/examples/sim_targets.rs`: the simulated user and the measures above.
- Run: `uv run probes/completion-targets/targets.py --data <main data/mapping> > targets.jsonl`, then `plugin/target/release/examples/sim_targets <completion dir> < targets.jsonl`. Pinning: `probes/completion-v11/score.py` (reads `tk` spans and reports pin_GPT and T8).
