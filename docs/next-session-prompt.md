# Prompt für die nächste Session (27.10.)

## Was Noa vorher oder parallel macht

1. FL-Projekte speichern. Dann die Hardware-Kits in dieser Reihenfolge, je Kit die README befolgen und die RESULT-Felder ausfüllen:
   1. `probes/fl-camera-plugin/` (Kamera im Plugin, Automation, MIDI-Out)
   2. `probes/headphone-motion/` (Powerbeats Kopfbewegung)
   3. `probes/heart-rate-ble/` (Powerbeats Herzfrequenz)
   4. `probes/fl-script-bridge/` (vorher IAC-Treiber aktivieren; die Ausgabe des Dump-Befehls kopieren)
2. Aufnahmen mit `tools/recorder/` (README dort): S1 und S2 mit der eingebauten Kamera, S3 mit dem iPhone. Gutes Licht. S4 erst am 28.10.
3. Entscheiden: Darf es einen ausdrücklichen Opt-in-Button "Neue Plugins mit Muse Spark Open scannen" geben (online)? Grund: der Bonuspreis "Best Build on Meta Open Weight Models" prüft, ob das Projekt auf Spark Open oder Glimmer läuft.
4. Den Prompt unten in eine neue Devin-Session kopieren, geöffnet im Ordner `/Users/no8/Desktop/life/Lifemaxxing/synthexis`, und die Logs/Antworten aus Schritt 1 dazu einfügen.

---

## Prompt

Du arbeitest an **synthexis**, Noas Beitrag zum Meta Global AI Developer Hackathon (Indie-Track, Abgabe 4.11. 21:00 ET = 5.11. 03:00 MEZ). Projekt: `/Users/no8/Desktop/life/Lifemaxxing/synthexis`, Repo `smashburger-dev/synthexis` (privat bis 3.11.). Harness: Devin, mit Sidekick für Umsetzung und Verifikation; du planst und entscheidest, der Sidekick baut und misst.

Lies zuerst, in dieser Reihenfolge: `AGENTS.md`, `docs/session-2026-10-26-plan-and-probes.md`, `spec/synthexis.yaml`, `docs/plan.md`, `docs/hillclimb.md`, `docs/recording-protocol.md`. Lade die Skills `unslop`, `build-rust`, `design-glass`, `design-rust`.

**Ziel heute:** Die Daten-Pipeline läuft mit echten Gewichten und Noas Aufnahmen, die Hardware-Fragen sind beantwortet, und das Plugin-Grundgerüst bringt ein Kamera-Signal als aufnehmbare Automation in FL Studio.

### Welle 1: Fakten einsammeln
- Noas Hardware-Ergebnisse (unten eingefügt) in die RESULT-Dateien und `spec/synthexis.yaml` (Probes P1 bis P6, Status, Entscheidungen) übernehmen. Den REC-Pfad der FL-Bridge aus seinem Dump korrigieren.
- `probes/sam31-modal/sam31_real.py` schreiben und auf H100 laufen lassen: Secret `huggingface`, Volume `sam31-weights` als Cache, 20 Frames x 5 Prompts im Bildmodus mit Overlays und Kontaktbogen nach `data/sam31_real/`, Videomodus pro Konzept über 300 Frames mit ms/Frame, Objektzahl und Kosten. Grenze 5 $. Den Kontaktbogen selbst ansehen, bevor Labels darauf aufbauen.
- Gate: Die Masken für Hand, Gesicht, Mund und Person sitzen auf dem Kontaktbogen sichtbar richtig. Die Zahlen stehen in RESULT.md.

### Welle 2: Labels aus Noas Aufnahmen
- Pipeline unter `tools/labels/`: Frames mit 15 fps aus `data/recordings/*/cam*.mov`, Upload in ein privates Modal-Volume, SAM 3.1 im Videomodus pro Konzept (person, face, eye, mouth, hand und die Objektnamen aus Block E), Masken plus Manifest. Labels aus `segments.json` mit den Rändern `margin_s`; als schlecht markierte Abschnitte fliegen raus. S4 bleibt Held-out und wird nie trainiert.
- Gate: Manifest mit Frame-, Masken- und Label-Zahlen pro Session. Eine Stichprobe von 30 Frames als Kontaktbogen, selbst angesehen.

### Welle 3: Plugin-Grundgerüst
- `plugin/` (Rust, nih-plug, VST3 + CLAP, egui): eine Kamera-Engine pro Host-Prozess, der Signalvertrag aus der Spec (Wert 0..1, Konfidenz, Zeit), die Konditionierungskette (One-Euro-Filter, Halten, Wiederaufnahme), 32 Makros. Vorerst Platzhalter-Features (Helligkeit, Bewegung), bis die Schüler trainiert sind. Wahrnehmung nie im Audio-Thread. Design nach design-glass und chiasmas STYLE.md, neu geschrieben.
- Gate: In FL nimmt ein Makro Automation auf, die von der Kamera bewegt wird. Messung von der Kamera bis zum Parameter (Median, p95) im Log.

### Regeln
- Keine Secrets ausgeben, loggen oder committen. Aufnahmen und Datensätze nie ins Repo.
- Nur neue Arbeit ab 26.10.; nichts aus chiasma oder anderen Projekten kopieren.
- Jede Zahl braucht eine Messung auf echter Hardware oder echter GPU, mit Median und p95; sonst als Hypothese markieren. Hill-Climb-Läufe kommen in `docs/hillclimb.tsv`.
- Budgets: Modal höchstens 100 $ insgesamt, Meta-API höchstens 150 $; pro Lauf eine Grenze im Brief.
- Ein Cargo-Prozess zur Zeit, über `probes/fl-camera-plugin/guarded-cargo.sh`.
- Commits und Pushes ins private Repo sind erlaubt, sobald ein Schritt geprüft ist.

### Fertig, wenn
Welle 1 und 2 ihre Gates bestanden haben und Welle 3 mindestens die FL-Automation zeigt; Spec, Plan und Session-Digest sind aktualisiert und gepusht. Abschlussbericht an Noa auf Deutsch: Ergebnis zuerst, Zahlen, was offen ist, was er als Nächstes tun muss.

### CONTINUATION (danach, laut Bauplan)
- 29.10. bis 30.10.: Schüler v1 auf Modal trainieren (synthexis-seg mit MobileNetV4-Small + LR-ASPP, synthexis-hand mit MobileNetV3-Small, Keypoint-Labels offline von Apple Vision), CoreML-Export, Eval-Gate auf S4, dann Hill-Climb.
- 30.10. bis 1.11.: Gesten und MIDI-Patterns, Snapshot-Anker mit der SAM-3.1-API, Kopfhörer und Herzfrequenz.
- 1.11. bis 2.11.: FL-Bridge, Parameter-Ernte, param-sense und Befehls-Parser (offline, Fallback mit klickbaren Begriffen), "Built with Muse Spark Open" in UI und Doku.
- 2.11.: UI-Politur. 3.11.: Demo-Projekt und 3-Minuten-Video, Repo öffentlich, Ansicht prüfen. 4.11.: Abgabe, X-Post.
- FigJam-Diagramme 1, 4 und 5 aus der Spec neu erzeugen (Stand vor D3 und vor der Offline-Entscheidung).

### Noas Ergebnisse von heute
<hier Logs und RESULT-Antworten einfügen>
