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

## Auszufüllen von Noa

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
