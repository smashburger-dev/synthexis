# Mapping assistant: design v1 (2026-10-29)

How a typed sentence becomes route edits, offline, and how Muse Spark Open teaches the small models at build time. Measured numbers go to `RESULT.md`; everything here without a citation is a design choice or marked hypothesis.

## Runtime (offline, in the plugin)

```
sentence ──► parser ──► frame (intent, routes, ontology ids, span pointers)
                          │
project params ──► resolver ◄── known-plugin table ── param-sense
                          │
                          ▼
             route diff (accept / edit / undo)   or   chips (map.fallback)
```

1. Parser: encoder plus a small decoder that emits a frame over a closed vocabulary: intent, route brackets, source ids, taxonomy ids, modifier values, and span pointers `[i, j]` into the sentence for free text (track names, plugin names, literal parameter names, numbers, note names). Constrained decoding through a frame grammar, so every output is a valid edit. This is the span-pointer formulation of Shrivastava et al. 2021 (Meta, arXiv 2104.07275: frames from ontology tokens plus span endpoints; 87 EM on TOPv2, better cross-lingual transfer than text generation), and the pointer seq2seq of Rongali et al. 2020 ("Don't Parse, Generate!"). Why not a pure BIO tagger: one sentence carries several routes with shared scope ("fist controls reverb and delay on the pad, mouth opens the bass filter"); a tagger needs extra linking rules for that, a frame decoder emits it directly. The encoder gets a BIO tagging head as auxiliary loss; the same tags drive the chips in the fallback.
2. Selective prediction: the parser returns a confidence (frame log-probability). Below a threshold calibrated on dev, the plugin shows chips instead of an edit. Precision over coverage: target exact route-edit accuracy on accepted sentences >= 0.95, report coverage next to it (hypothesis targets; spec map.eval asks >= 0.90 overall).
3. Resolver (deterministic): for each target, candidates = loaded plugin params whose class matches; order: known-plugin table, then param-sense probability; filter by track/plugin spans with fuzzy match on channel and plugin names; prefer the plugin type that fits (reverb mix inside a reverb); apply direction d (Dry, Damping, Threshold flip the route) and the safe range; never pick unsafe or mappable=false classes without an explicit literal parameter name. Several candidates left = chip with options.
4. Known-plugin table: exact (plugin name, parameter name) -> class, d, safe range for the demo plugins (ZENOLOGY, Serum 2, Analog Lab V, Valhalla, FabFilter, FL stock, Apple AUs), double-labelled by Spark Open and adjudicated. Shipped as data. Demo plugins never depend on classifier generalization.
5. param-sense: classifier for unknown plugins. Inputs available in FL at runtime: plugin name and parameter name (FL exposes no units or groups; getParamValueString works only for some FL plugins per the FL API stubs). Plus the other parameter names of the same plugin as context (tells a delay from a reverb). Hashed char n-grams and words, embedding bag, MLP, <= 1M params. Trained with group, unit and value strings as optional inputs (dropped at random).

## Model sizes and memory

| model | params | RAM (int8 / fp16) | latency target |
|---|---|---|---|
| param-sense | <= 1M | 1 / 2 MB | < 0.1 ms per parameter |
| parser, from scratch: SentencePiece 8k (EN+DE, byte fallback), encoder 6 x d256, decoder 2 x d256 | about 8M | 8 / 16 MB | < 10 ms per sentence on M1 CPU (hypothesis) |
| parser, pretrained option: 6 layers of a multilingual MiniLM (H384), vocabulary pruned to about 10k pieces, same decoder | about 14M | 14 / 28 MB | < 15 ms (hypothesis) |

Noa allows up to about 10M; the pretrained option is a measured candidate only if it clearly wins on the challenge set (below) and its weight licence allows redistribution. The parser runs on the main thread or a worker thread when the user presses enter, never on the audio thread.

## Build time (Muse Spark Open on Baseten)

1. Harvest (`tools/mapping/harvest/`): AU parameter trees of every installed plugin, own Swift tool on Apple's AU API. FL stock plugins through an FL script dump (`tools/mapping/fl/`, needs Noa once).
2. Labels (`label.py`): taxonomy class, direction, safe range per parameter. Plugin and template dedup for the bulk, single pass. Priority plugins: no dedup, two passes (temperature 0.2 / 0.7 with reversed order), adjudication of disagreements with reasoning. Audit: a random sample checked by eye, accuracy with a confidence interval in RESULT.md.
3. Glossary (`glossary.py`): 25-40 EN and DE phrases per concept with form and polarity, including jargon, beginner wording, Denglisch and typos. Used for rare wordings in the edits, span substitution, chips, ambiguity hints.
4. Edits (`edits.py`, config `edits.yaml`): structured edits sampled from the ontology with explicit attributes (language, register, noise, intent, slot combinations, modifiers, glossary wording). Attribute-conditioned generation gives more diverse and less biased synthetic data than class-only prompts (Yu et al. 2023, AttrPrompt, arXiv 2306.15895).
5. Commands (`commands.py`): Spark Open writes each edit in two styles with inline slot marks `[[words|slot]]`; labels are known by construction, the marks give the spans. Same idea as LINGUIST (Rosenbaum et al. 2022, COLING: generating slot-annotated utterances with bracket markup), here with an open-weight teacher and no fine-tuning.
6. Filters: mark validator (every slot marked exactly once); blind round-trip on a sample (Spark parses the plain sentence without the edit; mismatch rate = label noise estimate); after the first parser training, sentences where the trained parser confidently disagrees with the label go back to Spark for a second look (confident-learning style cleanup).
7. Augmentation (free): span substitution with glossary phrases of the same concept and polarity; casing and punctuation noise.

## Evaluation (decides, not opinions)

- Synthetic dev: held-out edits, never seen as edit or sentence.
- Challenge set: about 500 sentences from Spark with a different prompt ("write the way producers type in Discord, no glossary, mix languages"), never trained. Picks architecture and data size.
- Noa's 100 hand-written commands (`map.eval`): final gate only, never trained, never used for tuning.
- param-sense: vendor holdout (whole manufacturers held out) top-1 against Spark labels, plus the eye audit of the teacher labels.
- Learning curve at 25/50/100 % of the synthetic data decides whether more Spark data is worth buying.

## Training interfaces (settled 2026-10-29)

Parser frame (decoder output), canonical order = order of first mention in the sentence:

```
<intent:map> <route> <src> hand.fist <side:right> </src> <tgt> param:fx.reverb.mix </tgt> <tgt> toggle:delay <mode:toggle> </tgt>
  <track> PTR </track> <plugin> PTR </plugin> <pname> PTR </pname> <inv> <lo:20> <hi:70> <amt:0.25> <curve:exp>
  <smooth:high> <hold:freeze> <q:8> <note> PTR </note> <gate> hand.shape <shape:fist> </gate> <time> PTR </time> </route>
<intent:map> <route> <ref> <track> PTR </track> </route>                 "the same for the bass"
<intent:modify> <route> <ref> <delta:+0.1> </route>                       "a bit more"
<intent:replace> <route> <src> .. </src> <from> param:x </from> <to> param:y </to> </route>
<intent:role> <side:left> <role:midi>   <intent:preset> <preset:orb>   <intent:scale> <key:A> <scale:minor>   <intent:undo> <undo:redo>
```

- Closed vocabulary: intents, source ids, sides, object names, shapes, taxonomy ids, toggle families and modes, macro numbers 1..32, midi kinds and CC numbers, lo/hi in 5 % steps, amount and delta buckets, curve, smooth, hold, quantize values, presets, roles, keys, scales. PTR = two pointer steps (start and end token) into the encoder sequence, for track, plugin and literal parameter names, note names and times; the resolver parses PTR text (fuzzy names, note regex, beats/bars/ms regex).
- Rejected self-corrections (`w` slots) produce no frame tokens; the auxiliary BIO head tags them REJ so the encoder learns to skip them. BIO tag set: SRC, TGT, TRACK, PLUGIN, PNAME, NUM, AMT, MOD, NOTE, GATE, TIME, REF, REJ, ROLE, PRESET, KEY, SCALE, UNDO, DELTA.
- Constrained decoding by a small state machine over this grammar; the frame log-probability is the confidence.
- Tokenizer: SentencePiece unigram, 8k pieces, byte fallback, trained on all synthetic sentences plus glossary phrases plus harvested plugin and parameter names.

param-sense:

- Inputs (runtime-only branch): plugin name, parameter name, the other parameter names of the same plugin; optional value strings at 0/0.2/0.5/0.8/1 (dropout 0.5 in training; FL natives deliver them through getParamValueString). No group path or unit (FL does not expose them).
- Model: one hashed embedding table, 2^14 buckets x 48 dims (786k params), shared by three bags with field prefixes: name (char 3-5 grams + words), context (words of sibling names and plugin name), values (char 3-grams). Bag means concatenated (144) -> MLP 96 -> 44 classes, plus a 2-way head for d. About 0.8M params, 1.6 MB in fp16. Hash = FNV-1a 32-bit over UTF-8 of `field|gram`, mod 2^14, identical in Python and Rust.
- Loss weighted by inverse square root of class frequency; identical feature tuples deduplicated. Split by manufacturer: 20 % of vendors held out. Report top-1, macro F1, and per-class F1 for unsafe, ctl.switch, ctl.select, system.
- Known-plugin table: priority labels keyed by normalized plugin name and parameter name -> class, d, lo, hi; shipped as data; looked up before param-sense.

## Review round 1 (Spark Open as outside reviewer, `review-r1-*.md`, 0.14 $)

Accepted and done in configs/prompts: typos may hit marked words; template keyword sentences without verbs; flatter class weights and more toggle/MIDI/macro targets; more plugin and literal-name scopes; en/de 70/30 plus a codeswitch register; refs to earlier routes ("same for the bass", "invert that"); relative deltas ("a bit more"); toggle targets for events and shapes; gates ("only while I hold a fist"); ramp times ("over 4 bars"); self-corrections; negative lexicon check for unmarked extra concepts; round-trip check of every inverted sentence plus a 5 % sample; fx.tone direction rule; safe ranges rounded and overridden by parsed dB/percent where possible; rule labels for generic macro slots.
Accepted for training and runtime: canonical frame order = order of first mention; modifiers as quantized value tokens (span only for provenance); class-default safe ranges for unknown plugins, per-parameter ranges only in the known-plugin table; resolver ranks softly over param-sense top-2 instead of filtering on top-1; scope none with several matching tracks -> FL's selected channel, else chips; tokenizer trained on harvested names + glossary + synthetic text; param-sense unknown-plugin branch evaluated with runtime features only; selective-prediction threshold calibrated on human sentences, not synthetic; report Wilson intervals on Noa's set.
Rejected: deleting polarity pairs (hand.fist / hand.openness). A span-based parser decides the id from the words it sees ("fist" vs "open hand"); folding into one signal plus invert is the resolver's job and composes correctly ("fist, inverted" = openness). Kept edits_per_job at 20 (per-request reasoning overhead); the negative check catches cross-edit bleed.

## 2026 papers that change the plan

- Ventirozos et al., arXiv 2610.35102 (26 Oct 2026): persona- and syntax-conditioned synthetic queries for a cold-start semantic parser. Syntax conditioning moves synthetic data toward real traffic, but syntax-heavy prompt families had 38-42 % hallucinated items, mostly dropped constraints; the divergence between synthetic and human query embeddings tracks parser F1 (rho -0.917). Taken over: per-variant sentence frames, validator and round-trip against dropped constraints, and an embedding-divergence check against Noa's calibration sentences as a cheap data-quality proxy.
- Shen and Zhang, EMNLP 2026 (Preference-Aligned Data Synthesis for Fine-Grained Intent Detection): LLM-made data is ambiguous between neighbouring intents and repetitive within one. Taken over without fine-tuning the teacher: minimal pairs that differ in one confusable slot.
- Wang and Li, arXiv 2610.25344 (18 Oct 2026, TOMORI): an online code agent for REAPER; explicit instructions reach high agreement, abstract perceptual requests do not. Positions synthexis: offline, small, and abstract requests go to chips instead of guesses.
- Distilled structural reasoning (Scientific Reports 2026): trie grammar constraints remove syntax errors in decoding (87.42 EM on TOP with an 8B student). Supports the constrained frame decoder; our student is about 1000x smaller.

## Open, needs measurement

- Teacher label accuracy (eye audit), agreement of the two priority passes, cost per labelled parameter (pilots).
- Valid-variant rate of the command generator and the round-trip mismatch rate.
- Parser from scratch vs pretrained on the challenge set; latency and RSS on the M1 Pro.
