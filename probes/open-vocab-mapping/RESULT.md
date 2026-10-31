# RESULT: open-vocabulary parameters and live writes (2026-10-31)

Question (Noa): can the mapping assistant handle plugins and parameter names it was never trained on (ShaperBox sections, Portal, Thermal, delays), and can a user add plugins live without retraining?

Short answer: yes, if the parser hands over the words the user said for the target and a resolver searches the plugin's live parameter list for them. The taxonomy class alone does not carry unseen names.

## 1. What the parser does with unseen names

`p-v3-c50-s0` on eight sentences with parameter names it never saw (scratch test, BIO head printed next to the frame):

| sentence (short) | frame target class | BIO target span |
|---|---|---|
| Gravity knob on Portal | mod.amount (0.44) | "Gravity knob" |
| Liquid Center in ShaperBox | fx.drive (0.86) | "mehr Liquid Center" |
| Tempest amount in Thermal | osc.fm (0.49), plugin "rmal" | "Tempest amount" |
| Grain Density in Portal | sample.position (0.90) | "Grain Density" |
| Noise Mix in ShaperBox | osc.level (0.78), plugin dropped | "Noise Mix" |
| Wow Flutter on the tape echo | filter.cutoff (0.95) | "Wow Flutter" |
| Diffusion in Valhalla Delay | fx.reverb.mix (0.32) | "open" (wrong) |
| Feedback on Timeless | fx.delay.feedback (0.99) | "Feedback" |

The span is right in 7 of 8. The class is a guess for every unseen name, often with high confidence (0.95 for "Wow Flutter" as cutoff). "Thermal" is split by the tokenizer into "The" + "rmal", so the plugin pointer comes back as "rmal".

## 2. Resolver by span against resolver by class

`uv run probes/open-vocab-mapping/resolve.py --eval --n 20 --seed S`

Template sentences (three English, three German templates, six gesture phrases) around 20 randomly drawn real parameter names per plugin. The names are lower-cased and half of the time lose the slot prefix ("SLOT 1 DELAY FEEDBACK" said as "delay feedback"; then any slot counts as correct). Plugins: ShaperBox 3, Portal, Thermal, ValhallaDelay, Timeless 3, UADx Galaxy Tape Echo.

- class: parameters of the plugin labelled with the parser's class (the current `map.resolver` design)
- span: name similarity (char 3-gram Dice + word-prefix cover, digits must match) between the target text and every parameter name of the plugin
- both: span + 0.15 x class agreement

Seed 0 was used to find the failure causes and fix them (target text taken from the first to the last TGT piece, plugin found by project plugin names when the span is broken, digits as identifiers). Seeds 1 and 2 were run once after that.

| seed | n | target text found | class top-1 | span top-1 | span top-3 | both top-1 |
|---|---|---|---|---|---|---|
| 0 (dev, before fixes) | 119 | 91 | 0.118 | 0.555 | 0.622 | 0.546 |
| 0 (dev, after fixes) | 119 | 104 | 0.160 | 0.773 | 0.832 | 0.765 |
| 1 (held out) | 119 | 89 | 0.134 | 0.681 | 0.765 | 0.672 |
| 2 (held out) | 119 | 102 | 0.118 | 0.807 | 0.874 | 0.798 |

Plugin found: 0.824 before the fixes (Thermal 0/20), 1.000 after.

Seeds 1 and 2 by plugin, span top-1 / top-3: ShaperBox 0.80 / 0.85, Portal 0.78 / 0.85, Thermal 0.78 / 0.93, ValhallaDelay 0.82 / 0.85, Galaxy Tape Echo 0.92 / 0.92, Timeless 0.38 / 0.53. Timeless has 1,007 parameters, most of them modulation-matrix slots ("XLFO 6 Step 16 Random"); random draws land there and the BIO head drops the digits. German 98/122, English 79/116.

The class bonus does not help: the labels for these plugins are noisy (ShaperBox: 220 of 875 needed adjudication, 282 are "other").

Limits: template sentences are easier than real speech. Noa's 100 eval sentences decide.

## 3. Live writes into the real plugins

`uv run probes/open-vocab-mapping/live_write.py` (log in `data/mapping/open-vocab/live_write.log`)

Sentence -> parser -> span resolver -> write raw 0.0 and 1.0 through pedalboard 0.9.26 (a generic VST3/AU host, standing in for FL's `plugins.setParamValue`) -> render a 3 s saw-and-noise signal -> compare with a control (same setting rendered twice). Audible = change at least 6 dB above the control.

| sentence | resolved to | result |
|---|---|---|
| pinch controls the grain dry wet in Portal | GRAIN DRY/WET | audible, level +1.6 -> -0.1 dB |
| fist closes the Feedback on Timeless | Feedback | audible, level +0.3 -> +10.2 dB, centroid 11.2 -> 6.4 kHz |
| head tilt changes the loop age on the Galaxy Tape Echo | Loop Age | audible (change -14.9 dB, control silent) |
| spread my fingers to open Diffusion in Valhalla Delay | Mix (wrong: span "open") | audible, but the wrong parameter |
| mehr Noise Low Mix in ShaperBox (module off, then on) | Noise Low Mix | write accepted (display 1.00), output identical to input |
| my fist switches the volume shaper on in ShaperBox | VolumeShaper On | write accepted, output identical to input |
| die Faust steuert den Drive in Thermal | DRIVE | Thermal does not load in pedalboard (AU and VST3 scan failure) |

ShaperBox passes audio through unchanged in this host for every setting tried (modules on, all band mixes 1, master mix 0, bypass 1). Its shapes run against the host's song position, and pedalboard does not supply a transport; that is the likely cause, not verified. ShaperBox and Thermal need the FL test: the FL bridge (`probes/fl-camera-plugin/bridge/`) has only written into the own probe plugin so far.

## 4. Module plugins

ShaperBox (12 modules x 3 bands), Portal (2 slots) and Thermal (3 stages, 2 slots) expose every module to the host at all times, 875 / 143 / 410 parameters. Which ones sound depends on state the host can read: `NoiseShaper On`, `SLOT 1 DELAY TYPE`. A resolver that reads those values can prefer active modules, break slot ties ("delay feedback" when only slot 1 holds a delay) and offer "switch the Noise module on" as part of the route diff.

## Files

- `resolve.py`: resolver prototype and the template eval
- `live_write.py`: end-to-end write and render test
- `data/mapping/open-vocab/` (not in git): eval rows per seed, live log
