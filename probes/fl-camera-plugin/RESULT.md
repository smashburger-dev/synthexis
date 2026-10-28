# fl-camera-plugin RESULT (von Noa auszufüllen)

Send back: `~/Library/Logs/synthexis-probe.log` Inhalt + diese Felder.

## Headless verifiziert (Ich)

- Release-Build + Bundling OK: `synthexis-probe.vst3`, `synthexis-probe.clap`
  (ad-hoc signiert via xtask). nih-plug @ de42101.
- Installiert nach `~/Library/Audio/Plug-Ins/VST3/` und `.../CLAP/`.
- Standalone .app lief: Kamera-Auth `notDetermined` → Prompt → `authorized`,
  Session 1280x720, **~18.9 fps** delivered, capture→callback latency
  **med ~98–114 ms, p95 ~152–215 ms**, Luminanz ändert sich mit Szene
  (lum 0.15–0.35 bei Handbewegung vor der Kamera).
- "editor opened, GuiContext captured" wurde im Standalone geloggt
  (nih-plug spawned dort ein Fenster).
- Kein separater pluginval/clap-validator-Run (kein fertiges Binary ohne
  großen Download gefunden; Bundles laden in nih-plug's eigenem Loader).
- Versionen: rustc/cargo via rustup, nih-plug de42101, egui-baseview rev
  ec70c3f, baseview 9a0b42c.

## Probe v2 (2026-10-28, H2), headless verifiziert

- Neue Parameter: Source (Luminance, Triangle 4 s), Path (A Hintergrund-Thread, B Main-Thread, D MIDI-CC 20 über virtuelle CoreMIDI-Quelle "synthexis"), Gesture (Per write, Held). Weg C = CLAP-Bundle mit Path A. Ein 30-Hz-Pump-Thread schreibt, nicht mehr der Kamera-Callback.
- Standalone (`SYNTHEXIS_PROBE_TEST=...`, `-r 44100`), Log `~/Library/Logs/synthexis-probe.log`:
  - A: `thread=synthexis-pump writes_per_s=30`, min 0,00 max 1,00.
  - B: `thread=main writes_per_s=30`, auch mit Gesture Held.
  - D: Quelle "synthexis" angelegt, bevor der Editor offen war; `tools/midi-listen 12`: 30 CC/s, nur CC 20, Kanal 1, Werte 0 bis 127 (362 Nachrichten).
- Kamera-Diagnose im Log: Gerät, activeFormat (Preset 1280x720 läuft auf dem Geräteformat 1920x1080 420v, 15 bis 30 fps), `alwaysDiscardsLateVideoFrames=true`, Synchronisationsuhr = Host-Uhr (Offset 0,000 ms), Video-Effekte. Latenz jetzt `Host-Zeit − CMSyncConvertTime(PTS, syncClock, hostClock)`; in Standalone identisch zur alten Rechnung, die alte Zahl war also kein Uhrenfehler.
- Build: `./guarded-cargo.sh xtask bundle synthexis-probe --release` (Peak 960 MB), `codesign --force --deep -s -` auf beide Bundles, installiert nach `~/Library/Audio/Plug-Ins/{VST3,CLAP}/`. Log-Zeile `synthexis probe v0.2.0 build=948fc05`.
- Checkliste für FL: [README.md](README.md).

## Auszufüllen von Noa (v0.1)

| Feld | Antwort |
|------|---------|
| FL findet und lädt "synthexis probe" (VST3? CLAP?) | |
| Kamera-Prompt kam, hieß "FL Studio"? | |
| Vorschau + fps/Latenz im Editor sichtbar? | |
| Signal 1 bewegt sich (Drive open-only)? | |
| Automation aufgenommen mit Editor offen? | |
| Automation aufgenommen mit Editor geschlossen (Drive=Always)? | |
| MIDI test: Synth bekam Note 60 pro Beat? Aufgenommen? | |
| Auffälligkeiten / Crash / Freeze | |
