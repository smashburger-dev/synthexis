# Changelog

synthexis for FL Studio on macOS (Apple Silicon), VST3 and CLAP. Downloads on the [releases page](https://github.com/smashburger-dev/synthexis/releases).

## 0.1.3 (2026-11-13)

Labels for the stock plugins of Ableton Live and Logic Pro, ready for the bridges that will report their names ([probes/ableton-stock](probes/ableton-stock/RESULT.md), [probes/logic-stock](probes/logic-stock/RESULT.md)).

Changed:
- 71 Ableton Live devices carry hand-made labels for 2,421 parameters: 47 audio effects (8 of them legacy), 12 instruments, 8 MIDI effects and the 4 racks. The names are the ones a Live remote script reads (`DeviceParameter.original_name`), taken from the parameter banks in Live's own control surface scripts and checked against the Live 12.4 Reference Manual.
- 87 Logic Pro 13 plug-ins carry hand-made labels for 1,744 controls: 64 effects (7 MIDI plug-ins among them) and 23 instruments, named as in Apple's Logic Pro 13 User Guide.
- Both sit in the label table as `ableton:<device class>` and `logic:<plug-in>`, so a third-party plugin called "Reverb" or "Compressor" keeps its own labels. Every existing label stays as it was. The table grows from 943 to 1,101 plugins.

Known limits:
- No Live or Logic bridge reads these names yet; until one does, the labels are not used.
- 124 Live parameter names come only from 12.0 or 12.1 scripts for devices that Live 12.4 changed (Wavetable, Reverb, Delay, Drum Sampler, Chorus-Ensemble); they may be gone in 12.4. Max for Live devices, External Audio Effect, External Instrument, Spectrum and Tuner have no labels.
- The Logic names are the guide's control labels; the names Logic uses for automation can differ. Noise Gate, Scripter, the metering, legacy and surround plug-ins have no labels, and ES2 has only 16.

## 0.1.2 (2026-11-13)

FL Studio stock plugins work as route targets. Measured in FL Studio 2026 on 109 stock plugins ([probes/fl-stock-plugins](probes/fl-stock-plugins/RESULT.md)).

Changed:
- A sentence that names a plugin now binds that plugin. Before, synthexis resolved every sentence on the picked plugin, FL's focus or the pin: "spread my right fingers to open the filter on Fruity Filter", typed while Fruity Blood Overdrive had focus, gave `Fruity Blood Overdrive / Post filter`. Now the picker moves to Fruity Filter and the route goes to `Cutoff frequency`. The pin stays as it was, and the next plugin you click in FL takes the picker back.
- 56 stock effects carry hand-made labels for 675 parameter names, among the effects Fruity Reeverb 2, Delay 3, Filter, Limiter, Parametric EQ 2, LuxeVerb, Maximus and Gross Beat. The label table grows from 887 to 943 plugins. Pure dry levels count as "other", so "open the reverb" raises Wet level, not Dry level.
- The picker hides parameters whose names are control characters. FL Slayer, WASP, WASP XT and Fruity LSD report such names and ignore every write.

Known limits:
- Stock synths and the large racks (Sytrus, Harmor, Fruity Love Philter, Delay Bank and others) have no labels yet; synthexis guesses their parameters from the names. The paid label run waits for the Baseten deployment `muse-spark-open-1026`, which is deactivated.
- 7 of 106 stock plugins with parameters do not take every write; the list is in the [README](README.md#stock-plugins).
- Patcher, Wave Traveller and Soundgoodizer expose no parameters to FL scripts, so no route reaches them.
- A route on a stock plugin has not yet been played with a real hand in front of the camera. Every step after the camera is measured.
- Logic Pro and Ableton Live are not supported yet ([docs/host-support.md](docs/host-support.md)).

Updating from 0.1.1: quit FL Studio and run `install.command` again.

## 0.1.1 (2026-11-05)

- Recorded automation no longer stutters on playback. synthexis wrote a parameter on every camera frame that changed it, up to 30 points a second, hand jitter included. Now every value goes out on the 128 steps of a MIDI CC and only after it moved a full step, as from a hardware knob: a hand held still writes nothing, and a full sweep writes at most 128 points.

Built from commit 5c9852e.

## 0.1.0 (2026-11-05)

First release, the hackathon build: camera inside the plugin, routes from English and German sentences, the plugin picker that follows FL's focus, MIDI parts per finger, the head section, Pong, presets and profiles.

Built from commit 62f2287.
