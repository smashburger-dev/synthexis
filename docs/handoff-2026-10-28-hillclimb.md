# Übergabe an die Hauptsession (aus der Hill-Climb-Linie, 28.10.)

Diesen Block in die Hauptsession oder eine neue Session kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.

---

Du arbeitest an **synthexis** (Meta-Hackathon, Abgabe 4.11. 21:00 ET). Die parallele Hill-Climb-Session vom 28.10. ist abgeschlossen. Lies zuerst `docs/session-2026-10-28-hillclimb.md` (Digest mit allen Zahlen und Quellen), dann `AGENTS.md` und `docs/next-session-prompt.md` für deinen eigenen Plan. Lade `unslop` und `build-rust`.

**Was jetzt feststeht:**
- P1, P2, P3 bestanden. Automation in FL Studio 2026 läuft über VST3-Parameter-Edits auf dem Main-Thread; FL nimmt sie auch bei geschlossenem Plugin-Fenster auf. CLAP nimmt mit geschlossenem Fenster nicht auf. Der MIDI-Umweg (Link to controller) ist nicht nötig.
- Welle 3 ist erledigt: `plugin/` enthält das Grundgerüst (Kamera-Engine, Signalvertrag, Konditionierung, 32 Makros, Handrollen-Parameter, Main-Thread-Schreibweg, Latenz-Log) mit einem kleinen nih-plug-Patch, damit das Plugin ohne geöffnetes Fenster schreibt. Gate in FL bestanden: Kamera-Signale und Testdreieck werden als Automation aufgenommen. `plugin/` gehört ab jetzt nicht mehr zu Welle 3 der Hauptsession; baue darauf auf statt neu.
- Kamera bis Parameter in FL: 116 bis 126 ms Median, 142 bis 145 ms p95, fast alles Kamera-Auslieferung. iPhone über Continuity ist schneller (100/116 ms Licht bis Callback gegen 154/178 ms bei der eingebauten Kamera).
- H3 (Schüler-Latenz) ist bewusst verschoben bis nach dem ersten Training; Exportcheck: MobileNetV4-Small, 0,5-Variante und RepViT-M0.9 konvertieren bei 384x216 und 320x180, EfficientViT-B0 nicht.

**Deine Aufgaben aus dieser Übergabe:**
1. Spec aktualisieren (Abschnitt "Spec updates" im Digest): P1 bis P3 done, D1-Zusatz Main-Thread, `budget.latency`, D12 ohne EfficientViT-B0, Welle-3-Status.
2. FigJam-Diagramme, die P2/P3 oder den Automationsweg zeigen, aus der Spec neu erzeugen.
3. Offene Punkte aus dem Digest in die Spec übernehmen: `setParamValue`-Fehler bei synthexis (FL leitet den Aufruf nicht ans Plugin weiter, REC-Weg funktioniert; Stand im Digest unter "Open"), Aufnahme-Latenzkompensation als Feature (Takes mit Zeitstempel, verschoben exportieren; Noten optional quantisieren), iPhone liefert nur 30 fps.
4. Dann weiter mit deinem eigenen Plan: S2/S3/S4 labeln, Spark-Open-Zeitleiste, Trainings-Packer und Preflight, Training ab 1.11.

**Werkzeuge, die es jetzt gibt:** FL-Bridge (`probes/fl-camera-plugin/bridge/`, Einrichtung in dessen README), damit Agenten FL selbst testen können; Gate-Lauf `bridge/gate_run.py`; Kamera-Messbank mit Bildschirm-Blitz (`probes/camera-latency/`).

**Regeln wie immer:** keine Secrets, keine Aufnahmen ins Repo, ein Cargo-Prozess zur Zeit, Zahlen nur mit Messung (Median, p95), sonst Hypothese.
