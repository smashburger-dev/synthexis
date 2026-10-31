# Spec proposal from the mapping session (2026-10-29)

For the main session to merge into `spec/synthexis.yaml`. Numbers cite `probes/mapping-pipeline/RESULT.md`; unmeasured values are marked hypothesis.

## map.taxonomy (replace)

"44 classes, defined with boundaries and examples in tools/mapping/ontology.yaml (list order = class index). Level and space: amp.gain, amp.pan, stereo.width. Filter: filter.cutoff, filter.resonance, filter.env_amount. Sources: osc.pitch, osc.fine, osc.shape, osc.level, osc.fm, unison.detune, voice.glide, sample.position. Envelopes: env.attack, env.decay, env.sustain, env.release. Modulation: lfo.rate, lfo.amount, mod.amount, macro. Effects: fx.reverb.mix, fx.reverb.decay, fx.delay.mix, fx.delay.time, fx.delay.feedback, fx.mod.mix, fx.mod.rate, fx.mod.depth, fx.drive, fx.tone, fx.mix, fx.comp.amount, fx.gate, fx.eq.gain, fx.eq.freq, fx.eq.q, fx.pitch. Not performed: ctl.switch, ctl.select, system, unsafe; catch-all: other. Each label also carries a direction d (+1/-1: does raising the parameter raise the class quantity, e.g. Dry level is fx.reverb.mix with d=-1) and a safe range [lo, hi] in fractions of the parameter's own range."

## map.ontology (new)

"tools/mapping/ontology.yaml is the vocabulary of the mapping assistant: taxonomy, 42 sources (hand per side incl. fingers, spread, x/y/depth, pinch, speed, finger-state shapes; two-hand distance and orb; mouth, smile, eyes, head yaw/pitch/roll; arm raise, lean; tracked objects x/y/size/angle/speed; heart rate; motion primitives), 12 named finger states, 6 target kinds (param, macro, midi.note, midi.chord, midi.pattern, midi.cc), 11 intents (map, unmap, clear, modify, replace, role, preset, scale, undo, query, none), route modifiers (invert, range, curve, amount, smooth, hold, quantize, note, grid), track roles, presets. Polarity convention: every continuous source is 0..1 and its id names the state that reads 1 (hand.fist = 1 - hand.openness); the resolver folds pairs into one signal plus invert."

## bridge.harvest (replace)

"Parameter dump of all installed plugins with own build-time tools (tools/mapping/harvest/, never shipped; pedalboard dropped: GPLv3): AU via Apple's AU API (886 of 906 plugins, 122,664 params) and VST3 via the Steinberg VST3 SDK, MIT since 3.8.0 (405 of 549 bundles, normalized 0..1 value strings as FL sees them). Per parameter: name, group/unit path, unit, range, default, flags, value strings at 11 points. FL Studio stock plugins need the FL script dump (tools/mapping/fl/, Noa once)."

## map.param_sense (amend)

"Runtime inputs in FL are the plugin name and the parameter name; plugins.getParamValueString works only for some FL plugins (FL API stubs, plugins module) and FL exposes no units or groups. So the classifier is trained with group, unit and value strings as optional inputs (dropped at random in training) plus the names of the other parameters in the same plugin as context."

## map.parser (amend)

"Training data: tools/mapping/edits.py samples structured edits from tools/mapping/edits.yaml; Spark Open verbalises each edit in two styles (en 60 %, de 40 %) and marks the words of every slot ([[text|slot]]), so spans and canonical values are known by construction (tools/mapping/commands.py). A glossary of EN/DE phrases per concept (tools/mapping/glossary.py) feeds rare wordings into the edits and allows span-substitution augmentation."

## map.glossary (new)

"EN/DE phrases per concept (sources, shapes, params, intents, modifier values, tracks, presets, objects, sides, roles, scales) with form (noun/verb) and polarity, written by Spark Open at build time. Uses: rare wordings for synthetic commands, span substitution, the chip fallback (map.fallback) and ambiguity hints (phrases that belong to several concepts)."

## budget.money.baseten_usd (amend)

"Muse Spark Open on Baseten: input 1.25 $/M, cached input 0.15 $/M, output 4.25 $/M (Baseten /v1/models, 2026-10-29). Per-token billing, no idle cost; caching is automatic for identical prompt prefixes, so every run keeps one fixed system prompt and a stable x-session-affinity per worker. Spend so far: see probes/mapping-pipeline/RESULT.md and `uv run tools/mapping/spark.py ledger`."

# Spec proposal, ontology v1.1 (2026-10-31)

Triggered by Noa's design examples below and by parser v3 failing all ten of them (`tools/mapping/predict.py`, RESULT.md "Real sentences, first look"). The ontology change is in `tools/mapping/ontology.yaml` v1.1; the perception and engine parts need the main session.

## feat.hand.orientation (new)

"Per hand: roll = rotation of the palm around the forearm (angle of the index-MCP to pinky-MCP line against the image horizontal, signed), pitch = wrist bend (angle of the wrist to middle-MCP ray against the vertical). Both normalised 0..1 with 0.5 = neutral hand. Sources hand.roll, hand.pitch." Hypothesis: computable from the 21 hand landmarks already used for finger extension; not measured.

## feat.hand.finger (amend)

"Per finger also a direction: angle of the MCP-to-tip ray, 0 pointing down .. 1 pointing up, 0.5 horizontal (sources hand.finger.<f>.tilt). Plus finger count: number of fingers with extension above the finger-state threshold, 0..5 mapped to 0..1 in five steps (source hand.finger_count)."

## midi.chord, midi.patterns (amend)

"Generator properties a route can drive (target midi.prop): chord.size, chord.voicing, arp.rate, arp.octaves, velocity, note.length. A midi.chord route may name its chord (symbol like Am, G7 or degree like IV); several finger routes on one hand give a chord per finger (F30)."

## gest.hand_role (amend)

"Roles also per track or plugin: midi (synthexis sends only notes there and never writes its parameters), parameter (only parameter control), off."

## cond.range (amend)

"Centred routes (bipolar): the source's neutral middle leaves the target at its current value; one direction lowers, the other raises it, up to the route's range. For pitch targets with no pitch parameter in the plugin, the resolver may fall back to the host channel's pitch (FL channel fine pitch); hypothesis, FL API not checked."

## Design examples (Noa, 2026-10-31)

Design input, not eval data: these sentences shaped ontology v1.1 and are never used to score the parser. Paraphrased from Noa's voice notes.

- Open the fist: the synth opens (cutoff up); spreading the fingers at the same time makes it sharper (resonance up). Noa's own wording for the second part: "währenddessen", two routes that work at once.
- Pinch and drag vertically: only the granulizer gets more intense (gate pinch, source hand height, plugin scope).
- Tilt the hand from left to right: the plucks detune in that direction (centred route on hand roll).
- Tilt the head to the left shoulder: the synth detunes down to about -100 cents; to the right up to about +100 (centred route on head roll; plugin pitch or host channel pitch).
- Thumb drives the cutoff, index finger the resonance. Showing only the thumb and pointing it down closes the cutoff, pointing it up opens it; the same for the index finger and the resonance; both fingers shown and tilted move both. ("If I put my thumb down, close the cutoff; if I put my thumb up, open the cutoff.") Finger tilt gated by that finger's state.
- The left hand plays a chord per finger.
- The more fingers shown, the more octaves the arpeggiator plays.
- A VST plugin should only receive MIDI from synthexis.
