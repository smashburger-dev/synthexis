# Session 2026-10-28: hill-climb line (H1 camera latency, H2 automation, P3, plugin skeleton)

Ran in parallel to the main session. Every number cites its RESULT file; unmeasured values are marked hypothesis.

## Results

- **H2 / P2 passed** ([probes/fl-camera-plugin/RESULT.md](../probes/fl-camera-plugin/RESULT.md), section "H2 in FL Studio 2026"). FL Studio 2026 records VST3 parameter edits (begin/perform/end) issued on the **main thread** with the plugin window closed (playback range 0.98-0.99, per-write and held gestures). From a background thread FL records only with the window open; the CLAP bundle recorded nothing with the window closed. MIDI path D was not needed and not tested.
- **P3 passed** ([probes/fl-script-bridge/RESULT.md](../probes/fl-script-bridge/RESULT.md)). An FL script setting a VST3 parameter with `plugins.setParamValue` during recording is recorded (range 0.987, 127 values). Target was our own VST3 (a third-party plugin from FL's view); a commercial plugin was not tested.
- **Welle 3 plugin skeleton built and gated in FL** (`plugin/`, [RESULT section "Welle-3-Gate"](../probes/fl-camera-plugin/RESULT.md)): one camera engine per host process, signal contract, conditioning chain (range/curve, One Euro, occlusion hold, reacquire slew), many-to-many routes onto 32 host-automatable macros "Signal 1..32", main-thread writes, hand-role params (stored, no behaviour yet), minimal functional editor. Vendored nih-plug de42101 with a `host_context_available` hook ([plugin/vendor/PATCHES.md](../plugin/vendor/PATCHES.md)) so the plugin writes without its window ever opening. In FL: context 1 ms after load, writes with the window closed, FL records camera-driven Signal 1 (brightness), Signal 2 (motion) and the test triangle on Signal 32 (playback range 0.744, empty pattern flat). 11/11 unit tests.
- **Camera to parameter in FL**: 116-126 ms median, 142-145 ms p95; of that, camera delivery (PTS to callback) 117-135 ms median, features 0.7 ms median.
- **H1 camera latency** ([probes/camera-latency/RESULT.md](../probes/camera-latency/RESULT.md)). Judge: screen flash, light to callback via `CAMetalDrawable.presentedTime`. Built-in camera baseline 154 / 178 ms (median / p95), spread 2.8 % over 3 runs. Rejected levers: activeFormat + locked 30 fps, 640x480, 420v, userInteractive queue. iPhone via Continuity 100 / 116 ms (delivered 30 fps even with a 60 fps format; open). The built-in camera has two delivery states about 65 ms apart; ~20 ms of CPU work per frame on the capture queue raises time in the low state from 2 % to 48 % (n=3 per condition), 3 ms does nothing, sleeping makes it worse. Mechanism open. Derived floor for the built-in camera: about 80-90 ms light to callback. The same two states show in FL (52-63 vs 100-125 ms).
- **H3 deferred** ([probes/coreml-student-latency/RESULT.md](../probes/coreml-student-latency/RESULT.md), "D10-D12 export check"): MobileNetV4-Conv-Small, its 0.5 variant and RepViT-M0.9 convert to CoreML at 384x216 and 320x180, with and without a previous-mask channel. EfficientViT-B0 fails to convert (attention context module). No latency measured; run after the first trained student.
- **FL bridge** (`probes/fl-camera-plugin/bridge/`): own FL MIDI script "synthexis bridge" (SysEx JSON commands: channels, params, get/set, rec, show window, transport, pattern, link_last) plus a CoreMIDI daemon `flbridged` with a unix socket and `flb.py`. Test-only tooling, never shipped. Lets the agent run FL tests without Noa clicking.

## Open

1. `plugins.setParamValue` from the FL script does not reach synthexis (value stays 0), while it worked on the probe. Diagnosed with logging in the vendored nih-plug VST3 wrapper (`~/Library/Logs/synthexis-nih.log`, enabled by the marker file `synthexis-nih.enable`): FL never forwards the call. No `setParamNormalized`, no process parameter change, no read-back reaches the plugin; FL's `getParamValue` answers from its own mirror. `general.processRECEvent(getRecEventId(ch) + REC_Chan_Plugin_First + idx, v * 2^30, REC_MIDIController)` arrives as `setParamNormalized` (dropped by nih-plug while processing, by design) plus a process parameter change (applied), so it works; the bridge and the gate use it. Why FL drops `setParamValue` for synthexis is open. Pending experiment (hypothesis): synthexis declares no MIDI input, the probe did; a build with `MIDI_INPUT = Basic` is staged and installs when FL is closed.
2. Writing without the window opening once is solved for VST3 by the nih-plug patch; CLAP and AU not checked.
3. Latency compensation for recording: FL compensates audio inputs and has a global note offset, but nothing per plugin for automation (FL manual, audio settings). Proposal: the plugin records takes with `t_capture` timestamps and exports them shifted by the measured latency (automation clip / .mid drag-out); live: grid quantization for note triggers, optional motion prediction (hypothesis).
4. iPhone 60 fps format delivered 30 fps.
5. Licence: nih-plug's VST3 export links GPLv3 bindings, so distributed VST3 binaries are GPLv3; source stays MIT.

## Spec updates for the main session

- probes P1, P2, P3: done (results above); P2 winner = VST3 main-thread edits.
- D1: add "host parameter edits are dispatched to the main thread".
- `budget.latency`: camera to parameter in FL 116-126 / 142-145 ms (median / p95); built-in camera floor about 80-90 ms light to callback (derived); iPhone 100 / 116 ms light to callback.
- D12: EfficientViT-B0 does not convert to CoreML.
- `plugin/` is owned by this line from now on; the skeleton satisfies the Welle 3 gate.

## Commands

- Plugin: `cd plugin && ./install.sh` (guarded build, sign, install VST3 + CLAP).
- Bridge: `probes/fl-camera-plugin/bridge/flbridged &`, one-time FL setup in `bridge/README.md` (input "synthexis bridge" with controller type "synthexis bridge", port 77; output "synthexis bridge return", port 77). After editing the FL script, FL reloads it only when the controller type is switched to another and back.
- FL gate: `python3 probes/fl-camera-plugin/bridge/gate_run.py <fresh pattern number>`.
- Camera bench: `probes/camera-latency/build.sh`, then `open -W -n CamLat.app --args --label X --flash 1 --out <abs dir>`; `analyze.py`.

## Commits

b93a69c probe v2, 65a00cd H1 + H3 export, fc88c1e bridge, 2090634 H2, 0f8fb86 P3, fd6f44a plugin skeleton, ecba4b7 Welle 3 gate.
