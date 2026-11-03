# What is left when the user chips everything (2026-11-03)

Question (Noa, 11-03): with the completion the user picks the gesture (with its hand) and the parameter as chips, so only the words between them ("öffnen", "nach oben", "weniger", "nur bis 80 Prozent") are left to the parser. Are the frames then near perfect, and where not, what can be improved?

## Setting

Frames: the v11 four-seed ensemble in Q8_0 with `rules.py conv,pronoun`, all 5,735 rows of the nine synthetic dev sets of ds-v11c, dumped once by `probes/completion-v11/dump.py` (`data/mapping/pin-hill/rows.jsonl`). Never the real set.

Oracle user (`score.py`, pins through `plugin/parser/examples/pin_eval.rs`): every gold SRC span gets a gesture chip with its gold source, shape and side; every gold TGT span a parameter chip of its gold class (a switch for a toggle) or a target chip of its kind (MIDI, macro). Spans are paired with gold routes as in `probes/completion-v11/score.py` (`assign`); rows where the counts do not pair get fewer chips. Modes: `gpts` (all chips with sides), `gpt` (no sides), `gp` (no target chips).

## Rule (written after the first run h0 and its error list, before any change)

Metric `chip_edit`: route_edit with every `T:param:<class>` written `T:param` and every `T:toggle:<family>` written `T:toggle`, in pred and gold, on the `gpts` frames. A pinned parameter carries its own id, so the class the frame shows is never used after a chip (the resolver binds by the chip). route_edit on the unchanged frames is reported next to it.

A change (pin.rs, or a rule after decoding that only runs where chips exist) is kept when the mean chip_edit over the nine sets rises by at least 0.002, no set loses more than 0.005, and the mean route_edit of the unpinned frames (`base`) does not change (chip-only code paths). Changes are tried one at a time; a rejected change is reverted before the next.

## Runs

`uv run probes/pin-hill/score.py --rows data/mapping/pin-hill/rows.jsonl --name <run>` (needs `cargo build -p synthexis-parser --release --example pin_eval`).

| run | base | gpts route_edit | chip_edit | decision |
|---|---|---|---|---|
| h0: oracle as in completion-v11 | 0.7761 | 0.8205 | 0.8330 | |
| h1: harness fix, a gate (G:) ends the source, so a chip no longer takes the gate's shape or side ("blink ... only while my hand is open" gave the blink chip shape:open) | 0.7761 | 0.8216 | 0.8342 | new baseline |
| C1: pin.rs writes a pinned shape after the side ("S: side: shape:", the order of parser and gold; route_edit compares the unit in order) | 0.7761 | 0.8219 | 0.8345 | rejected (+0.0003) |
| C2: a parameter chip no target takes joins the nearest route that has a source, instead of a route of its own without a source | 0.7761 | 0.8231 | 0.8358 | rejected (+0.0016 < 0.002) |

C2 halves the rows with a wrong route count (111 -> 63) but most of them stay wrong for another reason; it is the same change as completion-v11's P3 (+0.0015 there). Both were reverted by the rule. Noa then took C2 in anyway (11-03): a route without a source never drives anything, and no set lost by it. C2 is in pin.rs since then; C1 stays out.

## Where the rest is (h1, all 5,735 rows pooled)

| rows | share |
|---|---|
| right | 0.851 |
| wrong only in a modifier | 0.054 |
| wrong in structure (sources, targets, routes) | 0.033 |
| wrong in scope (TRACK, PLUGIN, PNAME, REF) | 0.022 |
| wrong in intent (map / modify / none) | 0.012 |
| mixed | 0.029 |

Modifier-only rows by unit: INV 127, amt 60, delta 56, lo 31, hi 30, smooth 21, BIP 16, hold 8.

So with every gesture and parameter chipped, the words between them come out right in about 95 of 100 rows; direction (INV) is the largest single error. Of the INV-only rows, 67 miss an INV and 55 have one too many. Read by hand, about half are ambiguous sentences or gold conventions ("the HP cutoff goes down and makes everything muffled, and when I turn it back the cutoff opens up again": gold without INV; "je höher die rechte Hand, desto weniger Macro": gold without INV), and many are one gesture driving several parameters in opposite directions ("the mod rate rises and the B Warp falls"), where the parser has to put INV on the right one of four routes. A word-list rule for direction would be fitted to that noise; it was not tried.

## What changed instead

The answer line shows what the frame read between gesture and parameter: "hand openness inverted -> Topos / Tone (range 0-80 %, steps, snappy)" (`Route::mod_labels` in `plugin/parser/src/resolve.rs`). A wrong direction or range is then visible before anything is mapped; flipping it with one click belongs to the route diff (map.agent), which is not built.

## Edge cases found on the way

- Two chips for one parser target: the parser wrote fewer targets than the user chipped ("more wobbly ... more dreamy and ring longer"); pin.rs opened a route without a source for each extra chip. Fixed by C2.
- A gate's shape is not the source's: "blink ... only while my hand is open" has `G:hand.shape shape:open` on the blink route; a harness (and a user) must not give the source chip the gate's shape.
- One gesture, parameters in opposite directions: INV is per route, so a fan-out with opposite directions needs one route per direction; pin.rs pins per target and keeps the parser's routes.
- Ranges read as lo or hi ("nur bis 95 Prozent" as lo:0.95 in one gold row, hi:0.95 from the parser).
- Limits: oracle chips with gold classes; synthetic sentences; route_edit compares a unit's tokens in order (C1).

## Do chips make the four-seed ensemble unnecessary? (11-03)

Same harness after C2, frames dumped again with fewer v11 seeds (Q8_0, same rules):

| parser | base (no chips) | chip_edit (all chips) |
|---|---|---|
| four seeds (shipped, about 350 MB, 75 ms median) | 0.7761 | 0.8358 |
| seeds 0 + 1 | 0.7652 | 0.8249 |
| seed 0 alone | 0.7506 | 0.8142 |

The chips do not close the gap: each halving of the ensemble costs about as much with chips as without (one seed -2.2 points with chips, -2.6 without; on the real set one seed had 44 of 102 exact against 66). The four seeds stay; memory is better saved by loading the parser on demand.

## One alignment for pin.rs and the resolver (11-03)

Question (Noa): pin.rs and the resolver each decide which words belong to which target (pin.rs by PNAME cover, class, the only free target, the nearest route; the resolver by the BIO head's name groups). Can both use the same alignment, with less code and the same or better frames?

Built first: `plugin/parser/src/align.rs` holds the resolver's alignment (name groups from the BIO TGT and PNAME pieces, groups per route, pairing with the T: tokens); the resolver uses it unchanged (held-out template top-1 0.828 and real eval 0.681, as before). `pin_spans` adds rule 0: a parameter chip takes the free target whose group its bytes overlap; with fold, the other targets that share that group go (one chipped name, one parameter).

Rule (written before the run): frames dumped again with the BIO head's spans (`rows-bio.jsonl`, same frames). Two chip conditions: the gold class (as above, `gpts`) and a class that matches nothing in the frame (`gptsx`, the case of a real plugin whose param-sense class differs from the parser's). Variants: V0 pin.rs as it is, V1 rule 0 without fold, V2 rule 0 with fold. V1 or V2 is kept when its mean chip_edit with the mismatched class rises by at least 0.005 over V0 and with the gold class falls by at most 0.002; V2 is preferred unless it is more than 0.002 below V1 in either condition. Then V3: the kept variant without the PNAME-cover rule (less code), kept when both conditions stay within 0.002 of it.

Runs (`--modes gpts,gptsx` for V0, `gptsb,gptsxb` for V1 to V3; chip_edit, means over the nine sets):

| variant | chip class = gold | chip class matches nothing | decision |
|---|---|---|---|
| V0: pin.rs (PNAME cover, class, only free, nearest route) | 0.83585 | 0.60800 | |
| V1: rule 0 by words, no fold | 0.83672 | 0.83605 | kept over V0 (+0.0009 / +0.228) |
| V2: rule 0 by words, with fold | 0.83475 | 0.83408 | preferred over V1 (0.00197 below, not more than 0.002) |
| V3: V2 without the PNAME-cover rule | 0.83475 | 0.83408 | kept: identical, the words cover what PNAME did |

V2 against V1 per set (gold class): challenge -0.0054, dev_s7 -0.0044, dev_s4 -0.0024, dev_s5 -0.0024, dev_s8 -0.0023, dev_s2 -0.0007, the other three equal. The fold removes a target the gold keeps when the BIO head joins two names into one group; by the rule V2 stays, V1 is the fallback if real sentences show the loss.

What this says:
- pin.rs found the target of a parameter chip mostly through the class. With a real plugin the chip's class is param-sense's class of that parameter, which often differs from the parser's; then 23 points of chip_edit were lost. By the chip's words (the BIO head's name groups, the same alignment the resolver uses) the loss is gone: 0.834 with and without a matching class.
- One alignment now serves both: `align.rs` is the only place that decides which words name which target; pin.rs lost its PNAME rule (`pname_at`, `target_for_pname`), the resolver lost its private copy. The plugin calls `pin_spans` with the parse's spans.
- Limits: the oracle chips sit on gold TGT spans; the mismatched class is the worst case (no class matches), real chips sit between the two conditions.

After a review of the branch (five reviewers, two skeptics per finding, 22 defects confirmed), the fold was narrowed: a chip folds its group only when it covers the whole group and no other chip touches it, only parameter targets fold, and inside a group the chip takes the slot of its own class first. Same harness, same rows:

| variant | chip class = gold | chip class matches nothing |
|---|---|---|
| V3 (as committed in 29022e1) | 0.83475 | 0.83408 |
| V3 with the narrowed fold (shipped) | 0.83819 | 0.83585 |

The narrowed fold removes V2's losses (challenge 0.8796 -> 0.8869, dev_s7 0.7862 -> 0.7906) and is above V0 and V1 in both conditions.

A second review round of the fixes found that "fold the chip's whole group" still removed targets whose words the BIO head had missed ("Cutoff and resonance" with only "Cutoff" tagged: resonance inherited the group). The rule now folds only targets whose own words lie inside the chip; a target that only inherited the chip's group stays in the frame, and the resolver drops it when it binds the chip's parameter again (`resolve`, a target repeating a chip goes). Frame-level numbers, same rows:

| variant | chip class = gold | chip class matches nothing |
|---|---|---|
| narrowed fold (4941eca) | 0.83819 | 0.83585 |
| own words only (shipped) | 0.83824 | 0.83757 |
