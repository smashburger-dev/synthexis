# Completion and pinning on parser v11 (2026-11-02)

Question from session parser-v11 for Noa: does the sentence-field completion (plugin/complete, plugin/parser/src/pin.rs) have to follow parser v11 and its Q8_0 runs, and can it be tuned to profit from them?

## Selection rule (written before any measurement)

Data: the nine synthetic dev sets of ds-v11c (dev, challenge, dev_s2 to dev_s8). Never the real set (`data/mapping/eval/`) or anything derived from it.

Frames: the v11 four-seed ensemble with Q8_0 weights (`p-v11-s0-q8` to `p-v11-s3-q8`, the session parser-v10-lean reference), greedy, `rules.py conv,pronoun`, dumped once by `dump.py`. On the dev sets Q8_0 and f16 differ by at most 0.7 points per set (`decode/lean-ens-v11-4-q8.json` against `v11dev-ens-v11-4.json`), so the result holds for both.

Simulated user (`complete/examples/sim_v11.rs`):
- Gesture chips: at every gold SRC span whose gold source is known (one source per span, assigned in text order), the user types the span's text character by character after the real text before it. As soon as the completion's top 3 hold a gesture of the gold source (after the fist and eye folds; the shape too for hand.shape), the user picks it. A span where this never happens gets no chip.
- Parameter chips: oracle, one per gold TGT span whose route target is `T:param:<class>` or `T:toggle:*`, with the gold class (synthetic sentences name no plugin, so the parameter list cannot be simulated).
- Pins: the chips at their gold span byte ranges, then `synthexis_parser::pin` on the dumped v11 frame.

Measures, per dev set and as the mean over the nine:
- `pin_GP`: route_edit (`frame_parser.compare`) of the pinned frame with gesture and parameter chips; `pin_G` and `pin_P` with one kind only; `base` the unpinned frame.
- `R3`: share of gold SRC spans with a known source that get a chip (gesture recall in the top 3 while typing the span).
- `F`: share of word starts outside SRC spans where typing the word shows a gesture as ghost text whose source is not in the row's gold frame (false ghosts).

Rule:
1. A change to `pin.rs` is kept when the mean `pin_GP` rises by at least 0.002, the mean `pin_G` and `pin_P` each fall by at most 0.002, and no single dev set loses more than 0.01 on `pin_GP`.
2. A change to the completion's context rules (where gestures are offered) is kept when `R3` rises by at least 1 point and `F` rises by at most 0.5 points, and the mean `pin_GP` falls by at most 0.002.
3. Changes are tried one at a time in the order listed in "Runs"; a rejected change is reverted before the next.
4. `pin_GP` must end at or above `base` on every dev set; a dev set below `base` is reported.

## Order of the changes (written after the dev_s8 smoke run, before the full measurement)

The smoke run on dev_s8 with the committed code (7871f01) showed two things: parameter pins lower route_edit (0.659 -> 0.591) because v11 writes a fan-out as one route with several `T:` targets and pin.rs adds a new route for every chip after the first; and the completion finds only 24 % of the gesture spans, which sit after side words ("die linke Hand"), after "and", "pinch und", "dann", and several words after a trigger ("wenn ich bei den Hats die rechte Faust").

pin.rs:
- P1 pins per target instead of per route (fan-out).
- P2 the fist and eye folds: a gesture chip of hand.fist or eye.closed sets hand.openness or eye.open; INV flips when the chip replaces another source, and stays when the route already has the folded source.
- P3 a parameter chip no target takes joins the nearest route as one more target instead of opening a new route.
- P4 side carry-over: when a gesture chip of a hand source sets the source of a route without a side token, the route takes the side of the nearest earlier route with a hand source and a side.

Context rules (plugin/complete/src/context.rs):
- C1 side words (left, right, linke, rechten, beide ...) are skipped like articles between the trigger and the query.
- C2 possessives (my, mein, meine, meinen ...) count as triggers.
- C3 a trigger anywhere in the last 4 words of the clause counts, not only right before the query.
- C4 "and", "und", "then", "dann", "indem ich", "dass ich" count as triggers.

Added after C1 to C4 were measured, before C5 was measured:
- C5 the positions of C2 and C4 (possessive; "and", "und", "then", "dann", "indem ich", "dass ich") offer gestures in the list only, never as ghost text; ghost text for gestures keeps the C1 positions. Same rule 2.

Added after C5 was measured (and after the harness fix for fan-in, see Runs), before P5 was measured:
- P5 gesture pins per source token instead of per route: v11 writes a fan-in ("smile and move my left hand") as one route with several `S:` tokens. A gesture chip takes the free `S:` token of its own (folded) source; the rest go in order when there are as many free `S:` tokens as chips, else to the nearest route with a free `S:` token; a route without any `S:` counts as one free slot. Rule 1.
- C5 again on top of P5 (C5b), rule 2.

## Runs

Harness fix before the decisions below: the first version gave every SRC span of a row with a fan-in route (several `S:` tokens in one route) the route's first source, so the second chip carried a wrong source. `score.py` now lists every `S:` token; all variants were scored again with the fixed harness, and the decisions are the same as with the first one. Ties in the completion's ranking were broken by HashMap order (R1 moved by 0.001 between runs); `lib.rs` now breaks them by chip id, from c0 on.

Means over the nine dev sets (5,735 rows), `pin_*` and `base` are route_edit:

| step | base | pin_G | pin_P | pin_GP | R3 | R1 | F | decision |
|---|---|---|---|---|---|---|---|---|
| v0 (7871f01) | 0.7761 | 0.7632 | 0.6940 | 0.6834 | 0.2186 | 0.0398 | 0.0686 | |
| P1 per target | 0.7761 | 0.7628 | 0.7948 | 0.7821 | 0.2186 | 0.0401 | 0.0686 | kept (+0.099) |
| P2 fold | 0.7761 | 0.7761 | 0.7948 | 0.7956 | 0.2186 | 0.0409 | 0.0686 | kept (+0.0135) |
| P3 nearest route | 0.7761 | 0.7761 | 0.7960 | 0.7971 | 0.2186 | 0.0401 | 0.0686 | rejected (+0.0015 < 0.002) |
| P4 side carry | 0.7761 | 0.7723 | 0.7948 | 0.7919 | 0.2186 | 0.0393 | 0.0687 | rejected (-0.0037) |
| c0 (P2, ties by id) | 0.7761 | 0.7761 | 0.7948 | 0.7956 | 0.2186 | 0.0422 | 0.0684 | |
| C1 side words | 0.7761 | 0.7763 | 0.7948 | 0.7959 | 0.2383 | 0.0448 | 0.0707 | kept (R3 +2.0, F +0.2) |
| C2 possessives | 0.7761 | 0.7706 | 0.7948 | 0.7893 | 0.5056 | 0.0788 | 0.0883 | rejected (F +1.8) |
| C3 window of 4 | 0.7761 | 0.7750 | 0.7948 | 0.7940 | 0.3089 | 0.0572 | 0.1190 | rejected (F +4.8) |
| C4 and/dann/indem | 0.7761 | 0.7563 | 0.7948 | 0.7756 | 0.3906 | 0.0667 | 0.1247 | rejected (F +5.4) |
| C5 weak = list only | 0.7761 | 0.7558 | 0.7948 | 0.7746 | 0.5982 | 0.0443 | 0.0707 | rejected (pin_GP -0.021) |
| P5 per source token | 0.7761 | 0.7792 | 0.7948 | 0.7987 | 0.2383 | 0.0448 | 0.0707 | kept (+0.0028) |
| C5b = C5 on P5 | 0.7761 | 0.7828 | 0.7948 | 0.8021 | 0.5982 | 0.0443 | 0.0707 | kept (R3 +36.0, F 0, pin_GP +0.0034) |

C5 failed on the first try because the extra gesture chips, all of the right source, went to the wrong place: pin.rs pinned one source per route, so the second chip of a fan-in overwrote the first source or opened a route of its own. P5 fixed that, and the same C5 then passes.

Final state (C5b) per dev set:

| set | n | base | pin_GP | R3 | F |
|---|---|---|---|---|---|
| dev | 805 | 0.8820 | 0.8969 | 0.6322 | 0.0672 |
| challenge | 548 | 0.8577 | 0.8759 | 0.5800 | 0.0672 |
| dev_s2 | 1380 | 0.8428 | 0.8580 | 0.5910 | 0.0628 |
| dev_s3 | 416 | 0.7572 | 0.7668 | 0.5858 | 0.0536 |
| dev_s4 | 823 | 0.8408 | 0.8591 | 0.5401 | 0.0679 |
| dev_s5 | 410 | 0.7512 | 0.7854 | 0.5952 | 0.0762 |
| dev_s6 | 464 | 0.6810 | 0.7241 | 0.6177 | 0.0832 |
| dev_s7 | 449 | 0.7127 | 0.7617 | 0.6540 | 0.0734 |
| dev_s8 | 440 | 0.6591 | 0.6909 | 0.5882 | 0.0846 |

`pin_GP` is above `base` on every set (rule 4).

## What this says

- The completion has to follow v11, and the part that had to change is pin.rs, not the index: the committed pin.rs made v11 frames worse (0.776 -> 0.683 with every chip) because v11 writes fan-out and fan-in as one route with several `T:` or `S:` tokens. Pinning per token and the fist/eye fold turn that into a gain (0.776 -> 0.802 on the mean, +1.5 to +4.9 points per set).
- The Q8_0 runs change nothing for the completion: it never runs the parser, and pin.rs only reads the frame. Which runs the plugin loads stays the parser-runtime session's and Noa's call. The Rust runtime has no `rules.py conv,pronoun` yet; pin.rs folds only the chips' own sources.
- Side carry-over in pin.rs (P4) hurts: v11 carries the side itself, and pin.rs keeps every `side:` token it finds. A test pins that.
- A lone parameter name (v11's `T:param:macro` or `T:param:other` with `PNAME`) is reached by the PNAME rule: the chip under the PNAME span takes that target and sets its own class.
- Gestures: the list (top 3) now finds 60 % of the gold gesture spans while typing them, against 22 % before; ghost text only 4.4 %, because a gesture phrase rarely starts with the words the sentence uses. Ghost text stays strict: a weak position (possessive, "and", "dann", "indem ich") offers gestures in the list only, so false ghosts stay at 7.1 % of the other words.
- Limits: parameter chips are oracle (no plugin in synthetic sentences), the simulated user always opens the list and picks the right gesture as soon as it shows, and the chip keeps the user's words instead of the glossary label. The numbers rank pin.rs and context rules against each other; they are not a forecast for Noa's sentences.

## Files

- `dump.py`: ensemble frames with span bytes per dev row (needs `tools/mapping` of the v11 branch: `rules.py`, `conventions.py`).
- `score.py`: oracle sources and classes, runs `plugin/complete/examples/sim_v11.rs`, scores with `frame_parser.compare`.
- Run: `uv run probes/completion-v11/dump.py --tools <v11 tools/mapping> --runs p-v11-s0-q8,p-v11-s1-q8,p-v11-s2-q8,p-v11-s3-q8 --device mps --out rows.jsonl`, then `uv run probes/completion-v11/score.py --tools <v11 tools/mapping> --rows rows.jsonl --completion data/mapping/completion`.
