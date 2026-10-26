# fl-camera-plugin — Checkliste für Noa (P1, P2, P6)

1. FL Studio neu starten (falls offen), dann **Options → File settings →
   Plugin search & management → Find more plugins / Verify plugins** rescan
   lassen.
2. "synthexis probe" im **Channel Rack** laden (steht unter Instrumenten,
   VST3 oder CLAP).
3. Beim Laden: **Kamera-Dialog erlauben** (er sollte "FL Studio" nennen,
   FL hat NSCameraUsageDescription). Editor öffnen → Kameravorschau und
   Zahlen prüfen.
4. Im Editor **Drive = "Editor open only"** setzen, winken → "Signal 1"-Slider
   muss sich bewegen. Automation von Signal 1 aufzeichnen (5–10 s winken).
5. Editor schließen, **Drive = "Always"**, Automation nochmal aufzeichnen —
   jetzt ohne Editor.
6. **MIDI test** anhaken. Auf dem "synthexis probe"-Kanal in den
   Channel-Settings einen **MIDI-Output-Port** setzen (z. B. 5). Auf dem
   Ziel-Kanal (beliebiger Synth) **MIDI-Input-Port** auf dieselbe Nummer.
7. Transport laufen lassen + aufnehmen → der Synth sollte C4 (Note 60) auf
   jedem Beat bekommen.
8. Log und Antworten einsammeln: `~/Library/Logs/synthexis-probe.log` und
   RESULT.md unten ausfüllen.
9. Optional: Standalone-Test ohne FL —
   `open probes/fl-camera-plugin/synthexis-probe-standalone.app`
10. Log-Datei an mich zurückschicken (Inhalt reicht als Paste).

Hinweise: Das Plugin gibt Stille aus. nih-plug ist gepinnt auf Commit
`de421011f41a6d10fc8c7a6084e4f4dee0143683`. Bundles sind ad-hoc signiert.
