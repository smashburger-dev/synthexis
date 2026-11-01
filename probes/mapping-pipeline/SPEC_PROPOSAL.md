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

## map.parser.model (amend, 2026-11-01)

"The parser's encoder is a pretrained multilingual model (mmBERT small trimmed to German and English, 60M parameters), fine-tuned with the frame decoder; it ships as f16 (121 MB) and runs at 30 ms median per sentence on one CPU thread. RAM budget for the plugin: 500 to 800 MB including the image segmenter (Noa, 2026-11-01), so f16 weights are preferred over int8 (dynamic int8 costs 2.2 points route_edit)." Measured in probes/parser-errors/RESULT.md, section 10; real-sentence eval 31 % exact against 22 % for the from-scratch parser (probes/real-eval/RESULT.md, "Parser v7"). The plugin needs a ModernBERT runtime (open).

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

# Spec proposal, open-vocabulary parameters (2026-10-31)

Triggered by Noa's question whether plugins with unusual sections (ShaperBox, Portal, Thermal, delays) and plugins added later can be mapped without retraining. Measurements in `probes/open-vocab-mapping/RESULT.md`. The rule behind all items: the parser learns how people talk; what plugins and parameters exist is data read live from the DAW, never weights.

## map.resolver (amend)

"Binds a target in this order: user alias (map.aliases) > target text against the live parameter names of the named plugin (char 3-gram Dice + word-prefix cover, digits must match) > param-sense class for meaning-only targets ('brighter', 'more space') > sound fingerprint (map.sound_probe). The target text is the sentence text from the first to the last TGT piece of the parser's BIO head, the plugin a parser span or, when the span matches no project plugin, a project plugin name found in the sentence. Module plugins: read module switches and slot types (ShaperBox 'NoiseShaper On', Portal 'SLOT 1 DELAY TYPE'), prefer active modules, break slot ties with them, and put 'switch module on' into the route diff when the target sits in an inactive module. Below a score threshold (calibrated on Noa's 50 calibration sentences) or with a small margin to the second candidate, show the top-3 as chips (map.fallback)." Measured on template sentences with unseen names, held-out seeds: top-1 0.68 / 0.81, top-3 0.77 / 0.87; the class-only binding reaches 0.13 / 0.12.

## map.live_inventory (new)

"When a plugin is inserted, the FL script reads its parameter count, names and value strings (`plugins.getParamCount/getParamName/getParamValueString`, as `tools/mapping/fl/device_synthexis_harvest.py` does) and param-sense labels them on the spot. A plugin never seen at build time is mappable seconds after insertion, offline." The dump path is built; reading one third-party plugin through the bridge is not tested yet.

## map.sound_probe (new, research)

"Names can mislead ('Gravity', 'Liquid', 'Tempest'); the sound does not. For each parameter, render a fixed test signal at a few raw values and keep a fingerprint of what changes: level, spectral centroid, stereo correlation, tail energy after the bursts, noise floor, odd/even harmonics. Fingerprints answer descriptor targets ('heller', 'breiter', 'dreckiger', 'mehr Raum') for parameters whose names say nothing." Example from the live test: Timeless 'Feedback' raw 0 -> 1 raises the level by 9.9 dB and lowers the centroid from 11.2 to 6.4 kHz. Open: ShaperBox renders unchanged without a host transport and Thermal does not load in pedalboard, so the probe has to run inside FL (offline render of a scratch project) or a host that supplies a song position. Related work maps words to effect settings for EQ and reverb only: SocialFX / Audealize (crowdsourced word-to-parameter data), Text2FX (CLAP-guided, differentiable effects, arXiv 2409.18847), LLM2Fx (LLM predicts EQ and reverb parameters, arXiv 2505.20770). None of them handles black-box third-party plugins.

## map.language_learn (new)

"MIDI learn for words. When a target stays unresolved, synthexis asks the user to move the control they mean; the FL script compares the plugin's parameter values before and after and binds the phrase to the changed parameter (stored in map.aliases)." Polling a parameter diff works with the existing script API; whether FL exposes the last tweaked parameter to scripts directly is not checked.

## map.aliases (new)

"Per user: phrase -> plugin parameter and phrase -> source, written whenever the user picks a chip or uses map.language_learn, checked first by the resolver. No weights change, so nothing is forgotten and every entry can be shown, edited and deleted; exportable as a small JSON file." The pattern of fixing a frozen on-device model through stored prototypes instead of retraining: Continual Error Correction on Low-Resource Devices (arXiv 2511.21652).

## map.adapt (new, optional, not for the demo)

"Weight updates on the Mac are feasible: param-sense (0.80M parameters) trains on all 78,846 rows in 0.7 min on the CPU; the parser (9.30M) took 45 min for 16,000 steps on MPS. A user-triggered 'retrain' could tune a low-rank adapter or the output layers of the parser on the user's corrected sentences, mixed with replayed synthetic sentences, and keep the new weights only if the dev sets do not drop." Not proposed now: new plugins and names need no parser change (map.resolver), user wording lives in map.aliases, and an on-device training runtime is a large build for a small gain.

# Spec proposal, demo requests (2026-10-31)

Source: Noa's message (the XY pad of a plugin, Portal: MACRO 1 and MACRO 2 are the pad's axes) and the request types in his real sentences (`probes/real-eval/RESULT.md`). Each item says where the need showed up. Needs first seen in eval groups are marked "eval": if stage 4 or the retrain covers them, the final report scores those eval groups separately, so the number is not read as generalisation.

## map.xy (new)

"A two-axis target: one hand's x and y, or a pinch-held position, drive two parameters at once. 'The XY pad', 'X and Y' or 'both macros' expand to the plugin's pair: parameter names with Hor/Ver or X/Y (FabFilter Saturn 2, Timeless 3, Twin 2, Volcano 3: XY Controller N Hor/Ver; Serum 2: Filter 1 X/Y; Lunacy CUBE: Orb X/Y Position), else a per-plugin table for pads not exposed by name (Portal: MACRO 1 = x, MACRO 2 = y)." Measured: 6 of 405 harvested VST3 plugins name XY parameters; Portal's pad needs the table. Seen in: Noa's message, eval.

## cond.clutch (new)

"Pinch as a clutch: while pinched, the route adds the source's change since the pinch began to the target's value at that moment; on release the value stays. Pinch and turn is a virtual knob (hand.roll under the clutch), pinch and drag a virtual fader (hand.x or hand.y). Without the clutch, a gated route jumps to the hand's absolute position when the gate opens." Seen in: calibration and eval; 6 of Noa's 34 sentences use a pinch.

## cond.value (new)

"A trigger sets a fixed value in the parameter's own units (a wavetable position by number, a range in octaves, a rate as a note value). The resolver converts with the parameter's value-to-text mapping: the harvest stores 11 samples per parameter; the plugin can ask for the text of any value at runtime." Measured on the harvest: Serum 2 A WT Pos reads 1 .. 256 and A Warp Mode reads its mode names, so both convert; Arp Range and Arp Rate read -100.0 .. 100.0, so octaves and note values need a per-plugin table or a live check. Seen in: eval.

## cond.step (new)

"A trigger steps a stepped or list parameter to its next value (warp mode, filter type) and wraps at the end. A target the plugin does not expose gets an answer instead of a guess: 'not automatable here', with chips for synthexis' own generator." Measured: Serum 2 has no arpeggiator pattern parameter in the harvest (Arp Enable, Rate, Range, Offset, Repeats, Gate, Chance and others exist). Seen in: eval.

## cond.ramp_toggle (new)

"A trigger on a continuous target starts a ramp; the next trigger reverses it. Speed from modifier time or words like 'slowly'." Seen in: eval.

## midi_props (amend)

"arp.style: the arpeggiator's note order (up, down, converge, diverge, pinky up, thumb up and the like), one per finger state when the user names one per finger." Seen in: calibration.

## shapes (amend)

"Any finger combination is a finger state (5-bit mask), not only the 12 named shapes; index and ring finger together is 01010." Seen in: eval.

Already expressible, no change: one source driving several targets in different directions (routes sharing a source, each with its own invert), and a chord or a note roll per finger (midi.chord, midi.pattern with note and grid).
