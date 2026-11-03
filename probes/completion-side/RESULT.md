# Hand side and finger in gesture completion (2026-11-03)

Noa, 11-03, after typing "opening my fist" in FL: the list offered "open fist" with no way to say which hand. The first build (decided with Noa in this session: default "any hand", sides any/left/right/both, Shift for the hand on a finger row) was replaced the same day by Noa's final decisions, relayed by the picker session and written into `docs/ux-plugin.md` ("Hand side"): no default side and never both hands; Left picks left and Right picks right at any moment while the panel is open; Tab or Enter without a side opens the panel and inserts nothing; a bare "Finger" shows the fingers once a side is picked, Up/Down go through them inside the panel; Esc closes the panel first; the panel always sits right of the list. Only a side named right before the gesture ("die linke Fau") is preset. The pin carries the side (`side:` token after the source); the sentence text stays as typed.

## Measurement (written before the run)

Question: does pinning the side the user picks help or hurt the v11 frames?

Data: the nine synthetic dev sets of ds-v11c, the v11 four-seed ensemble in Q8_0 with `rules.py conv,pronoun`, frames dumped by `probes/completion-v11/dump.py` (same dump as `probes/completion-targets`). Never the real set.

Simulated user (`plugin/complete/examples/sim_v11.rs`, `probes/completion-v11/score.py`): as in completion-v11, plus the gold side of each SRC span (the `side:` token after its source in the gold route). Without `SIM_SIDES` the user leaves every panel at its default (the side the clause names, else any hand); with `SIM_SIDES` the user picks the gold side in the panel whenever the row has one.

Rule: side pins are kept when the mean pin_G and pin_GP with `SIM_SIDES` are at least the values without it, and no dev set loses more than 0.01 on pin_GP. If they fail, the panel stays but its pick is not pinned.

## Runs

| run | pin_G | pin_GP | decision |
|---|---|---|---|
| default panel, nothing pinned unless picked (no SIM_SIDES) | 0.7827 | 0.8019 | |
| gold side picked, default = side named anywhere in the clause | 0.7613 | 0.7802 | fails |

The failure was in the default, not in the pin: a span whose gold has no side got the side named earlier in the same clause ("hook left swipe to eq and circle motion": circle got left), and a user who leaves the panel alone would pin it the same way. 130 rows got worse, 17 better. Changed after this run, before the next: the default side is the one in the two words right before the gesture ("die linke Fau", "my left"), else any hand. Same rule.

| gold side picked, default = side in the two words before | 0.7826 | 0.8018 | fails (dev_s7 -0.0111) |

Two kinds of rows got worse (20 worse, 17 better). The two-word window reached over "und": "meine Hand nach rechts und Fasut ballen" gave the fist "right", where "rechts" is a direction. And gold quirks: "when i lift my right thumb ..." has its side:right after the last route, outside every route, so the right side pinned on the thumb counts as an error. Changed after this run, before the next: the default side is the word right before the gesture, after articles and possessives only. Same rule.

| gold side picked, default = word right before | 0.7853 | 0.8046 | passes (largest loss dev_s4 -0.0049) |
| panel default taken (SIM_SIDES=default) | 0.7817 | 0.8010 | |

The rule asks for the user who picks; a user who only presses Tab takes the default, and that cost 0.0009 on pin_GP. Of the 6 rows it made worse, 5 are a gold convention: one side token after the last route holds for every route ("my right thumb ..., index ..., middle ..."), and the pin added it to the first route as well. The sixth is an older pin.rs gap: "map openness to Signal 19 ... and left fist to reverb": the fist folds to openness and went to the first route with that source, not the nearest. Changed after this run, before the next (pin.rs): a side the frame already holds outside the routes is not pinned again; a gesture chip whose source a route already names goes to the nearest such route. All three modes measured again.

Final code (also "both"/"beide" right before a gesture count as a side, which the article list used to skip; same numbers), means over the nine dev sets:

| user | pin_G | pin_GP | pin_GPT | largest loss per set (pin_GP) |
|---|---|---|---|---|
| never picks a side (nothing pinned) | 0.7827 | 0.8019 | 0.8024 | |
| presses Tab, takes the default | 0.7824 | 0.8018 | 0.8024 | dev_s2 -0.0007 |
| picks the gold side in the panel | 0.7860 | 0.8054 | 0.8059 | dev_s2 -0.0007 |

Rule: passes (gold pick +0.0035 pin_GP, +0.0033 pin_G, no set loses more than 0.0007).

## What this says

- A side the user picks helps the frames; it never has to be guessed. Most gains are where v11 dropped or misplaced the side (dev_s3 +0.0072, dev_s6 +0.0065, dev_s8 +0.0114).
- The default must stay narrow. A side word anywhere in the clause belongs to another gesture as often as not ("hook left swipe ... and circle"); a direction looks like a side ("nach rechts"). Only the word right before the gesture counts; with that, taking the default costs nothing measurable (-0.0001).
- Two pin.rs fixes came out of this: a side the frame already holds for all routes is not repeated, and a folded chip (fist -> openness) goes to the nearest route with that source.
- Limits: synthetic sentences; the simulated user always finds the gesture in the top 3 and picks the gold side. Fingers (the finger axis) are not measured here: the dev sets name their fingers.

## Final decisions (no default side, left or right only)

Same frames, same harness; SIM_SIDES picks what a user of the final field would end up with:

| user | pin_G | pin_GP | pin_GPT |
|---|---|---|---|
| no side pinned (none) | 0.7827 | 0.8019 | 0.8024 |
| only the side named right before the gesture (named) | 0.7824 | 0.8018 | 0.8024 |
| picks the gold side; where the gold has none, the named one or none (gold) | 0.7860 | 0.8054 | 0.8059 |
| always picks a side: gold, else named, else left (forced) | 0.6026 | 0.6161 | |

The forced row is what the final field does to a sentence that names no hand: the user must pick one. The gold of ds-v11c writes no `side:` there; to the parser, its training data and the resolver a missing side means either hand. A forced side is the hand the user meant, but it is a token the gold convention does not have, so route_edit counts it as wrong on every such route (pin_GP 0.8019 -> 0.6161 with "left" as the stand-in pick). This is a convention gap, not a pin error: where the gold names a side, the picked side lifts the frames (gold row). It matters downstream: the resolver must read `side:left` as "only the left hand" and a route without `side:` as before.
