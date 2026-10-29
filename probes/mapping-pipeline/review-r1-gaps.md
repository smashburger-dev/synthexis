40 sentences the current grammar/ontology cannot emit correctly. Each would either fail constraint decoding, force hallucinated slots, or resolve to wrong safe behavior.

**English 20 - would fail or mis-resolve:**

1. nudge the reverb up a tiny bit on the pad
2. a bit less delay feedback on the lead please
3. do the same for the bass
4. apply that mapping to the drums too
5. that one should be on Insert 4, not the pad
6. only while i'm holding a fist, right hand y controls the filter
7. when i clap, toggle reverb mix on the pad
8. cross both hands to swap pad and bass filters
9. average both hands y to control cutoff on the lead
10. fade it in over 4 bars when i open my hand
11. hold for 2 beats then release to trigger the fill
12. actually not right hand, left hand controls cutoff
13. sorry i meant mouth open, not smile
14. no reverb, just the delay on that track
15. everything except the drums
16. load filter_hand on the lead but half as sensitive
17. what is currently controlling the bass filter?
18. is my right hand mapped to anything right now?
19. open filter with right hand y, but only a little and super snappy
20. map hand openness to reverb, and invert it when i smile

**German 20 - would fail or mis-resolve:**

21. mach den reverb bisschen mehr auf dem pad
22. gleiches für den bass bitte
23. das gleiche wie eben nur für Schlagzeug
24. nur solange ich die Faust halte soll hand y den Filter steuern
25. über 4 Takte einfaden wenn ich die Hand hebe
26. wenn ich klatsche, mach den delay an/aus
27. ach nee linke Hand statt rechte Hand für den Cutoff
28. sorry Mund auf, nicht Lächeln
29. nicht der delay, nur reverb auf dem pad
30. alles außer master
31. was steuert gerade den Bass Filter?
32. worauf ist meine rechte Hand gemappt?
33. beide Hände zusammen steuern den Cutoff - Durchschnitt
34. lade orb preset aber kleinerer range, nur 20 bis 60%
35. rechte Hand x steuert Pan, linke Hand y gleichzeitig Filter - getrennt
36. mach den Filter bisschen mehr auf, aber nur wenn Bass spielt
37. invertiere das letzte mapping
38. tausche pad und bass mappings
39. spiel hi-hat pattern schneller je schneller sich die Hand bewegt
40. mouth open soll Filter öffnen aber nur solange Augen offen sind

### Gaps -> concrete changes, no online model

**1. `tools/mapping/ontology.yaml:modifiers` + `edits.yaml:modifiers` - no relative delta**
What breaks: 1,2,21,36,37. Grammar only has `amount/range/curve/smooth` absolute. `nudge a bit more/less`, `bisschen mehr` has no slot. Parser forced to hallucinate `amount` or `range` and resolver overwrites value instead of adding -3dB.
Smallest fix: add `delta: float -1..1` + `delta_unit: percent|db|semitone` to modifiers. Add to `commands.md` marking rule `[[a tiny bit more|d1]]`. Add to edits.yaml `delta_prob:0.1`. Resolver: `new = clamp(old + delta)`.

**2. `probes/mapping-pipeline/DESIGN.md:1 Parser frame grammar` + `ontology.yaml:intents/targets` - no dialogue reference/anaphora**
What breaks: 3,4,5,22,23,37,38. Frame requires explicit `k1/p1/t1` span per route. No `ref` slot for `same/that one/that mapping/das gleiche/wie eben/letzte`. Span pointers `[[same|s1]]` invalid. `copy` would need prior diff in context window.
Smallest fix: add `refs: {last_map, last_track, last_target, selected_track}` to ontology, allow `track/plugin/target` slot to be `ref` OR span. Add `intents: copy` + `replace` with `from:ref`. Parser input = concat(prev accepted frame JSON + sentence), still offline seq2seq. Training: edits.py samples 30% edits as `copy+modify` with explicit antecedent.

**3. `tools/mapping/ontology.yaml:modifiers` - no conditional gate**
What breaks: 6,20,24,40,36. `hold` only handles source disappearance (`reset/hold/freeze`). `only while holding fist / solange Faust` + `when i smile` is gating by second source. Currently mis-parsed as two independent routes `fist->?` + `hand.y->filter`.
Smallest fix: add `gate: {source_id, side, shape, invert}` modifier. Edits.yaml `gate_prob:0.08`. Frame: `r1: s1 -> t1 | gate=s2`. Resolver: multiply route by gate threshold 0.5.

**4. `tools/mapping/ontology.yaml:modifiers` - no musical time / duration**
What breaks: 10,11,25. No `duration/ramp` slot. `over 4 bars / für 2 beats / über 4 Takte` has nowhere to go; parser drops it or tags as `range`. `hold` has no time param.
Smallest fix: add `ramp: {time_beats: 0.25..64, sync: bool}` + `hold_time: beats` to modifiers. Add to `motion.hold` params. Edits.yaml `ramp_prob:0.07`. Resolver maps beats->ms via host BPM at route creation, still deterministic.

**5. `probes/mapping-pipeline/DESIGN.md:3 Resolver` + `ontology.yaml:sources` - no source combinators / toggle semantics**
What breaks: 7,8,9,26,33,34,35,39. Sources are 1:1 or 2:1 without op. `average both hands`, `cross to swap`, `both together` needs `op: avg/diff/max/min`. `toggle on clap` needs `ctl.switch` with `event->toggle` but `ctl.switch mappable:false` hard-refuses. `faster hand -> faster pattern` needs `hand.speed -> midi.pattern.rate` which doesn't exist.
Smallest fix: ontology: allow `sources_per_route:2` with `op: {mix, diff, avg}`. Make `ctl.switch` mappable when source has `event:true` and add `action: toggle/momentary` to target. Add `midi.pattern.rate` as target param. Resolver implements avg/diff arithmetically.

**6. `probes/mapping-pipeline/DESIGN.md:1 Parser` frame grammar - no negation/exclusion**
What breaks: 14,15,29,30. No `exclude` operator. `not reverb / alles außer master / everything except drums` forces parser to either ignore `not` or emit positive route for excluded target. `none` intent is for chatter, not scoped negation.
Smallest fix: add `exclude: [track|target]` list to frame. Grammar: `r1: s1 -> t1 | exclude=[t2,k1]`. Resolver filters candidate list by exclude before ranking. Add 10% edits.yaml samples with `exclude`.

**7. `tools/mapping/prompts/commands.md:Output rules` + `DESIGN.md:1` - no self-correction handling**
What breaks: 12,13,27,28. Marks require exactly one slot per id, no repair. `actually not X, Y` / `ach nee ... statt` contains two competing spans for same slot. Current validator `every slot marked exactly once` discards these; at runtime parser emits first mention only.
Smallest fix: allow `repair` syntax in frame: `s1 -> t1 | supersedes=s1_old`. Training: edits.py generates 8% edits with `correction: true`, commands.py renders `[[right hand|s1_old]] actually [[left hand|s1]]`. Loss masks `s1_old`.

**8. `ontology.yaml:intents` + `DESIGN.md:2 Selective prediction` - query intent underspecified**
What breaks: 17,18,31,32. `query` only has `source/target/track/all` scope, no WH-slot `what-controls-X` vs `what-does-source-control`. Parser has no `query_target`/`query_source` distinction, fallback chips show generic ambiguity hints instead of answering from current diff.
Smallest fix: split `query` into `query.source {source_id}` and `query.target {taxonomy|track|plugin}` + `query.route {ref}`. Add to glossary and edits.yaml `query_scope`. No model change, just closed vocab expansion. Resolver answers by scanning current route table, no LLM.