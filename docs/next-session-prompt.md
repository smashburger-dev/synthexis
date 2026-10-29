# Prompt für die nächste Hauptsession (ab 30.10.)

Diesen Block in eine neue Devin-Session kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.

---

Du arbeitest an **synthexis** (Meta-Hackathon, Indie-Track, Abgabe 4.11. 21:00 ET). Du planst und entscheidest, der Sidekick baut und misst.

Lies zuerst `AGENTS.md`, `docs/roadmap.md`, `docs/session-2026-10-29-labels.md`, `probes/label-image-mode/RESULT.md` und in `spec/synthexis.yaml` die Abschnitte `train.*`, `budget.*` und D9 bis D12. Lade `unslop` und `build-rust`.

**Stand:** S1 und S2 sind gelabelt, im Bildmodus `grouped` (`run.py --mode image`). Der Worker liest die Bilder jetzt parallel vor. Ob das S3 auf etwa 1 $ drückt, ist eine Hypothese; miss es an `io_s` und ms pro Bild und Begriff. Das Oktober-Guthaben ist verbraucht. S3 und S4 zahlt Noa aus seinem 12,50-$-Puffer (nach dieser Session noch etwa 9 $), ab 1.11. gibt es 30 $ Modal-Guthaben für das Training.

**Aufgaben, in dieser Reihenfolge:**
1. S3 labeln, sobald Noa es aufgenommen hat: iPhone, seitlich, ohne Brille. Ordner `data/recordings/2026-10-30-S3`. Prüfe vorher in `objects.json`, welche Objekte Noa in Block E benutzt hat (Bilder aus E_* ansehen), und setze bei Abweichungen Prompts pro Session. Grenze 2,50 $. Kontaktbogen selbst ansehen. Danach S4 genauso (kurzes Protokoll, Split `heldout`).
2. Packer und Mac-Preflight laut `train.setup`. Lizenz jeder vortrainierten Gewichtsdatei prüfen. Entscheidung, ob die Augenklasse für S2 aus dem Loss fällt (Brille), mit Noa klären.
3. Apple Vision Handkeypoints offline auf S1 bis S4 als Ziele für `perc.hand` (lokal, kostenlos). Nicht laufen lassen, während Noa aufnimmt.
4. D9 bis D12 mit Noa entscheiden, damit das Training am 1.11. starten kann.
5. FigJam-Diagramme zu P2/P3 und zum Automationsweg aus der Spec neu erzeugen.

**Parallel:** Eine Mapping-Session kann laufen (`docs/mapping-session-prompt.md`). Sie besitzt `tools/mapping/**` und `data/mapping/**` und schickt Spec-Änderungen an diese Session. Fasse diese Pfade nicht an.

**Regeln:** keine Secrets ausgeben, keine Aufnahmen ins Repo, jeder bezahlte Lauf mit Grenze und den drei Sperren, Zahlen nur mit Messung (Median, p95), sonst Hypothese. Speicherplatz prüfen: Am 29.10. waren nur 12 GiB frei. Am Ende `docs/roadmap.md` abhaken, Digest schreiben und nach Noas Freigabe committen.
