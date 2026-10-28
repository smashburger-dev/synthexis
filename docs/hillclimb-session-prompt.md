# Prompt für die parallele Hill-Climb-Session (ab 28.10.)

Läuft parallel zur Hauptsession aus `docs/next-session-prompt.md`. Alles hier ist lokal und kostet kein Cloud-Geld.

---

## Prompt

Du arbeitest an **synthexis** (Meta-Hackathon, Abgabe 4.11. 21:00 ET), Projekt `/Users/no8/Desktop/life/Lifemaxxing/synthexis`. Diese Session macht nur Hill-Climbs auf Dingen, die heute ohne trainierte Modelle und ohne Cloud messbar sind. Eine zweite Session arbeitet parallel an Labels, Spark Open, dem Plugin-Grundgerüst und dem Trainings-Packer. Du planst und entscheidest, der Sidekick baut und misst.

Lies zuerst: `AGENTS.md`, `docs/session-2026-10-27-data-engine.md`, `docs/hillclimb.md`, `spec/synthexis.yaml` (`budget.latency`, `perception`, D3, D9 bis D12), `probes/fl-camera-plugin/README.md` und `RESULT.md`, `probes/coreml-student-latency/RESULT.md`. Lade die Skills `unslop` und `build-rust`.

### Besitz (keine Überschneidung mit der Hauptsession)
- OWNED: `probes/fl-camera-plugin/**`, `probes/camera-latency/**` (neu), `probes/coreml-student-latency/**`, `docs/hillclimb.tsv`.
- Nicht anfassen: `spec/`, `plugin/`, `tools/`, `docs/` außer `hillclimb.tsv`. Ergebnisse stehen in den RESULT.md der Probes; die Hauptsession überträgt sie in die Spec.

### Regeln
- Protokoll aus `docs/hillclimb.md`: eine Zeile pro Lauf in `docs/hillclimb.tsv`, eine Änderung pro Lauf, Median und p95, Kommando im RESULT.md. Annahme nur, wenn der Gewinn über der Lauf-zu-Lauf-Streuung liegt (Streuung mit 3 Wiederholungen bestimmen).
- Ein Cargo-Prozess zur Zeit über `probes/fl-camera-plugin/guarded-cargo.sh`. Keine Messung, während ein Build oder ein anderer Messlauf läuft.
- Nur neue Arbeit ab 26.10., kein kopierter Code. Keine Secrets, keine Aufnahmen ins Repo.
- Commits ins private Repo nach jedem geprüften Schritt, nur eigene Pfade stagen.

### H1: Kamera-Latenz
Ausgangslage (Noa in FL 2026, 27.10., `~/Library/Logs/synthexis-probe.log`): 29,7 bis 30 fps, Capture-Latenz 126 / 142 ms (Median / p95), gemessen als Host-Zeit beim Callback minus Präsentations-Zeitstempel des Bildes. Das frisst fast das ganze Latenzbudget.
- Erst die Messung prüfen: Zeitstempel und Uhr (Host-Clock vs. Session-Clock) gegen Apple-Doku verifizieren, damit die Zahl wirklich Kamera-Pipeline ist.
- Dann je Lauf eine Stellschraube: activeFormat statt sessionPreset, kleinere Auflösung, 60-fps-Format falls vorhanden, Pixelformat 420v statt BGRA, `alwaysDiscardsLateVideoFrames`, Callback-Queue-Priorität, Belichtungsmodus (kurze Belichtung bei gutem Licht), Continuity Camera als Vergleich.
- Ziel (Hypothese): Median unter 60 ms, p95 unter 80 ms. Licht notieren, weil die Kamera bei wenig Licht länger belichtet.

### H2: P2 Automation, vier Wege
Stand: Drive war auf "Editor open only"; FL zeigte keine aufgenommene Automation. Die Probe schreibt `performEdit` aus dem Kamera-Thread (VST3 verlangt den UI-Thread) und loggt keine Schreibvorgänge. Signal 1 folgte der Helligkeit, im Betrieb nur 0,50 bis 0,63.
- Probe v2: Schreibvorgänge pro Sekunde mit Thread-Name und Wertebereich loggen; Testsignal Dreieck 0..1 alle 4 s als Quelle wählbar.
- Weg A: wie bisher (Kamera-Thread). Weg B: Schreibvorgänge per Dispatch auf den Main-Thread. Weg C: CLAP-Bundle in FL (lädt FL 2026 CLAP? Doku prüfen), Parameter-Events aus dem Audio-Thread. Weg D: das Plugin öffnet eine eigene virtuelle CoreMIDI-Quelle "synthexis" und sendet CC; in FL "Link to controller" (so macht es Motion2MIDI, dort aber über den IAC-Treiber). FL nimmt Bewegungen verlinkter Controller als Automation auf.
- Für Noa eine Checkliste je Weg (Editor offen und geschlossen): wo die Kurve in FL landen müsste, Aufnahme-Filter, was er zurückschickt.
- Gate: mindestens ein Weg nimmt mit geschlossenem Editor eine Kurve auf, bestätigt von Noa.

### H3: Schüler-Latenz auf der Neural Engine (zufällige Gewichte)
- Kandidaten nach D10 bis D12: Segmentierer MobileNetV4-Conv-Small, RepViT-M0.9, EfficientViT-B0 mit LR-ASPP bei 384x216 und 320x180, mit und ohne Zusatzkanal "vorige Maske" (D11); Gesichts- und Handkopf auf 128-px-Ausschnitten.
- Messen: CoreML FP16 auf der ANE, Median, p95, p99, Speicher; dazu der ganze Wahrnehmungs-Frame (Segmentierer + 2 Hände + Gesicht + Geometrie) gegen das Budget `perception_frame_p95_ms: 5`.
- Lizenz jeder vortrainierten Gewichtsdatei notieren (für die spätere Wahl).

### Fertig, wenn
H1 eine angenommene Verbesserung oder eine begründete Untergrenze hat, H2 eine Checkliste für Noa mit allen vier Wegen gebaut hat und H3 die Kandidatentabelle steht. Abschlussbericht an Noa auf Deutsch: Ergebnis zuerst, Zahlen, was er in FL testen muss.
