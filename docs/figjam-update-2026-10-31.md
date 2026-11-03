# FigJam update: what the hackathon taught us (state 2026-10-31)

Source for a new area on the [FigJam board](https://www.figma.com/board/WJIE59w34hCSTXuq3XpKGH), next to the 8 planning diagrams from 10-26. Built on 10-31:
- the area "What six days taught us" to the right of diagram 8
- titles 1-8 above the planning diagrams
- "Oct 31" status notes at the outdated parts
- build-plan bars coloured by status, plus a row for the mapping assistant

Nothing was deleted. Audience: someone curious about the project, not an expert, watching the demo video. Every number comes from a RESULT file or session protocol in this repo. Language on the board: English (demo and judges).

Layout: one row of sections, read left to right, each with a short headline, 3 to 6 stickies or shapes, and connectors where something flows. Colours:
- blue: what we built
- green: what worked
- red: what broke and how we fixed it
- yellow: deviations from the plan
- violet: new ideas
- grey: open

## 0. The idea in one picture

Camera -> body and objects become signals (hand open, fist, head tilt, a ball moving) -> synthexis plugin inside FL Studio -> knobs move and notes play, recorded as automation. A sentence like "open my fist to open the filter" creates the link.

## 1. Six days in one line (timeline)

- 10-26: plan, spec, four probes in one day
- 10-27: real SAM 3.1 labels, recorder fixed, S1 recorded and labelled
- 10-28: camera latency measured, automation into FL proven, plugin skeleton
- 10-29: labels three times cheaper, S2 labelled, mapping pipeline on Baseten
- 10-30: parser trained on the Mac overnight, v1 -> v2 -> v3
- 10-31: budget check, unknown plugins, error analysis

## 2. Seeing: from camera to signal

- Built: tiny student models for the plugin, taught by a big teacher (SAM 3.1 on Modal). Apple Vision needs 29.6 ms per frame; the students need 1.1 ms.
- Worked: camera to parameter in FL in 116-126 ms (median); iPhone via Continuity 100 ms from light to callback, the built-in camera 154 ms.
- Broke and fixed: the recorder silently stopped writing (a new preview layer on every redraw), now one layer plus a self-test.
- Broke and fixed: the first label run cost twice its estimate. Three cost guards now stop any run early.
- Deviation: image mode instead of video mode for labels, a third of the cost, finds more blurred hands.

## 3. Data engine

- Recordings S1 and S2: 25,729 labelled frames (person, face, hands, eyes, mouth, objects).
- Surprises:
  - the "pen" was a drumstick, the "mug" a soda can, so object prompts are now set per session
  - glasses made eye masks 2.2x larger
  - hands change so much in 67 ms that 15 fps stays
- Open: S3 and S4 recordings, student training from 11-01.

## 4. Writing into FL Studio

- Worked: automation is recorded only when the plugin writes on the main thread. Then it works even with the plugin window closed.
- Broke and fixed: FL ignores one script call (`setParamValue`) for synthexis. A second route through FL's record events works.
- New challenge: no live write into a commercial plugin through FL has been tested yet (ShaperBox, Thermal, Portal are next).

## 5. Mapping assistant: a sentence becomes a route (pipeline diagram)

Build time, online:
- Muse Spark Open on Baseten labels 302 plugins twice (85.5 % agreement, the rest adjudicated)
- it writes 11,333 everyday phrases in English and German
- it writes about 29,600 synthetic commands
- 54.65 $ of 100 $ so far

Runtime, offline:
- sentence -> parser (9.3M parameters, 9 ms, 9.4 MB)
- -> resolver
- -> route shown as a diff
- -> accept, edit or pick from chips

No cloud call when you play.

## 6. How the parser learned (chart)

Challenge set:
- v1 0.628
- v2 0.712
- v3 0.748

Dev:
- v1 0.697
- v2 0.768
- v3 0.796

Lessons:
- filtering sentences by a round-trip check removed useful data (-11 points)
- gluing two sentences into one taught multi-route commands
- each batch gave less: +15 points per doubling at first, +7.5 for stage 2, +1.6 for stage 3

So we stopped buying synthetic data.

## 7. Bugs we found on the way (red wall)

- Baseten client held a lock around every request: 3 requests/min -> 56-133/min after the fix.
- Plugins printing to stdout broke the harvest JSON: 312 -> 886 of 906 plugins read.
- A copy-chain bug left 665 parameters of Serum 2 and Augmented VOICES unlabelled.
- The tokenizer has no "ß": "schließe" becomes 4 pieces, German sentences with ß are only 40 % right.
- Labels say more than the sentence:
  - 29 % of objects are never named
  - 75 % of momentary switches have no "hold" or "while"
  - so the parser learned to guess
- "Thermal" is cut into "The" + "rmal", so the plugin name got lost.

## 8. Deviations from the plan (yellow)

- No online fallback, not even opt-in (Noa): unclear sentences show chips.
- The round-trip check is a measurement, no longer a filter.
- The parser trains on the Mac (45 min a run), no cloud GPU needed.
- Synthetic batches stopped after stage 3. Noa's own 150 sentences decide next.
- Runs are chosen on dev sets only, the challenge set is just reported (no tuning to the test).

## 9. New challenge: plugins nobody trained on (violet)

- Measured on 6 plugins with unseen names (ShaperBox, Portal, Thermal, Valhalla Delay, Timeless, Galaxy Tape Echo):
  - matching the words you said against the plugin's own parameter list finds the right knob in 68-81 % of cases
  - going through fixed categories finds it in 12-13 %
- Live test: writes into Portal, Timeless and Galaxy Tape Echo change the sound.
  - ShaperBox needs a playing song.
  - Thermal does not load in the test host.
- Ideas:
  - read every new plugin live when you insert it
  - learn knobs by their sound (brighter, wider, dirtier)
  - "MIDI learn for words": move the knob you mean once
  - your own words are remembered without retraining

## 10. Language in the model, the world at runtime (principle)

The parser outputs only what the sentence says. The rest is filled from context synthexis already has:
- the object the camera sees
- whether the plugin is a synth or an effect
- the project's plugin names
- your default settings (switch behaviour, automatic opposite direction)

Same principle for unknown plugins and for vague sentences.

## 11. Open until submission (grey)

- Noa's 50 + 100 sentences
- FL write test with ShaperBox, Thermal, Portal
- one bundled retrain with regression gates (tokenizer with ß, cleaner labels, word hash embedding)
- student training from 11-01
- resolver and chips in the plugin
- demo video 11-03
- submission 11-04

## Budget (small table)

- Baseten: 54.65 $ of 100 $, 3,670 requests. Reserve for motion labels 25 $.
- Modal: 25 $ credit used up by 10-29, overage allowed by Noa, 30 $ from 11-01.
- Meta API: 150 $ claimed.

## Update Nov 3: FL target picker and write test

Source: `probes/fl-target-picker/RESULT.md`. Changed on the board:
- "Writing into FL": the grey "not tested yet" sticky is now green (Portal and ShaperBox move while the song plays, knobs turn in the plugin window, Portal audible, FL records automation). A second row adds the inventory speed (blue), the 64 KB freeze (red), ShaperBox hiding Bits and Resample in its curves (yellow) and the open focus question (grey).
- "Open": the FL write test sticky is green, Thermal does not load in FL.
- "Unknown plugins": the ShaperBox/Thermal sticky now reports the FL result.
- "UX 4 Target picker": the follow-FL note says what was measured and that pick track, then slot is the default; the first-pick note gives 7 to 20 ms per plugin and the dropped MIDI CC entries.
