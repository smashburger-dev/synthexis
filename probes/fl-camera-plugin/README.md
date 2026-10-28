# fl-camera-plugin: Checkliste für Noa (Probe v2, H2)

Frage: Nimmt FL Studio 2026 eine Automationskurve von "Signal 1" auf, während der Plugin-Editor geschlossen ist? Probe v2 testet vier Wege. Ein Weg reicht, wenn er mit geschlossenem Editor eine Kurve liefert.

| Weg | Was das Plugin tut | Bundle | Einstellung im Plugin |
|---|---|---|---|
| A | schreibt Signal 1 aus einem Hintergrund-Thread (`synthexis-pump`) | VST3 | Path = A |
| B | schreibt Signal 1 auf dem Main-Thread | VST3 | Path = B |
| C | wie A, aber im CLAP-Bundle; FL bekommt die Werte aus dem Audio-Thread | CLAP | Path = A |
| D | sendet MIDI-CC 20 (Kanal 1) über eine eigene MIDI-Quelle "synthexis"; FL steuert Signal 1 per "Link to controller" | VST3 | Path = D |

Testsignal: Source = Triangle (0 bis 1 und zurück, alle 4 s). Gesture = Per write. Nur wenn A, B oder C mit "Per write" nichts aufnehmen, denselben Weg mit Gesture = Held wiederholen.

## Vorbereitung (einmal)

1. FL Studio komplett beenden und neu starten. Das alte Plugin v0.1 steckt noch im laufenden FL.
2. Options → Manage plugins → Find installed plugins. "synthexis probe" muss als VST3 und als CLAP auftauchen.
3. Neues leeres Projekt. "synthexis probe" (VST3) in den Channel Rack laden, Editor öffnen. Im Log-Bereich unten muss `synthexis probe v0.2.0 build=...` stehen.
4. Aufnahme-Filter: Rechtsklick auf den Record-Button, "Automation" muss angehakt sein.
5. Wo die Kurve landet: FL nimmt Automation in das ausgewählte Pattern auf, nicht in einen Automation Clip. Prüfen über Browser → Current project → Generators → synthexis probe → Rechtsklick auf "Signal 1" → "Edit events". Eine Kurve ist da, wenn der Event Editor ein Dreieck zeigt. Im Playlist-Pattern-Clip erscheint sie als Hintergrundbild.

## Ablauf pro Weg A, B, C

Für jeden Weg zwei Aufnahmen von je 10 s:

1. Editor offen: Drive = "Editor open only", Source = Triangle, Path wie in der Tabelle. Der Slider "Signal 1" muss sich bewegen. Record an, Play, 10 s laufen lassen, Stop.
2. Editor geschlossen: Drive = "Always", Editor schließen. Record an, Play, 10 s, Stop.
3. Nach jeder Aufnahme in "Edit events" von Signal 1 nachsehen: Dreieck ja oder nein.

Für C: das CLAP-Bundle "synthexis probe" in einen zweiten Kanal laden (das VST3 vorher löschen oder Drive dort auf Off), dann wie oben mit Path = A.

Wichtig: Das Plugin braucht den Editor einmal offen, bevor A, B und C schreiben können. Danach darf er zu sein.

## Ablauf Weg D (MIDI)

1. Options → MIDI settings (F10). In der Input-Liste "synthexis" auswählen und Enable anklicken. Fehlt der Eintrag: Path im Plugin einmal auf D stellen, Drive auf Always, dann "Refresh device list".
2. Im Plugin: Path = D, Source = Triangle, Drive = Always. Die MIDI-Aktivitätsanzeige in FLs Hauptleiste muss blinken.
3. Verknüpfen: Drive kurz auf Off. Multilink-Knopf (Ctrl+J) an, im Plugin den Slider "Signal 1" mit der Maus kurz bewegen, dann Drive auf Always. FL verknüpft den ersten eintreffenden CC mit Signal 1. Falls das nicht greift: Browser → Current project → Generators → synthexis probe → "Signal 1" anklicken, dann Drive auf Always.
4. Der Slider muss jetzt dem Dreieck folgen, auch bei geschlossenem Editor.
5. Editor geschlossen: Record an, Play, 10 s, Stop. In "Edit events" von Signal 1 nachsehen.

## Was du zurückschickst

Pro Zeile ja oder nein:

| Weg | Editor offen: Kurve? | Editor zu: Kurve? | Gesture Held nötig? |
|---|---|---|---|
| A (VST3, Hintergrund-Thread) | | | |
| B (VST3, Main-Thread) | | | |
| C (CLAP) | | | |
| D (MIDI-CC, Link to controller) | n/a | | n/a |

Dazu den Inhalt von `~/Library/Logs/synthexis-probe.log` ab der Zeile `synthexis probe v0.2.0`. Die Zeilen `write path=... thread=... writes_per_s=...` zeigen, ob und von welchem Thread geschrieben wurde. Die Zeilen `device`, `active_format`, `video_effects` und `capture fps=...` brauche ich für die Kamera-Latenz (H1): FL maß am 27.10. 126 ms, die Standalone-App heute 53 ms.

## Hinweise

- Das Plugin gibt Stille aus. nih-plug ist auf Commit `de421011f41a6d10fc8c7a6084e4f4dee0143683` gepinnt. Bundles sind ad-hoc signiert.
- Headless-Test ohne FL: `SYNTHEXIS_PROBE_TEST="drive=always,source=tri,path=d" probes/fl-camera-plugin/synthexis-probe-standalone.app/Contents/MacOS/synthexis-probe -r 44100` (ohne `-r 44100` bricht der Standalone auf diesem Mac mit einem Buffer-Assert ab). MIDI prüfen mit `probes/fl-camera-plugin/tools/midi-listen 10`.
