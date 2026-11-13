# Logic AU probe: result

2026-11-13, macOS 27.0.1 (26A434), Logic Pro 12.4 (`com.apple.logic10`), M1 Pro.
Build: synthexis 0.1.1 from branch `claude/logic-au-probe-105c23` (camera guard,
`MIDI_INPUT = Basic`, AUv2 through clap-wrapper 0.3.1). Question: does the
Audio Unit load in Logic, where does it run, and what happens with the camera?

## Short answer

- The Audio Unit builds and passes `auval -v aumu sgnl Snxs`.
- Logic 12.4 runs third-party AUs out of process on this Mac. A crash in
  synthexis would take down `AUHostingServiceXPC`, not Logic.
- Logic declares no camera use, and neither does the hosting service. The
  camera guard keeps synthexis from asking there; checked in a minimal AU
  host with the same missing key, not yet inside Logic.
- Loading synthexis inside Logic was not run: the screen was busy (below).

## Logic checklist (docs/host-support.md)

| Step | Result |
|---|---|
| 1. Baseline | `plutil -p "/Applications/Logic Pro.app/Contents/Info.plist" \| grep -i camera` prints nothing (Logic 12.4). Before the install `auval -v aumu sgnl Snxs` had nothing to find. |
| 2. Scratch project | Not possible without closing Noa's open project: Logic has one project open at a time, and File > New asked to save or delete the unsaved "Ohne Titel" project. Cancelled; a new Software Instrument track ("Deluxe Classic") was added to the open project instead. Backup of the project folder taken before. |
| 3. auval | Passes (section below). Logic itself scans AUs only at launch: `~/Library/Caches/AudioUnitCache/Logs/AUScan*.plist` shows scans at 15:46 and 15:56 (8 validator processes, 908 AUs); synthexis was installed at 16:24, so the running Logic has not scanned it yet. Needs a Logic restart or Plug-in Manager, Reset & Rescan. |
| 4. TCC log | `log stream --predicate 'subsystem == "com.apple.TCC"'` ran; with synthexis never loaded in Logic it has nothing about synthexis. |
| 5. Load, hosting process | Not measured: screen busy. Measured without synthexis: Logic holds no third-party `.component` binary; `AUHostingServiceXPC` and `AUHostingServiceXPC_arrow` (parent launchd, started with the project) hold them. Their entitlements: `com.apple.security.cs.disable-library-validation`, `allow-unsigned-executable-memory`, `temporary-exception.audio-unit-host`; no app sandbox, no camera entry; no `NSCameraUsageDescription` in their Info.plist. |
| 6. CoreMIDI ports from the AU | Not measured in Logic. In the test host below the AU opens its "synthexis" bridge ports (or reports that another host owns them while FL has synthexis open). |
| 7. Camera | Not measured in Logic. Guard verified in the test host (section below). |
| 8. Signal automation | Not measured: screen busy. |
| 9.-12. Learn, notes, reopen | Not measured: screen busy (9 and 10 also need the LearnBridge build, step 7 of the plan). |

Why the UI steps did not run: Logic's plug-in menus and its plug-in search
popover close at once while Logic is in the background, so loading the AU
needs full-screen computer use. The parallel FL session held the full-screen
lock from about 16:30 to past 17:28, so the steps above stay open. Still
open for the next run, in this order: restart Logic or Rescan, load synthexis
on a Software Instrument track, `lsof` on the AUHostingService processes,
the MIDI port list, the window's camera status line, Signal 1 automation in
Latch with "Test signal".

The test track was deleted again at 17:25 (Track > Delete Track); the track
list matches the state before the probe. Noa's project was not saved or
closed.

## FL Studio recheck of the VST3

Not measured: the parallel FL session had FL Studio open with its own build
the whole time. `MIDI_INPUT = Basic` changes the VST3 bus layout (an event
input bus appears), so FL loading and recording need one check before
release.

## Build and auval

- `cargo xtask bundle synthexis --release` now also writes
  `target/bundled/synthexis.component` (`xtask/src/auv2.rs`): the CLAP binary,
  an Info.plist with one AudioComponent (`aumu sgnl Snxs`, version 0x101,
  factory `GetPluginFactoryAUV2`, `sandboxSafe` false), ad-hoc signature.
- The binary exports `clap_entry`, `GetPluginFactory` (VST3, nih-plug) and
  `GetPluginFactoryAUV2` (clap-wrapper). Param ids stay `sig01..sig32`:
  the AU parameters are the CLAP ones.
- `auval -v aumu sgnl Snxs`: AU VALIDATION SUCCEEDED. Cold open 733 ms (the
  plugin starts its engine threads), warm 0.3 ms. 0 audio inputs, 1 stereo
  output, Cocoa view available, parameter set and schedule pass, MIDI test
  passes. One warning: "Preset name is not retained in retrieved class data".
- `auval -a` crashes on this Mac (exit 139) before it lists anything useful:
  the crash reports are in "Auburn Sounds Inner Pitch" (`getTLSRange`), not in
  synthexis. `auval -v` with the codes is the check that works here.

## Camera guard outside Logic

`auhost.swift` is a minimal in-process AU host with no Info.plist, so it has no
`NSCameraUsageDescription`, like Logic. It instantiates `aumu sgnl Snxs`,
opens the Cocoa view (nih-plug's egui view appears inside clap-wrapper's
NSView) and waits. synthexis.log:

```
host declares camera use: false
host app declares no camera use, not asking for access
```

No TCC prompt, no crash. `editor.rs` shows `noHostAccess` as the
`cam_no_host` text in the camera status line (not looked at on screen).

A first version of the probe released its `AVAudioUnit` right after the
callback, which closed the AU and with it the editor (backtrace:
`WrapAsAUV2::~WrapAsAUV2` -> `ext_gui_destroy`). Hosts must keep the unit;
that was the probe's bug, not the plugin's.
