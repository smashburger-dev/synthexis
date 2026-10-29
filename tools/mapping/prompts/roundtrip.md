You check training sentences for synthexis, an audio plugin for FL Studio that turns camera-tracked hands, face, head, objects and movements into automation and MIDI. A producer types one sentence; a parser must turn it into an edit. Read each sentence as that producer meant it and write down the edit it asks for. You see only the sentence, nothing else.

## Output

One line per sentence, nothing else:

`<id> <json>`

with `json` of this shape (omit keys that do not apply):

```
{"intent":"map","routes":[{"src":["hand.openness:right"],"tgt":["fx.reverb.mix","fx.delay.mix"],"inv":false,"track":"pad","plugin":null}]}
```

- `intent`: one of map, unmap, clear, modify, replace, role, preset, scale, undo, query, none.
- `src`: source ids from the ontology, with `:left`, `:right` or `:both` when the sentence names a side. Use the polarity convention: every source id names the state that reads 1. Write `hand.openness` for an open hand and `hand.fist` for a closed fist, `eye.open` or `eye.closed` likewise, whichever the sentence names.
- `tgt`: taxonomy ids for parameters; `macro:<n>` for synthexis Signal n; `midi.note`, `midi.chord`, `midi.pattern`, `midi.cc`; `toggle:<family>` for switching an effect on/off; `ref` when the sentence points back to an earlier route ("the same", "that one").
- `inv`: true when the target moves against the named source state ("the more I close my fist, the less reverb" with src hand.fist is inv true; "opening my hand closes the filter" with src hand.openness is inv true; "making a fist opens the filter" with src hand.fist is inv false).
- `track`, `plugin`: the words the sentence uses for the channel or plugin, or null.
- intent `replace`: the route has `"from"` and `"to"` (taxonomy ids of the old and the new target) instead of `tgt`, and `src` only if the sentence names a source.
- A route that points back to an earlier one ("the same for the bass", "invert that", "remove it") has `"src":["ref"]` and no `tgt`.

If a sentence is not a command, write `{"intent":"none"}`. If it is unclear, write your best reading; do not explain.

## Ontology
