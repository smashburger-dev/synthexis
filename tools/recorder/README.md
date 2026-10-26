# synthexis recorder

1. App starten: `open tools/recorder/"synthexis recorder.app"` (Doppelklick geht auch).
2. Protokoll wählen (full = S1–S3, short = S4), Session-ID eintragen (S1, S2, S3, S4),
   Kamera wählen (eine reicht, optional zweite), Licht/Notizen eintragen.
3. **Start** — beim ersten Mal Kamera-Prompt erlauben.
4. Während der Aufnahme: große Anweisung befolgen. Tasten: **B** = Segment als
   schlecht markieren, **Leertaste** = Pause (Aufnahme läuft weiter, Pause
   gehört zu keinem Segment), **Esc** = Stopp & speichern.
5. Dateien landen in `synthexis/data/recordings/<datum>-<session>/`:
   cam0.mov (cam1.mov), segments.json, session.json.
6. Ende-Bildschirm zeigt Segmente, schlechte Marks, Dateigrößen, gemessene fps.

Beim ersten Start über `open` fragt macOS nach der Kamera für "synthexis recorder": erlauben.
Nach jedem Neu-Bauen der App kann die Frage noch einmal kommen. Startet nichts, geht auch:
`"tools/recorder/synthexis recorder.app/Contents/MacOS/recorder"` im Terminal.
Log: `~/Library/Logs/synthexis-recorder.log`.

Tipp: gutes Licht. Die App fixiert 30 fps, bei wenig Licht wird das Bild dann dunkler und rauschiger.
`--protocol-file <pfad>` lädt ein alternatives Protokoll, `--auto` startet
sofort (für Tests).
