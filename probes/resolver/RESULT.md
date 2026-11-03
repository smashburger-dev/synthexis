# Resolver in Rust: frame + picked plugin -> parameters (2026-11-03)

Question: with the shipped v11 parser and the Rust resolver (`plugin/parser/src/resolve.rs`), how often does a sentence land on the right parameter of the picked plugin?

## Rule (written before the run)

Setting: v11 four-seed ensemble in Q8_0 with the conv,pronoun rules (shipped), BIO head run in Rust, `pin()` with no chips, no aliases. Candidates: the parameters of the row's plugin only (the picked plugin), names and classes from the exported completion file of that plugin (`data/mapping/completion/plugins/*.json`, what the plugin reads first). Placeholder names dropped as in resolve.py. Resolver order: chip > alias > name (sim >= 0.5, NAME_MIN fixed before the run) > class.

Sets:
- Template sentences of `probes/open-vocab-mapping` (`data/mapping/open-vocab/eval-s{0,1,2}.jsonl`, 6 plugins, 3 English and 3 German templates around real parameter names). Seed 0 is the dev set: code may change after looking at it. Seeds 1 and 2 are held out and run once, with the code frozen. Reference: `picker.py` top-1 0.676 on seeds 1 and 2 (p-v3 parser, Python BIO span, picked plugin only).
- Real sentences (`data/mapping/eval/expanded.jsonl`, rows whose plugin has an exported file and that name parameters). Calibration split: may be looked at. Eval split: run once at the end with frozen code. Never trained on, and no rule or threshold is set from the eval split.

Metrics:
- template top-1: the parameter bound to the frame's first T:param/T:toggle target is one of the row's gold names (picker.py's metric; a frame without such a target is a miss).
- real: gold recall, the share of the row's gold parameters among the bound parameters of the frame; row exact, the set of bound parameters equals the gold set.

Decision: if held-out template top-1 is below 0.676, the gap gets explained before the resolver ships; otherwise it ships as is. Numbers on the eval split are reported, not used to change the code.

## Runs

`python3 probes/resolver/run.py template 0`, `real calibration`, `template 1 2`, `real eval` (needs `cargo build -p synthexis-parser --release --example resolve_eval`). Thermal has no exported completion file; its candidates are the harvest names with the known.tsv labels.

Dev-set changes, made after looking at seed 0 and the calibration split, before any held-out run:

| step | seed 0 top-1 | calibration recall |
|---|---|---|
| first build: TGT spans paired with targets, NAME_MIN 0.5 | 0.697 (99 rows, Thermal missing) | |
| TGT and PNAME pieces joined into one name when at most one word lies between; a name match >= 0.8 folds the extra targets of its route (v11 reads "pan mid env attack" as amp.pan + env.attack) and turns a macro or CC guess into that parameter ("slot 18 target"); "turn", "up", "down", "hoch", "runter" are filler; Thermal from the harvest | 0.866 | 0.700 |
| a name of another class needs >= EXACT_MIN when the plugin has parameters of the target's class ("cutoff" took "Cutoff Rand" over "Filter 1 Freq"); on a class tie the master/main/global/output control wins, the shorter name first ("mix knob" -> MASTER DRY/WET). EXACT_MIN tried at 0.85, 0.9, 0.95 on both dev sets: 0.857 / 0.800, 0.824 / 0.825, 0.807 / 0.800; 0.85 kept | 0.857 | 0.800 |

Held out, code frozen (run once each):

| set | rows | result | reference |
|---|---|---|---|
| template seeds 1 + 2 | 238 | top-1 0.811 | picker.py 0.676 (p-v3, Python span, same rows) |
| real eval split | 92 | gold recall 80/135 = 0.593, row exact 39/92 | |

Decision: 0.811 >= 0.676, the resolver ships.

Errors on the eval split (looked at after the frozen run):
- 8 of the 135 gold names are not in the candidate lists: the real-set gold for ShaperBox uses harvest names ("CrushShaper bits") that the exported ShaperBox file spells otherwise, so ShaperBox scores 0/12 for the harness, not only for the resolver.
- "B warp": resolve.py's cover skips words under 3 letters, so "b" never matched and the class "other" took Arp Rate. "macro two": number words never met the digit in "MACRO 2". v11's name-only classes (`other`, `macro`) blocked a name of another class.
- The parser leaves the target out ("the pad to open up": only `TRACK =pad`), reads it as MIDI ("range in the arpeggiator" -> `T:midi.prop:arp.style`), or as the wrong class (distortion toggle -> Compressor On; "Höhen" -> Bass by eq gain).

Fixes after the eval was seen (so the numbers below are no longer held out): a single letter counts when it equals a name word, number words one to eight (and German) count as digits, the classes `other` and `macro` never outvote a name.

| set | before | after |
|---|---|---|
| template seed 0 (dev) | 0.857 | 0.866 |
| calibration | 0.800 | 0.800 |
| template seeds 1 + 2 | 0.811 | 0.828 |
| real eval split | 0.593 (39/92 exact) | 0.681 (49/92 exact) |

## What this says

- Against the picked plugin the name decides almost every binding (234 of 236 on the template rows), as resolve.py found; the class matters on real sentences, where people say "the mix" or "it opens up" (43 of 130 bindings on the eval split).
- The Rust chain beats the Python probe by 15 points on the same held-out template rows; most of it is joining the BIO fragments and folding v11's split targets.
- Timeless stays weak (14/40): its 1,007 names are modulation-matrix slots ("XLFO 6 Step 9 Random") and the BIO head drops the words between the digits.
- Real sentences fail more on the parser than on the resolver: a missing or MIDI target cannot be bound.
- Limits: candidates come from the exported or harvested names, not from FL's own list; classes for unlabelled names (param-sense) are not run here.

## In FL with Noa (2026-11-03, build cad85a0 + this branch)

- "when i open my fist i want the drive" on Topos, "fist" picked as a left-hand chip: `I:map <route> S:hand.openness side:left T:param:fx.drive </route>`, resolved on Topos (55 params): "left hand openness -> Topos / Amp Drive", 179 ms for parse and resolve; the line shows below the sentence field.
- Classes no longer wait for a frame of the synthexis window: between "fl: 55 parameters of Topos" and "picker: classes for Topos" 7 ms (the classification itself took 7.1 ms), against 246 ms with 6.9 ms of classification before (`fl::Bridge::on_names`, the completion loader thread classifies).
