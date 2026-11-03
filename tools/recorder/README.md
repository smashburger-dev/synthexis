# synthexis recorder

Build (compiles, bundles the protocol, signs ad-hoc, runs a self-test; exit 0 = ready):
`tools/recorder/build.sh`. After each build macOS asks once more for the camera.

1. App starten: Doppelklick auf `tools/recorder/synthexis recorder.app` (oder `open` im Terminal).
2. Protokoll wählen (full = S1–S3, short = S4), Session-ID eintragen (S1, S2, S3, S4),
   Kamera wählen (eine reicht, optional zweite), Licht/Notizen eintragen.
   Der Startbildschirm zeigt schon vor Start ein Livebild je gewählter Kamera.
   Darunter steht die Statuszeile: grün `● live … fps · 1920x1080` = Kamera liefert 16:9 ab 1280x720
   (die Datei wird immer 1280x720), orange nennt die Abweichung (z. B. `liefert 1920x1440, nicht 16:9`
   oder `nur 18 fps: mehr Licht`), rot heißt `keine Bilder`. Erst Start drücken,
   wenn die Zeile grün ist. `Kameras neu suchen` findet ein gerade angestecktes
   Gerät ohne Neustart (Stecken/Ziehen wird aber auch automatisch erkannt).
   Orange Warnungen bei falscher Kamera oder falschem Protokoll für die Session
   blockieren Start nicht. Der freie Plattenplatz steht immer unten; rot unter 5 GB.
3. **Start**, beim ersten Mal Kamera-Dialog erlauben.
4. Während der Aufnahme: große Anweisung befolgen. Tasten: **B** = Segment als
   schlecht markieren, **Leertaste** = Pause (Aufnahme läuft weiter, Pause
   gehört zu keinem Segment), **Esc** = Stopp & speichern. Die Tasten wirken nur während der Aufnahme.
   Stoppt eine Kamera die Bilder (> 1 s) oder wird abgezogen, erscheint ein rotes
   Banner `KAMERA <name> LIEFERT KEINE BILDER` — die Aufnahme läuft weiter.
5. Dateien landen in `~/Movies/synthexis/recordings/<datum>-<session>/`
   (cam0.mov, cam1.mov, segments.json, session.json). `data/recordings` im Repo ist ein Symlink dorthin.
   Grund: Das Repo liegt auf dem Schreibtisch, den macOS pro App sperrt.
   session.json zählt pro Kamera zusätzlich `stalls_over_1s` und `disconnected`.
6. Ende-Bildschirm zeigt Segmente, schlechte Marks, Dateigrößen, gemessene fps.

Log: `~/Library/Logs/synthexis-recorder.log`. Optionen: `--protocol-file <pfad>` lädt ein
anderes Protokoll, `--session <id>` setzt die Session, `--auto` startet sofort (Tests),
`--selftest` prüft Protokoll und Zielordner ohne Fenster, `--list-cameras` listet
Kameras mit Typ, ID und Format-Auflösung ohne Fenster.

S3 (iPhone als Kamera):
- iPhone und Mac: gleiche Apple-ID, WLAN und Bluetooth an.
- iPhone quer und fest montiert, Nicht-stören-Modus an, Ladekabel dran.
- In `Kamera 0` taucht es als `<name> · iPhone (Continuity)` auf und wird für S3
  automatisch gewählt. Erst Start drücken, wenn die Statuszeile grün zeigt.

Tipp: gutes Licht. Die App fixiert 30 fps, bei wenig Licht wird das Bild dunkler und rauschiger.
