# Completion in the sentence field (prototype, 2026-11-01)

Noa's idea from 2026-11-01: while the user types a mapping sentence, the field completes parameters and gestures like an IDE. What gets picked this way is no longer guessed by the parser. Parser v9 gets 41 % of the real sentences exactly right, and most misses have a wrong gesture; which concrete parameter is meant is the resolver's job, and the resolver is not built. A chip fixes both.

## What it does

- Ghost text after the cursor shows the best item. Tab accepts it. It only appears from 2 typed characters, when the typed text is a prefix of the label or of the glossary phrase that led to it, and when the score is at least 0.8. A fuzzy match ("frq") never shows as ghost text, only in the list.
- Arrow down opens the top 3 by score; arrows move, Enter or Tab picks, Esc closes. The list opens in 150 ms with a cubic ease-out. Space never accepts.
- An accepted item becomes a chip: its label goes into the text with a background, and the chip carries an id (`param:<plugin id>#<index>` or `gesture:<source>[/<shape>]`). Typing inside a chip turns it back into text; a backspace into a chip removes the whole chip.
- Enter with the list closed sends the sentence and its chips to the parser worker. The log shows the parser's frame and the pinned frame.
- Settings > "Completion in the sentence field" switches it off; the choice is saved with the plugin state, as is the picked plugin.

Two kinds of items:

1. Parameters of the picked plugin, matched by their name and through the glossary phrases of their param-sense class: "cut" offers "Filter 1 Freq" through "Cutoff", "Filter auf" through "Filter aufmachen". A ctl.switch parameter becomes a toggle target, any other a param target.
2. Gestures from the glossary phrases of the ontology sources and hand shapes ("spr" offers "spread amount", "Fau" offers "Faust"). Only where a gesture is named: sentence start, or after "when I", "if I", "wenn ich", "sobald ich", "with", "mit", "by", "follow" and similar, articles skipped ("wenn ich die Fau"). Query spans never start with an article or a trigger word, so "the res" at sentence start is the query "res" and offers parameters, not gestures.

## Where the code is

| Part | File |
|---|---|
| Export of the index data | `tools/mapping/export_completion.py` writes `data/mapping/completion/` (vocab.json, plugins/*.json) |
| Engine | crate `plugin/complete` (synthexis-complete): `fuzzy.rs` score, `index.rs` prefix index, `context.rs` query and position rules, `lib.rs` ranking, `doc.rs` text with chips, `data.rs` loader |
| Field | `plugin/synthexis/src/sentence.rs`, placed in `editor.rs`; settings in `params.rs` |
| Pinning | `plugin/parser/src/pin.rs`, called in `plugin/synthexis/src/assistant.rs` after decoding |

The plugin reads the data from `$SYNTHEXIS_COMPLETION_DIR`, else `~/Library/Application Support/synthexis/completion` (copy `data/mapping/completion/` there; install.sh does not do it yet, same as for the parser runs). Without data the field still works as a plain sentence line.

## Engine

- Index: every word start of every key (phrase or parameter name), folded (lowercase, ß as ss, punctuation as space) and sorted. Words that share a prefix form one range, found by binary search; this is a trie flattened into an array. A keystroke scores only the keys that have a word starting with the query's first letter.
- Score, after VS Code's fuzzyScore: the query's letters in order, the first one at a word start. Bonuses for the key start (8), a word start (5) and a letter right after the previous match (5); a jump over skipped letters costs 3. Divided by the query's score against itself, so a prefix match is 1.0, plus up to 0.05 for short keys.
- The query is the last 1, 2 or 3 words before the cursor inside the clause and after the last chip; the best span wins ("Filter au" as two words).
- Rank = match + class prior (up to 0.05, log of the label count over all harvested plugins) + recent picks (up to 0.1, last 32 picks) + context: a switching word in the clause ("schalte", "toggle", "an", "aus", "bypass") lifts switch parameters by 0.15, else they lose 0.05; system and unsafe parameters lose 0.2; a gesture in gesture position gains 0.05; a glossary-phrase match counts 0.85 of a name match.
- Speed (release build, M1 Pro, Serum 2 with 2,622 parameters, 2,682 gesture phrases, 2,079 class phrases): data load 10 to 13 ms on its own thread, keystroke 1.7 ms median, 2.9 ms p95, 3.2 ms max over 103 keystrokes. The unit test at the same sizes asserts p95 under 25 ms unoptimised. It runs on the GUI thread, never on the audio thread, and never calls the neural parser.

## Pinning

`synthexis_parser::pin(items, pins)` takes the decoded frame and the chips as byte ranges of the submitted sentence:

- A parameter chip goes to the route whose PNAME span overlaps it, else to the route whose target has the chip's class, else to the only route with a param or toggle target when one chip is left. That route's target becomes `T:param:<class>` (`T:toggle:` for a switch, keeping the parser's effect family if it chose a toggle) and the route carries the parameter id, printed as `@<id>` after the target.
- A gesture chip goes to the route that already has its source; the rest go in sentence order when there are as many free routes as gesture chips, else each to the free route nearest to it (by its spans and pinned parameter). The route's `S:` becomes the chip's source; a shape chip also sets `shape:`.
- A chip no route takes gets a route of its own; `I:none` then becomes `I:map`.

Checked with the shipped v9 ensemble on four synthetic sentences (`plugin/parser/tests/pin_live.rs`, runs when `SYNTHEXIS_PARSER_DIR` is set): every parameter chip lands in a route, including where the parser chose the wrong class (`fx.eq.freq` for "Filter 1 Freq") or left the target out (only `PNAME =filter 1`).

## Tests

- `plugin/complete`: 31 tests on synthetic vocabularies and sentences (never the real sets in `data/mapping/eval/`): fuzzy score, prefix index, query spans, gesture positions, switching words, ranking, recent picks, chip text edits, latency.
- `plugin/parser/src/pin.rs`: 13 tests on hand-built frames.
- `plugin/synthexis/src/sentence.rs`: 5 headless egui tests that send key events through the field: Tab accepts and keeps the focus, arrows and Enter pick from the list, Esc dismisses, space never accepts, Tab with nothing to accept inserts nothing, chips become pins.
- Checked by hand in the standalone build: plugin picker, ghost text, the list, chips. Esc and the second Tab fix were checked only by the headless tests: the window sat on another Space for the second round.

## Open

- Learning from picks: recent picks only add a fixed bonus that decays by position. A per-user count of (typed prefix, picked id), saved with the plugin state, would let "cut" rank "Filter 1 Freq" over "Cutoff Rand" after a few picks. Weights are hand-set on synthetic sentences; nothing is tuned on Noa's sentences.
- Plugin picker: a stand-in combo over the exported plugins. The export holds 10 demo plugins; `--plugin` adds more, so the combo lists plugins that are not in the FL project (Noa, 11-03). The FL side is measured in `probes/fl-target-picker/RESULT.md`: the FL device script lists every mixer track, slot and plugin in 0.4 ms and the parameter names of one plugin in 7-20 ms (drop index >= 4096, FL's MIDI CC entries); with FL in front `OnRefresh` reports the clicked plugin (`getActiveEffectIndex` = (track, slot), form ID = channel for generators). Missing: a channel from the device script into the plugin, so the combo shows the live project and follows FL focus.
- Ambiguous glossary phrases show twice ("Faust" as hand.fist and as the fist shape; "spread my fingers" also under hand.openness). The list shows the source next to each item; the glossary's ambiguous list could pick one.
- Phrases with negative polarity ("Finger zusammen", "Hand öffnen") pin their source without setting INV; the parser still reads the words and decides.
- A gesture chip that is meant as a gate ("only while I make a fist") is pinned as the route's source.
- Values and tracks get no completion yet (VALUE, TRACK, PLUGIN spans).
- Ghost text shows only when the cursor is at the end of the line; mid-line edits use the list.
- Visuals are plain egui: chip background from the selection colour, ghost text in the weak text colour. The final look comes from Figma.
