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
