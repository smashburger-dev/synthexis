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
2. Gestures from the glossary phrases of the ontology sources and hand shapes ("spr" offers "spread amount", "Fau" offers "Faust"). Only where a gesture is named. Strong positions (list and ghost text): sentence start, or after "when I", "if I", "wenn ich", "sobald ich", "with", "mit", "by", "follow" and similar, articles and side words skipped ("wenn ich die linke Fau"). Weak positions (list only, never ghost text): after a possessive ("i want my mou") and after "and", "und", "then", "dann", "indem ich", "dass ich" ("pinch und dre"). Query spans never start with an article, a side word or a trigger word, so "the res" at sentence start is the query "res" and offers parameters, not gestures.

## Where the code is

| Part | File |
|---|---|
| Export of the index data | `tools/mapping/export_completion.py` writes `data/mapping/completion/` (vocab.json, plugins/*.json) |
| Engine | crate `plugin/complete` (synthexis-complete): `fuzzy.rs` score, `index.rs` prefix index, `context.rs` query and position rules, `lib.rs` ranking, `doc.rs` text with chips, `data.rs` loader |
| Field | `plugin/synthexis/src/sentence.rs`, placed in `editor.rs`; settings in `params.rs` |
| Pinning | `plugin/parser/src/pin.rs`, called in `plugin/synthexis/src/assistant.rs` after decoding |

The plugin reads the data from `$SYNTHEXIS_COMPLETION_DIR`, else `~/Library/Application Support/synthexis/completion` (`plugin/install.sh` links `data/mapping/completion/` and the parser runs in `data/mapping/runtime/` there, from the main checkout). Without data the field still works as a plain sentence line.

## Engine

- Index: every word start of every key (phrase or parameter name), folded (lowercase, ß as ss, punctuation as space) and sorted. Words that share a prefix form one range, found by binary search; this is a trie flattened into an array. A keystroke scores only the keys that have a word starting with the query's first letter.
- Score, after VS Code's fuzzyScore: the query's letters in order, the first one at a word start. Bonuses for the key start (8), a word start (5) and a letter right after the previous match (5); a jump over skipped letters costs 3. Divided by the query's score against itself, so a prefix match is 1.0, plus up to 0.05 for short keys.
- The query is the last 1, 2 or 3 words before the cursor inside the clause and after the last chip; the best span wins ("Filter au" as two words).
- Rank = match + class prior (up to 0.05, log of the label count over all harvested plugins) + recent picks (up to 0.1, last 32 picks) + context: a switching word in the clause ("schalte", "toggle", "an", "aus", "bypass") lifts switch parameters by 0.15, else they lose 0.05; system and unsafe parameters lose 0.2; a gesture in gesture position gains 0.05; a glossary-phrase match counts 0.85 of a name match.
- Speed (release build, M1 Pro, Serum 2 with 2,622 parameters, 2,682 gesture phrases, 2,079 class phrases): data load 10 to 13 ms on its own thread, keystroke 1.7 ms median, 2.9 ms p95, 3.2 ms max over 103 keystrokes. The unit test at the same sizes asserts p95 under 25 ms unoptimised. It runs on the GUI thread, never on the audio thread, and never calls the neural parser.

## Pinning

`synthexis_parser::pin(items, pins)` takes the decoded frame and the chips as byte ranges of the submitted sentence:

Chips pin tokens, not routes: since v11 one route can drive several targets (fan-out, "resonance and cutoff") and listen to several sources (fan-in, "smile and move my left hand").

- A parameter chip takes the free target next to the PNAME span that overlaps it, else a free target of its class (any toggle for a switch), else the only free param or toggle target when one chip is left. That target becomes `T:param:<class>` (`T:toggle:` for a switch, keeping the parser's effect family if it chose a toggle) and carries the parameter id, printed as `@<id>` after it. A lone parameter name (v11's `T:param:macro` or `T:param:other` with `PNAME`) goes by the PNAME rule.
- A gesture chip takes the free `S:` token that already names its movement; the rest go in sentence order when there are as many free `S:` tokens as chips, else each to the nearest route with a free `S:` token (a route without any `S:` has one slot). The token becomes the chip's source; a shape chip also sets the `shape:` of that token.
- Sources are folded like v11's `conventions.py`: a hand.fist chip sets `S:hand.openness`, eye.closed sets `S:eye.open`. INV flips with the fold when the chip replaces another movement and stays as the parser set it when the token already names the same movement.
- `side:` tokens stay as the parser wrote them; pin.rs does not carry sides over (v11 does that itself, and carrying in pin.rs scored worse).
- A chip nothing takes gets a route of its own; `I:none` then becomes `I:map`.

Measured on parser v11 (four seeds, Q8_0, `rules.py conv,pronoun`) over the nine synthetic dev sets of ds-v11c with a simulated user (`probes/completion-v11/RESULT.md`): route_edit 0.776 without chips, 0.683 with the first pin.rs, 0.802 now; above the unpinned frame on every dev set. The gesture list finds 60 % of the gold gesture spans while they are typed (22 % before), with false gesture ghosts on 7.1 % of the other words.

Checked with the shipped v9 ensemble on four synthetic sentences (`plugin/parser/tests/pin_live.rs`, runs when `SYNTHEXIS_PARSER_DIR` is set): every parameter chip lands in a route, including where the parser chose the wrong class (`fx.eq.freq` for "Filter 1 Freq") or left the target out (only `PNAME =filter 1`).

## Tests

- `plugin/complete`: 32 tests on synthetic vocabularies and sentences (never the real sets in `data/mapping/eval/`): fuzzy score, prefix index, query spans, gesture positions, switching words, ranking, recent picks, chip text edits, latency.
- `plugin/parser/src/pin.rs`: 18 tests on hand-built frames (fan-out, fan-in, fold, side kept).
- `plugin/synthexis/src/sentence.rs`: 5 headless egui tests that send key events through the field: Tab accepts and keeps the focus, arrows and Enter pick from the list, Esc dismisses, space never accepts, Tab with nothing to accept inserts nothing, chips become pins.
- Checked by hand in the standalone build: plugin picker, ghost text, the list, chips. Esc and the second Tab fix were checked only by the headless tests: the window sat on another Space for the second round.

## Open

- Done 2026-11-02: the Rust runtime runs the v11 four-seed ensemble in Q8_0 with `conv,pronoun` (session parser-runtime, merged here). pin.rs runs on the frame after these rules, so chips and rules agree; live check on four synthetic sentences in `plugin/parser/tests/pin_live.rs`.
- Gesture ghost text finds only 4.4 % of the gesture spans on the dev sets: the glossary phrases rarely start with the words the sentences use ("nach links drehen" against "Hand drehen"). Phrases indexed from the middle, or ghost text on fuzzy matches, would raise it; both need a measurement first.

- Learning from picks: recent picks only add a fixed bonus that decays by position. A per-user count of (typed prefix, picked id), saved with the plugin state, would let "cut" rank "Filter 1 Freq" over "Cutoff Rand" after a few picks. Weights are hand-set on synthetic sentences; nothing is tuned on Noa's sentences.
- Plugin picker: a stand-in combo over the exported plugins until the FL target picker (session fl-target-picker) gives the live inventory. The export holds 10 demo plugins; `--plugin` adds more.
- Ambiguous glossary phrases show twice ("Faust" as hand.fist and as the fist shape; "spread my fingers" also under hand.openness). The list shows the source next to each item; the glossary's ambiguous list could pick one.
- Phrases with negative polarity ("Finger zusammen", "Hand öffnen") pin their source without setting INV; the parser still reads the words and decides.
- A gesture chip that is meant as a gate ("only while I make a fist") is pinned as the route's source.
- Values and tracks get no completion yet (VALUE, TRACK, PLUGIN spans).
- Ghost text shows only when the cursor is at the end of the line; mid-line edits use the list.
- Visuals are plain egui: chip background from the selection colour, ghost text in the weak text colour. The final look comes from Figma.
