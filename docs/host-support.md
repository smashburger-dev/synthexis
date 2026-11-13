# Host support: Logic Pro and Ableton Live (state 2026-11-13, steps 1-3 built)

Research run on 2026-11-13: code read of the FL coupling, web research on Logic Pro 12.4 and Ableton Live 12.4, one adversarial check of the claims below. Logic was inspected on this Mac (bundle, entitlements, signing). UNVERIFIED marks claims without a primary source or a test.

## Measured (2026-11-13, `probes/logic-au/RESULT.md`)

- Steps 1 to 3 are built: camera guard, `MIDI_INPUT = Basic`, AUv2 `synthexis.component` (`aumu sgnl Snxs`). `auval -v aumu sgnl Snxs` passes.
- Logic 12.4 runs third-party AUs out of process on this Mac: `AUHostingServiceXPC` and `AUHostingServiceXPC_arrow`, both children of launchd, hold the project's AU binaries; Logic itself holds none. An AU crash takes down the hosting service, not Logic.
- Logic scans AUs only at launch (8 validator processes, `~/Library/Caches/AudioUnitCache/Logs/AUScan*.plist`). A component installed while Logic runs needs a restart or Plug-in Manager, Reset & Rescan.
- The camera guard works in a minimal in-process AU host without `NSCameraUsageDescription`: synthexis logs `host declares camera use: false` and does not ask; no TCC prompt, no crash.
- Not measured yet: loading synthexis inside Logic, the camera inside AUHostingService, the CoreMIDI ports from there, Signal automation in Logic. The probe session could not get the screen (see RESULT.md).
- Live: not run. The VST3 now has an event input bus (the cause of the Live 12.4 refusal).

## Why only FL works today

- Every write to another plugin and every MIDI note leaves synthexis as SysEx JSON for the FL device script (`writer.rs:607-610`, `fl.rs:273-291`, `midi.rs:654-661`). There is no `send_event` and no plain CC anywhere in `plugin/synthexis/src`. A DAW learn mode has nothing to learn.
- Plugin identity is FL-shaped and saved in projects: `fl::Key` with ids `fx:<track>:<slot>:<plugin>` and `gen:<ch>:<plugin>` (`fl.rs:66-110`), stored in routes, presets, profiles and the pin (`saved.rs`, `preset.rs`, `preset/profile.rs`, `params.rs`).
- Host-neutral already: the 32 Signal macros. `writer.rs:441-470` writes them with begin/set/end gestures on the main thread, which any host can record.
- Route values sit on a 128-step grid and go out only after a full step (`route.rs:329-341`, commit 5c9852e). That maps 1:1 onto 7-bit CC.
- The bridge protocol is 5 (`fl.rs:49`, `device_synthexis_bridge.py:86`); `spec/synthexis.yaml:180-181` still says 4.

## Blockers

| Host | Blocker | Severity | Check |
|---|---|---|---|
| Logic | DONE (step 3): AUv2 build, auval passes, not yet loaded in Logic. Before: no Audio Unit build. Logic loads AUv2/AUv3 only; synthexis exports CLAP and VST3 (`lib.rs:290-291`), nih-plug has no AU export. Route: Rust crate `clap-wrapper` (0.3.1, `export_auv2!()`), type `aumu` from the Instrument feature. A nih-plug CLAP wrapped to AUv2 has run in Logic before ([nih-plug#63](https://github.com/robbert-vdh/nih-plug/issues/63)). | critical | `auval -v aumu sgnl Snxs` (`auval -a` crashes on this Mac in an unrelated plugin) |
| Logic | Camera in the plugin very likely blocked. Logic 12.4 declares no `NSCameraUsageDescription` and no camera entitlement; neither does `AUHostingServiceXPC_arrow.xpc`, where Logic hosts AUs on Apple Silicon. `engine/camera.rs:353-366` asked AVFoundation without checking; since step 1 it reads the key first and does not ask without it. Silent deny or TCC killing the host: UNVERIFIED. Precedent: [SPARTA#82](https://github.com/leomccormack/SPARTA/issues/82) (REAPER SIGABRT). | critical | `plutil -p "/Applications/Logic Pro.app/Contents/Info.plist" \| grep -i camera` prints nothing |
| Logic | Which process hosts the AU decides whether it can open CoreMIDI virtual ports and read `~/Library/Application Support/synthexis`. Measured: third-party AUs run in `AUHostingServiceXPC` / `_arrow`; its entitlements have no app sandbox and no camera entry, its Info.plist no camera usage string. CoreMIDI ports from there: UNVERIFIED. | high | `lsof -p <pid> \| grep .component/` on the AUHostingService processes |
| Live | FIXED in code (step 2), untested in Live: `MIDI_INPUT = Basic` gives the VST3 an event input bus. Before: Instrument category with `MIDI_INPUT = None` (`lib.rs:188`), so nih-plug builds no event input bus (`vendor/nih-plug/src/wrapper/vst3/wrapper.rs:123-127`). Live 12.4 logs "instrument category, but no valid event input bus" for that case ([clap-wrapper#635](https://github.com/free-audio/clap-wrapper/issues/635), a user report). Live does not load CLAP. | high | Live `Log.txt` after a rescan |
| Live | Camera probably fine: Live 10 and 11 declare camera access ([zvid#196](https://github.com/lsegal/zvid/issues/196)); Live 12 not rechecked; one black-screen report ([motion2midi#1](https://github.com/froehlicht/motion2midi/issues/1)). | medium | `plutil` and `codesign -d --entitlements -` on the Live 12 bundle |
| Live | Third-party plugins with more than 64 parameters expose only configured parameters to MIDI Map and the LOM ([Live manual, plug-ins](https://www.ableton.com/en/live-manual/12/using-plug-ins/)). Serum 2 needs Configure first. | low | load a large plugin, panel stays empty |
| all | The FL bridge always starts in `initialize` (`lib.rs:224`), pings SysEx every 2 s and shows FL-only text after 20 s. Logic may record those pings on an armed track (UNVERIFIED). | medium | record 30 s on an armed MIDI track |

## What each host offers

| | FL Studio | Logic Pro | Ableton Live |
|---|---|---|---|
| Read plugin and parameter names | device script, every plugin | no public API; AppleScript has only Standard and Text suites; undocumented Lua MIDI Device Scripts send names of assigned parameters only | MIDI Remote Script (Python 3, `~/Music/Ableton/User Library/Remote Scripts`), LOM `Device.parameters`, stock devices by name |
| Write parameters | REC events, recorded | Controller Assignments Learn (Cmd-K, Cmd-L), Smart Controls; recorded in Latch/Touch | MIDI Map (Cmd+M) with Remote on; remote script via `Live.MidiMap` mappings |
| Stock plugins | Fruity plugins readable, unlabelled | Learn reaches them (Apple: "channel strip and plug-in parameters"); not AudioComponents, so `au_harvest` misses them | every device, names via LOM |

Whether a remote script setting `DeviceParameter.value` records automation: UNVERIFIED. Writing through script-built MIDI mappings records like a controller.

## Plan

1. DONE. Camera guard (2-3 h, all hosts). Read `NSCameraUsageDescription` from the main bundle before calling AVFoundation; when missing, show "this DAW gives plugins no camera access" instead of risking a host crash. `engine/camera.rs:346-375`, `i18n.rs`.
2. DONE in code; FL recheck still open (FL was busy). Live load fix (1 h + FL check). `MIDI_INPUT = MidiConfig::Basic` in `lib.rs:188`, ignore incoming events. Recheck loading and recording in FL.
3. DONE, auval passes. AUv2 probe build (0.5-1 day). `clap-wrapper` with `export_auv2!()`, xtask writes `synthexis.component` (type `aumu`, own codes), ad-hoc sign, install to `~/Library/Audio/Plug-Ins/Components`, `auval`. Keep param ids `sig01..sig32`.
4. PARTLY RUN (auval, hosting process, scan, guard in a test host; Logic UI steps open). Logic probe with Noa (checklist below). Decides the architecture. Results to `probes/logic-au/RESULT.md`.
5. HostBridge refactor (3-5 days). Trait with `link`, `view`, `want_params`, `write(key, index, v)`, `notes`, `caps`; `fl.rs` moves behind `FlBridge` unchanged. Versioned `TargetKey { Fl, Ableton, Learn }` that migrates old `fx:`/`gen:` ids. Route `writer.rs`, `midi::Bridge`, `pong::FlSink` and the GUI call sites through it.
6. Bridge selection (1 day). Host name from VST3 `IHostApplication::getName` / CLAP `host->name` through a small nih-plug patch (UNVERIFIED), manual override in settings. Open the FL ports only when an FL or Ableton bridge is active.
7. LearnBridge (3-5 days), shared by Logic, Live, Bitwig, Reaper. CoreMIDI sources "synthexis control" (CC) and "synthexis notes". CC = round(v*127). Learn slots with labels, a per-route "wiggle" button that sweeps the CC for the DAW's learn mode, all-notes-off on stop. 14-bit CC later, per route.
8. Logic path A (2-3 days, if the in-AU camera fails). The standalone `synthexis.app` with `NSCameraUsageDescription`, signed, runs camera, routes and LearnBridge, drives Logic through Learn. Follows MIDI clock from Logic.
9. Logic path B (1.5-3 weeks). AU shell keeps UI, project state, transport and Signal macros; the camera app streams samples into it.
10. Live test session (2-3 h) in a Live 12 Suite trial after 1, 2, 7. Results to `probes/ableton-live/RESULT.md`.
11. AbletonBridge (1.5-3 weeks). Python remote script `plugin/live/synthexis_bridge/`, same F0 7D JSON F7 protocol 5 with 16 KB pages, LOM inventory, focus listeners, writes through script-built MIDI maps.
12. Stock-plugin labels (own session). FL Fruity dump (`tools/mapping/fl/README.md`), Live device dump through the remote script, a hand-made Logic stock list.
13. Docs and installers. Host-neutral strings, installers that place the `.component` and the Live script, spec fixed to protocol 5, README table updated per test.

Steps 1 to 3 and 5 can run in parallel.

## Logic test checklist (Noa)

1. Baseline, no build: `plutil -p "/Applications/Logic Pro.app/Contents/Info.plist" | grep -i camera` (expect nothing). Plug-in Manager does not list synthexis.
2. Use an empty scratch project; the probe may crash the AU host.
3. After the AU probe is installed: `auval -a | grep -i synthexis`, then `auval -v aumu <subtype> <manufacturer>`, keep the last 30 lines. If Logic does not list it: Plug-in Manager, Reset & Rescan Selection, "Use" ticked.
4. Terminal: `log stream --predicate 'subsystem == "com.apple.TCC"' --info`, leave running.
5. Software Instrument track, load synthexis from AU Instruments. `ps aux | grep -i AUHostingService`.
6. Audio MIDI Setup, MIDI Studio: does a "synthexis" device appear while the plugin is loaded?
7. Open the synthexis window, start the camera. Note what happens (prompt, "An Audio Unit plug-in reported a problem", Logic quits, black view). Copy the TCC lines, check `~/Library/Logs/DiagnosticReports`, check Privacy & Security, Camera.
8. Signal automation: track in Latch, window closed, play, turn on "Test signal" from the automation menu. Are points recorded for Signal 1, stepped in 128 steps?
9. Learn pretest with any hardware knob: Channel EQ, Cmd-L, click a gain knob, move the knob, Cmd-L. Latch, play, move: is automation recorded? Repeat in Touch.
10. With the LearnBridge build: same with the "wiggle" button, then with a hand. Does the CC also land as MIDI on the selected track?
11. Notes: Settings, Advanced, Complete Features; track MIDI In Port "synthexis notes", record-arm, trigger a finger part, record 30 s. Notes present, none hanging?
12. Save, quit, reopen: routes still there?

Send back: Logic and macOS version, each step's result, TCC excerpt, crash reports, `ps` and `auval` output.
