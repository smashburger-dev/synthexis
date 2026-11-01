# Parser v9: what Noa's sentences say, and the plan

Result (same evening): `docs/session-2026-11-01-parser-v9.md`. Noa took conventions 1 to 4 of the plan, not the fixed filter-cutoff default for effect words, and had no time for a fresh dictated set (step 5).

Written 2026-11-01 after Noa read the gold-route diagrams (https://claude.ai/artifact/UuASRSfAqEbYaviStxhYr7) and named his doubts per sentence. Each doubt below was checked against the training data (ds-v6p train_all, 42,922 map sentences), the synthetic dev sets, and probes on the shipped ensemble (p-v7a-s0 + p-v7a-s1). All of this looked at eval sentences, so eval is no longer a clean judge for anything v9 changes (see "Judge").

## Checked

| Noa's doubt | Measured | Verdict |
|---|---|---|
| "and" is not learned and breaks context (4, 9, 16, 20, 34) | 27,108 training sentences contain and/und, 18,681 of them with several routes. But 15,958 training routes carry two or more targets ("fist controls reverb and delay") and 3,417 two or more sources, so "A up and B down" (16) and "detune by tilting and open it up by raising" (4) are pulled into one route. | Partly. Not the word, the label habit: one route with several targets is the most common multi-target form in training. |
| Later clauses lose the context (6, 9, 20, 27) | 6: "my index finger should be position four" gets a MIDI chord, 9: "middle finger to be diverge" a chord, 20: "soll länger werden" note length instead of reverb, 27: the right hand copies the left hand's up/down. All are clauses that leave out the target or the gesture and mean the earlier one. | Yes, as ellipsis: a clause that omits the target is filled with whatever the model guesses. |
| Period or commas split the sentence (6, 28; voice adds commas) | Without any punctuation eval drops 37 -> 34 and calibration 22 -> 21 (12 and 8 rows change); internal periods turned into commas change nothing; synthetic dev without punctuation 190 -> 189 of 226. Lowercase costs 37 -> 34. | No. Punctuation is not the cause. |
| Spread fingers missing from the ontology (1) | `hand.spread` exists, and on synthetic dev it is read right (1 of 54 sentences confused with hand open). Sentence 1 fails on the target: "the pad opens up" gives I:none (not a command) with confidence 0.89; "the pad filter opens up" gives a filter switch. | No, the gap is the target. A sound described by its effect ("opens up", "brighter") is not read as a parameter. |
| Finger combination read as single fingers (5) | 73 routes with a finger mask in all of training; sentences with "at the same time / gleichzeitig" and two fingers are labelled 12 times as two single fingers and 4 times as a mask. At runtime a single-finger route fires whenever that finger is up, also inside the combination, so Noa's worry holds. | Yes, too little data and inconsistent labels. |
| "turn on" should mean switch (26) | 38 training sentences use turn on / switch on / einschalten / anschalten / bypass (26 of them with a switch target). "turn on the reverser" gives reverb mix (the subword "rever" pulls toward reverb); "enable the reverser" gives a reverb switch. | Yes, the phrase is almost absent from training. |
| Pinch is never meant as pinch strength (12, 21, 27, 32) | Training: pinch strength alone 2,184 routes, pinch strength next to another source 179, pinch as gate 1,330. On synthetic dev a pinch gate is read as a source in 4 of 47 sentences; on Noa's sentences in 12 and 21, while 27 and 32 keep the gate and lose the movement (turning). | Label convention question for Noa, plus phrasing gap ("pinch and drag", "pinching and turning"). |
| Spread associated with hand open (32) | Not on synthetic dev (1 of 54). In 32, "my left hand should control X by spreading" adds "hand open, left" for the words "left hand". | The subject "my left hand" is read as a gesture of its own. |
| The model is too sure of wrong readings (6) | Sentence 6 has confidence 0.26. On eval, rows under 0.5 are right 6 of 37 times, rows at 0.9 or more 9 of 15; AUROC 0.69 eval, 0.85 calibration. | Confidence is a usable "unsure" signal for the UI, not a reliable one. |
| Plugin context would help (29, 30) | The parser never sees the plugin; parameter choice is the resolver's job and is not scored. 29 and 30 meant ShaperBox 3, not Portal (gold metadata, route score unchanged). | Untested until the resolver exists (parser-runtime session). |

Gold questions Noa raised: 19 is read as `I:preset preset:chord_hand side:right`, which is what Noa says the plugin would offer, so it can become an accepted reading. 9 and 18 ask for Serum's arpeggiator patterns, which Serum does not expose; replace or mark them. 17 counts as "one part wrong" although the parameter (Arp Range) is wrong, because parameters are not scored at all.

## Reading

The misses are not random. They sit where the synthetic generator was more explicit than people are: people name effects instead of parameters ("opens up", "turn on"), leave out what a later clause refers to, describe muscles ("pinch and drag") and finger combinations in everyday words. The command filter dropped exactly those sentences for an unmarked slot, and teacher labels for them were too noisy (v8t). So v9 should write such sentences for known labels instead of labelling found ones.

## Plan

1. Label conventions, decided by Noa before any data (each one is a rule applied to all training and dev labels, not only to new rows):
   - pinch next to a movement is a gate; pinch strength only when the sentence says how far or how hard the pinch is;
   - two targets that move in different directions are two routes; one target per route when the sentence gives it its own direction or value;
   - "at the same time / gleichzeitig / zusammen" with several fingers is one finger mask;
   - turn on / einschalten / anschalten / aktivieren / bypass is a switch;
   - an effect verb on a sound ("opens up", "geht auf", "brighter", "heller") targets the class default (filter cutoff for open/close and bright/dark).
2. Data by construction (DeepSeek V4.1 Flash writes, labels come from the edit): for each convention a batch of edits whose sentences must use the cue, plus ellipsis batches where the second clause leaves out the target or the gesture ("thumb is position one, index finger four"). Validation checks that the cue words appear and that no slot is named twice. About 1,000 to 2,000 sentences per phenomenon, about 1 to 2 $.
3. Retrain the v7a recipe on ds-v6p plus the batch and the relabelled conventions, two seeds, ensemble as now.
4. Runtime: below confidence 0.5 the plugin shows the reading as chips with a hint ("one gesture and one target per part works best"), Noa's "i" idea.
5. Judge: v9 is chosen on synthetic dev sets, then judged on a fresh set Noa dictates after step 1 is fixed (30 to 50 sentences, by voice as in the demo, never shown to any model before scoring). The current eval stays as a second number, marked as seen.
