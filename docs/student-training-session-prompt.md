# Prompt für die Trainings-Session der visuellen Schülermodelle (ab 2.11.)

Diesen Block in eine neue Session kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis` auf dem zusammengeführten `main`.

---

Du arbeitest an **synthexis** (Meta-Hackathon, Indie-Track, Abgabe 4.11. 21:00 ET, Demo-Video und öffentliches MIT-Repo am 3.11.). Deine Aufgabe ist das Training der beiden visuellen Schülermodelle `synthexis-seg` und `synthexis-hand`, von der Prüfung der Daten bis zum CoreML-Export und der Messung auf S4. Plane, entscheide mit Noa, baue, miss und mach es in einer sauberen Runde fertig.

**Zuerst lesen:** `AGENTS.md`, `docs/roadmap.md`, `docs/merge-2026-11-02.md`, in `spec/synthexis.yaml` die Abschnitte `perc.seg`, `perc.hand`, `train.*`, `budget.*` und D9 bis D12, dann `probes/train-packer/RESULT.md`, `probes/hand-keypoints/RESULT.md`, `probes/licence-check/RESULT.md`, `probes/motion-labels/RESULT.md` und `probes/motion-labels/noa-review-2026-11-02.md`. Lade `unslop`.

**Voraussetzungen, vor allem anderen prüfen:**
1. `main` enthält die Branches der Sessions vision-keypoints und train-packer mit ihrem letzten Stand (`docs/merge-2026-11-02.md` nennt sie). Fehlt einer, stopp und sag es Noa.
2. Die Hand-Keypoints kommen von SAM 3D Body (Noa hat seit 2.11. Zugang zum Hugging-Face-Repo), MediaPipe Hand Landmarker ist der kostenlose Gegencheck. Apple Vision darf nirgends als Lernziel auftauchen: Apples Bedingungen (DPLA 3.2) verbieten das Training mit Ausgaben von Apple-Modellen. Prüfe das im Manifest von `data/packed/`.
3. `data/packed/manifest.json` passt zu den Keypoints, die in `probes/hand-keypoints/RESULT.md` als final stehen. Sonst zuerst neu packen: `uv run tools/train/pack.py --sets hand`.
4. Der Mac-Preflight besteht auf `main`: `uv run tools/train/preflight.py` (64 Samples, 20 Schritte, CoreML-Export, kostenlos).

**Datenstand, den du kennen musst:**
- Vier Sessions mit 47.367 SAM-gelabelten Frames. S1 und S2 sind mit der FaceTime-Kamera aufgenommen, S2 mit Brille. S3 kommt vom iPhone, 20 bis 30 Grad seitlich, mit Studiolicht. S4 ist FaceTime mit Mütze und kurzem Protokoll und liegt komplett im Held-out-Set. Dev-Split = ganze Segmente pro Session, nie einzelne Frames.
- Keine Aufnahme ist gespiegelt, auch S3 nicht (am Daumen und am Arm geprüft). Die Bewegungslabels aus Block H (`throw_right`, `swipe_left`, `circle_cw`) beziehen sich auf die Nutzersicht im gespiegelten Preview, wie `hand.x` in `tools/mapping/ontology.yaml`. Auf den Aufnahmen müssen x und cw/ccw gespiegelt werden.
- Count-Segmente (`B_*_count*`) tragen keine Fingerlabels, weil Noa dort bis zum Ziel hochzählt.
- Die Spark-QA-Flags schließen keine Labels aus. Noas Stichprobe auf S3: 3 von 24 Flags stimmten, alle 10 geprüften "ok" stimmten. Ausgeschlossen sind nur S3 `D_step_back`, S3 `F_backlit` und der Teil von S3 `F_dim` vor dem Lichtausschalten.
- Offen aus der Roadmap: Fällt die Augenklasse für S2 aus dem Loss? S2-Augenmasken sind wegen der Brille etwa 2,2-mal zu groß.

**Entscheidungen mit Noa, gleich zu Beginn in einer Frage:** D9 bis D12 haben in der Spec je eine gewählte Option, Status `proposed`:
- D9: zweistufige Destillation über einen Proxy-Teacher
- D10: Vollbild 384x216 plus 128-px-Crops für Gesicht und Hände
- D11: Vorframe als Zusatzkanal plus Konsistenz-Loss
- D12: MobileNetV4-Conv-Small, RepViT-M0.9 als Herausforderer

Dazu kommt die Frage zur S2-Augenklasse. Hol Noas Ja oder seine Änderung ein und trag das Ergebnis in die Spec ein (`status: chosen`).

**Ablauf:**
1. Pilot auf Modal L4 mit den Sperren aus `train.setup`: Pilot-Epoche mit Neuschätzung und Abbruch über dem Limit, Laufzeit-Sperre, Checkpoint pro Epoche auf dem Volume, wiederaufnehmbar. Miss Samples pro Sekunde und $ pro Epoche, bevor du größer wirst.
2. Proxy-Teacher trainieren (D9) und alle Frames mit weichen Wahrscheinlichkeiten labeln.
3. Schüler `seg` und `hand` trainieren. `hand` lernt 21 Keypoints (SAM 3D Body) und 5 Finger-Logits aus dem Skript.
4. Auf S4 gegen `budget.quality` messen, pro Klasse und pro Block. Die Zahlen gehören in ein RESULT, auch die schlechten.
5. CoreML FP16 exportieren und die ANE-Latenz auf dem Mac messen (Roadmap H3: Median und p95, End-to-End pro Frame gegen `budget.latency`).
6. `probes/student-training/RESULT.md` schreiben, die Roadmap abhaken, einen Digest `docs/session-2026-11-0X-training.md` anlegen, nach Noas Freigabe committen. Push nur mit seinem Ja.

**Budget:** Modal-Guthaben ab 1.11.: 30 $ (`budget.money` in der Spec). Im November liefen darauf schon SAM-Labels für S3 und S4 (2,00 $), Parser-Läufe und SAM 3D Body. Lies den Stand aus Modals Billing oder dem Ledger der Tools, bevor du rechnest, und setz jedem bezahlten Lauf ein `--max-usd` mit den drei Sperren. Echtes Geld über das Guthaben hinaus nur mit Noas Ja.

**Lizenzen:** Die Schüler werden mit einer Kopie der SAM License ausgeliefert. Für die timm-Gewichte (Apache-2.0) gehört der Hinweis in `docs/credits.md`. Zusätzlich muss "Built with Muse Spark Open" dort stehen, siehe `probes/licence-check/RESULT.md`.

**Regeln:** keine Secrets ausgeben, keine Aufnahmen oder Frames ins Repo (`data/` zeigt Noas Gesicht), Zahlen nur mit Messung, sonst als Hypothese kennzeichnen. Speicherplatz prüfen, bevor du packst oder Checkpoints lädst. Antworten an Noa auf Deutsch, Code und Kommentare auf Englisch.
