You build a glossary for synthexis, an audio plugin for FL Studio that turns camera-tracked hands, face, head and objects into automation and MIDI for the instruments and effects a producer already owns. Producers type sentences like "my right fist controls the reverb on the pad" or "Mund auf öffnet den Filter vom Bass". A small on-device parser must understand every way people say these things. Your glossary lists those ways.

## Input

A language (`en` English or `de` German) and a list of concepts. Each concept has an id, a kind and a short meaning. Kinds:

- `source`: something the user does or that the camera sees (a hand, a finger, the mouth, the head, an object, a movement).
- `shape`: a named hand pose held as a trigger.
- `param`: what a plugin parameter means (cutoff, reverb amount, attack ...).
- `target`: a kind of thing a gesture can control (a parameter, a synthexis macro, a MIDI note, a pattern).
- `intent`: what the sentence asks the plugin to do (connect, remove, change, undo ...).
- `modifier`: how a connection behaves (inverted, limited range, curve, amount, smoothing ...).
- `track`: a channel role in a project (pad, bass, drums ...).
- `preset`, `object`, `side`, `role`, `scale`: preset names, physical objects, left/right hand, a hand's job (MIDI, parameter, motion, off), musical scales.

## Task

For every concept write 25 to 40 different phrases in the requested language that a producer would actually type when they mean this concept, ordered from most common to rarest. Cover:

- the plain words and the short forms producers type fast ("res", "fx", "rev", "dly", "LFO");
- casual speech and slang ("ball your hand up", "crank", "squash");
- producer and DAW jargon, including FL Studio words (channel rack, mixer insert, Fruity plugins);
- beginner descriptions that do not know the term ("the knob that makes it sound underwater" for filter cutoff, "the echo" for delay);
- words of people with no studio vocabulary at all, who describe feelings, images and plain body movements ("dreamier", "far away", "sparkly", "warmer", "wobbly", "squeeze my hand", "wave", "move my hand up"); at least a third of the phrases for `param`, `source` and `shape` concepts should be of this kind;
- for `de`: real German usage, which mixes in English terms the way German producers talk ("Cutoff", "Reverb", "Delay-Feedback", "den Filter aufmachen", "Hall", "Fläche", "Faust machen"); keep English words that German producers use as is;
- one or two common misspellings or typos per concept, marked as such.

Give each phrase a form and a polarity:

- `f` (form): `n` for a noun phrase ("my fist", "the cutoff", "Hallanteil"), `v` for a verb phrase or clause fragment ("making a fist", "open the filter", "den Mund aufmachen", "turn it up").
- `pol` (polarity): `1` if the phrase points to the concept's high end (for sources the state that reads 1, for params more of the quantity, for intents and modifiers the concept itself), `-1` if it names the low end ("lowering my hand" for hand height, "close the filter" or "darker" for cutoff, "less reverb"). Most noun phrases are `1`.
- `typo`: `1` for deliberate misspellings, else omit.

Rules:

- Stay inside the concept. Do not drift into neighbouring concepts (reverb size is not reverb amount, the index finger is not the whole hand).
- For `source` and `shape`, include phrases with and without "my", with body-part words and with the action ("my mouth", "mouth open", "opening my mouth", "when I open my mouth"). Leave out left/right; sides are their own concept.
- For `param`, include both the parameter word ("cutoff") and what it does to the sound ("brightness", "makes it duller" with pol -1).
- For `intent`, write the connecting words and frames with X and Y as placeholders ("X controls Y", "map X to Y", "let X drive Y", "verknüpfe X mit Y", "X steuert Y").
- For `modifier` values, write the words for exactly that value ("a little" for amount subtle, "voll" for amount full).
- No duplicates within a concept. No explanations.

## Output

Only JSON objects, one per line, no other text:

`{"c":"<concept id>","p":"<phrase>","f":"n","pol":1}`

## The domain

The ontology below lists every concept synthexis knows, so you can keep neighbouring concepts apart.
