# Prompt für die nächste Session (ab 28.10.)

## Was Noa vorher oder parallel macht

1. S2 aufnehmen: Recorder per Doppelklick, Protokoll `full` (jetzt 14,8 min mit Block H), Session `S2`, Tageslicht und anderes Shirt, möglichst ohne lange Pausen. Für Block E dieselben Objekte wie in S1 (Dose, Handy, Stift, Ball, Lufterfrischer).
2. Hardware-Probes P1 bis P5 nach den READMEs in `probes/` (FL-Kamera, Powerbeats Kopfbewegung, Herzfrequenz, FL-Bridge). Logs als Paste unten einfügen.
3. Modal-Dashboard: tatsächlich abgerechnete Oktober-Kosten notieren.
4. Diesen Prompt in eine neue Devin-Session kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.

---

## Prompt

Du arbeitest an **synthexis**, Noas Beitrag zum Meta Global AI Developer Hackathon (Indie-Track, Abgabe 4.11. 21:00 ET = 5.11. 03:00 MEZ). Repo `smashburger-dev/synthexis` (privat bis 3.11.). Du planst und entscheidest, der Sidekick baut und misst.

Lies zuerst: `AGENTS.md`, `docs/session-2026-10-27-data-engine.md`, `spec/synthexis.yaml` (besonders `motion`, `gest.*`, `train.setup`, `budget.money`, Entscheidungen D9 bis D12), `docs/recording-protocol.md`. Lade die Skills `unslop`, `build-rust`, `design-glass`, `design-rust`.

**Ziel:** S2 gelabelt, Spark-Open-Zeitleiste für den Freestyle läuft, Hardware-Fragen beantwortet, Plugin-Grundgerüst bringt ein Kamera-Signal als aufnehmbare Automation in FL Studio, Trainings-Packer und Mac-Preflight stehen bereit für das Training ab 1.11.

### Welle 1: Labels fertig machen
- S1: Konzept `mug` mit dem Prompt "soda can" neu rechnen (der leere Cache dieses Konzepts muss dafür ungültig werden), Grenze 0,20 $.
- S2 mit 15 fps auf L40S labeln, `--max-usd 7`. 15 fps bleiben, weil sich Handmasken zwischen Nachbarbildern stark ändern (Median-IoU 0,79, im Freestyle 0,58; Digest). Kontaktbogen selbst ansehen.
- Gate: Manifest und Kontaktbogen geprüft, Kosten mit Modal-Dashboard abgeglichen.

### Welle 2: Spark Open für Freestyle und Segment-Prüfung (`motion.labels`)
- Bildstreifen mit 2 fps und SAM-Overlay je Freestyle-Segment, Muse Spark Open auf Baseten schreibt eine Zeitleiste im Vokabular von `motion.primitives` plus Freitext-Namen. Dazu 3 Bilder pro Skript-Segment gegen die Anweisung prüfen, Abweichungen als Liste für Noa. Anfragen bündeln (Reasoning-Tokens), Kostengrenze 3 $.
- Gate: Noa prüft 20 Zeitleisten-Einträge und die Abweichungsliste.

### Welle 2b: Probe P2 klären (vor Welle 3)
Stand 27.10. (Noa in FL 2026, `~/Library/Logs/synthexis-probe.log`): P1 bestanden, Kamera im VST3 in FL mit 29,7 bis 30 fps, Capture-Latenz 126 / 142 ms (Median / p95, Kamera-Zeitstempel bis Callback). MIDI-Out zu Analog Lab klingt (P6), aber FL nahm weder die Noten ins Pattern noch sichtbare Automation auf. Der Log kann P2 nicht entscheiden, weil die Probe Parameter-Schreibvorgänge nicht protokolliert.
- Probe v2: jeden Schreibvorgang zählen und mit Thread-Name loggen (pro Sekunde Anzahl, Wertebereich); Testsignal mit großem Hub (Dreieck 0..1 alle 4 s, unabhängig vom Licht) zusätzlich zur Helligkeit; Option, `performEdit` auf den Main-Thread zu verlegen (VST3 verlangt den UI-Thread, die Probe schreibt bisher aus dem Kamera-Thread); prüfen, ob FL das CLAP-Bundle lädt (CLAP schickt Parameter-Events aus dem Audio-Thread, der saubere Weg ohne offenes Fenster).
- Für Noa eine Checkliste: Drive steht standardmäßig auf Off; Aufnahme-Filter mit Automation; wo FL aufgenommene Plugin-Parameter ablegt (Event-Daten im Pattern oder Automation-Clip).
- MIDI-Aufnahme: vermutlich nimmt FL die MIDI-Ausgabe eines Plugins nicht direkt ins Pattern auf (unbestätigt, Doku prüfen). Plan B fürs Produkt: das Plugin nimmt seine eigenen Noten auf und bietet sie per Drag-and-drop als .mid an.
- Gate: in FL eine aufgenommene Kurve von Signal 1, einmal mit offenem, einmal mit geschlossenem Fenster.

### Welle 3: Plugin-Grundgerüst
- `plugin/` (Rust, nih-plug, VST3 + CLAP, egui): eine Kamera-Engine pro Host-Prozess, Signalvertrag, Konditionierungskette, 32 Makros, Rolle pro Hand (`gest.hand_role`) als Einstellung, vorerst Platzhalter-Features (Helligkeit, Bewegung). Wahrnehmung nie im Audio-Thread. Design nach design-glass und chiasmas STYLE.md, neu geschrieben.
- Gate: In FL nimmt ein Makro Automation auf, die von der Kamera bewegt wird. Messung Kamera bis Parameter (Median, p95) im Log.

### Welle 4: Training vorbereiten (`train.setup`)
- Packer: Shards à etwa 2.000 Beispiele, Vollbild 384x216, Hand- und Gesichtsausschnitte 128 px aus dem Originalbild, Dev-Split nach ganzen Segmenten. Mac-Preflight (64 Beispiele, 20 Schritte, CoreML-Export). Lizenz jeder vortrainierten Gewichtsdatei prüfen.
- D9 bis D12 mit Noa entscheiden.

### Regeln
- Keine Secrets ausgeben, loggen oder committen. Aufnahmen und Datensätze nie ins Repo.
- Nur neue Arbeit ab 26.10.; Ideen aus Papern (EfficientSAM3, MobileSAM2, EUPE) neu implementieren, keinen Code kopieren.
- Jede Zahl mit Messung, Median und p95; sonst Hypothese. Läufe in `docs/hillclimb.tsv`.
- Jeder bezahlte Lauf mit Grenze und den drei Kostensperren. Budget: Modal-Credits 25 $ bis 31.10. (etwa 5,7 $ übrig), 30 $ ab 1.11., plus bis 12,50 $ Puffer von Noa; Baseten 100 $; Meta-API 150 $.
- Ein Cargo-Prozess zur Zeit. Commits ins private Repo nach jedem geprüften Schritt.

### Fertig, wenn
Welle 1 und 2 ihre Gates bestanden haben, Welle 3 die FL-Automation zeigt und Welle 4 den Preflight grün hat; Spec und Session-Digest aktualisiert und gepusht. Abschlussbericht an Noa auf Deutsch: Ergebnis zuerst, Zahlen, was offen ist, was er als Nächstes tun muss.

### Danach laut Bauplan
- 1.11.: Schüler v1 auf Modal (L4), Eval auf S4, Hill-Climb. S3 ab 1.11. labeln.
- 30.10. bis 1.11.: Bewegungsschicht (Primitive, Pong, Orb, Motive), Gesten und MIDI, Kopfhörer und Herzfrequenz.
- 1.11. bis 2.11.: FL-Bridge, Parameter-Ernte mit Spark Open, param-sense und Befehls-Parser, "Built with Muse Spark Open" in UI und Doku.
- 2.11. UI-Politur. 3.11. Demo-Video, Repo öffentlich. 4.11. Abgabe.

### Noas Ergebnisse
<hier Logs und Antworten einfügen>
