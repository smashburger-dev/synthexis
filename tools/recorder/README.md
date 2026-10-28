# synthexis recorder

Build (compiles, bundles the protocol, signs ad-hoc, runs a self-test; exit 0 = ready):
`tools/recorder/build.sh`. After each build macOS asks once more for the camera.

1. App starten: Doppelklick auf `tools/recorder/synthexis recorder.app` (oder `open` im Terminal).
2. Protokoll wählen (full = S1–S3, short = S4), Session-ID eintragen (S1, S2, S3, S4),
   Kamera wählen (eine reicht, optional zweite), Licht/Notizen eintragen.
3. **Start**, beim ersten Mal Kamera-Dialog erlauben.
4. Während der Aufnahme: große Anweisung befolgen. Tasten: **B** = Segment als
   schlecht markieren, **Leertaste** = Pause (Aufnahme läuft weiter, Pause
   gehört zu keinem Segment), **Esc** = Stopp & speichern. Die Tasten wirken nur während der Aufnahme.
5. Dateien landen in `~/Movies/synthexis/recordings/<datum>-<session>/`
   (cam0.mov, cam1.mov, segments.json, session.json). `data/recordings` im Repo ist ein Symlink dorthin.
   Grund: Das Repo liegt auf dem Schreibtisch, den macOS pro App sperrt.
6. Ende-Bildschirm zeigt Segmente, schlechte Marks, Dateigrößen, gemessene fps.

Log: `~/Library/Logs/synthexis-recorder.log`. Optionen: `--protocol-file <pfad>` lädt ein
anderes Protokoll, `--session <id>` setzt die Session, `--auto` startet sofort (Tests),
`--selftest` prüft Protokoll und Zielordner ohne Fenster.

Tipp: gutes Licht. Die App fixiert 30 fps, bei wenig Licht wird das Bild dunkler und rauschiger.
