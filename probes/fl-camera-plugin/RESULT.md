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

## H2 in FL Studio 2026, gemessen über die FL-Bridge (2026-10-28)

Methode (`bridge/h2_run.py`, FL-Skript `bridge/device_synthexis_bridge.py`): Pro Bedingung frisches Pattern, Kontrolle mit Drive = Off (muss flach sein), Plugin-Fenster 1 s auf und wieder zu, dann Drive = Always, Aufnahme 10 s mit Automation-Filter, Stop. Schiedsrichter: Drive = Off, Abspielen, Signal 1 über `plugins.getParamValue` 8 s lang mit 20 Hz auslesen. Aufgenommen = Spanne > 0,3 und mindestens 20 verschiedene Werte. Das Plugin-Log bestätigt, dass während der Abspielphase keine Schreibvorgänge liefen.

| Bundle | Weg | Gesture | Fenster | Spanne beim Abspielen | Werte | Ergebnis | Pattern |
|---|---|---|---|---|---|---|---|
| VST3 | A Hintergrund-Thread | per write | zu | 0,000 | 1 | nicht aufgenommen | 10 |
| VST3 | B Main-Thread | per write | zu | 0,983 | 156 | aufgenommen | 11 |
| VST3 | A | held | zu | 0,000 | 1 | nicht aufgenommen | 12 |
| VST3 | B | held | zu | 0,995 | 157 | aufgenommen | 13 |
| VST3 | A | per write | offen | 0,996 | 155 | aufgenommen | 30 |
| CLAP (Weg C) | A | per write | zu | 0,015 | 2 | nicht aufgenommen | 20 |
| CLAP | B | per write | zu | 0,034 | 2 | nicht aufgenommen | 21 |
| CLAP | A | held | zu | 0,018 | 2 | nicht aufgenommen | 22 |
| CLAP | B | held | zu | 0,174 | 3 | nicht aufgenommen | 23 |
| CLAP | A | per write | offen | 0,994 | 155 | aufgenommen | 24 |

Ergebnis: Gate erfüllt. FL nimmt Automation bei geschlossenem Plugin-Fenster auf, wenn das VST3 die Änderungen auf dem Main-Thread meldet (Weg B). Aus einem Hintergrund-Thread (Weg A) nimmt FL nur bei offenem Fenster auf, das CLAP-Bundle ebenso. Weg D wurde nicht getestet, weil B ohne jede Einrichtung funktioniert.

Offen für das Produkt: Die Wege A bis C brauchen den GuiContext von nih-plug, den es erst gibt, wenn das Fenster seit dem Start von FL einmal offen war. Der Lauf hat das Fenster dafür je Bedingung 1 s geöffnet. Das echte Plugin muss ohne diesen Schritt schreiben können.

Läufe: `bridge/runs/*.json` (nicht im Repo). Kommandos: `bridge/flbridged &`, dann `python3 bridge/h2_run.py --channel-name "synthexis probe"` bzw. `--channel-name "synthexis probe clap" --pattern-base 20`.

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

## Welle-3-Gate: synthexis-Grundgerüst in FL (2026-10-28)

- Plugin `plugin/` (VST3, nih-plug mit `host_context_available`-Patch). Log `~/Library/Logs/synthexis.log`: `host context available (api=Vst3)` 1 ms nach dem Laden, bevor FL das Fenster automatisch öffnete; Noa schloss es sofort. Danach schrieb das Plugin bei geschlossenem Fenster.
- Gate (`bridge/gate_run.py 52`): Kontrolle flach (0,000), Aufnahme 10 s mit Live an, Abspielen mit Live aus: Signal 32 (Testdreieck) Spanne 0,744, 158 Werte, aufgenommen. Gegenprobe mit `/tmp/pb.py`-Logik auf Pattern 50 und dem leeren Pattern 51: Pattern 50 spielt Signal 1 (Helligkeit, Spanne 0,061), Signal 2 (Bewegung, 0,039) und Signal 32 (0,754) ab, Pattern 51 ist flach. Die Kamera-Signale landen also als Automation in FL.
- Kamera bis Parameter in FL: 116 bis 126 ms Median, 142 bis 145 ms p95 (Log-Fenster je 300 Schreibvorgänge); davon Kamera-Auslieferung pts_to_cb 117 bis 135 ms Median, Merkmale 0,7 ms Median.
- Offener Fehler: `plugins.setParamValue` aus dem FL-Skript kommt bei synthexis nicht an (Wert bleibt 0), beim Probe-Plugin ging es (P3). `general.processRECEvent(getRecEventId(ch) + REC_Chan_Plugin_First + idx, v * 2^30, REC_MIDIController)` funktioniert; der Gate-Lauf nutzt diesen Weg. Ursache unbekannt.
- Erster Gate-Lauf (Pattern 50) meldete fälschlich "flat": der Lauf sprang vor dem Abspielen nicht ins aufgenommene Pattern zurück und prüfte den Record-Status nicht. Behoben in `gate_run.py`.
