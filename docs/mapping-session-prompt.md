# Prompt für eine parallele Mapping-Session

Diesen Block in eine neue Devin-Session kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.

---

Du arbeitest an **synthexis** (Meta-Hackathon, Abgabe 4.11. 21:00 ET), und zwar nur am Mapping-Assistenten. Parallel läuft die Hauptsession, sie kümmert sich um Daten, Training und Plugin.

Lies zuerst `AGENTS.md`, `docs/roadmap.md` (Abschnitt "Mapping assistant") und in `spec/synthexis.yaml` den Abschnitt `map.*` sowie `bridge.harvest`, `bridge.fl_script` und `budget.money`. Lade `unslop` und `codebase-design`.

**Ziel dieser Session:** Die Bauzeit-Pipeline mit Muse Spark Open läuft einmal komplett durch, für die erste Version von param-sense. Diese Pipeline trägt auch den Bonuspreis (`map.bonus_prize`).
1. Taxonomie festziehen (`map.taxonomy`, etwa 40 Klassen plus `unsafe`).
2. Parameter-Ernte der installierten Plugins: Namen, Bereiche, Einheiten, Wertetexte. Wege: das FL-Skript über die Test-Bridge (`probes/fl-camera-plugin/bridge/`, nur lesend benutzen, gehört der Hill-Climb-Linie) oder ein Host auf dem Mac. Bei jedem Werkzeug die Lizenz prüfen. Es läuft nur zur Bauzeit und wird nicht ausgeliefert.
3. Spark Open labelt die Ernte mit Taxonomie-Klasse und sicherem Bereich. Modell `meta-models/muse-spark-open-1026` auf Baseten, Key im Keychain `synthexis.baseten`. Jede Anfrage verbraucht 60 bis 80 Reasoning-Tokens, also 50 bis 100 Parameter pro Anfrage bündeln und kein knappes `max_tokens` setzen. Anzahl der Anfragen und Tokens mitschreiben (kommt in README und Demo).
4. Spark Open schreibt synthetische Befehle auf Deutsch und Englisch aus bekannten strukturierten Edits (`map.parser`). Die Labels sind damit per Konstruktion bekannt.
5. param-sense v1 trainieren (Zeichen-n-Gramme plus linear/MLP, Ziel laut Spec), Eval auf Plugins unbekannter Hersteller. Noa schreibt die 100 Eval-Befehle für `map.eval` selbst, nie trainieren.

**OWNED PATHS:** `tools/mapping/**`, `data/mapping/**` (git-ignoriert), `probes/mapping-*/**`. Nicht anfassen: `spec/`, `plugin/`, `tools/labels/`, `data/labels/`, `data/frames/`. Spec-Änderungen schreibst du als Vorschlag in `probes/mapping-*/SPEC_PROPOSAL.md`, die Hauptsession übernimmt sie.

**Geld:** Baseten hat 100 $. Erste Durchläufe mit Grenze 5 $, vorher Kosten aus einer Probe mit 1 Anfrage hochrechnen. Kein Modal.

**Regeln:** Nur neuer Code ab 26.10., nichts aus anderen Lifemaxxing-Projekten kopieren. Keine Secrets ausgeben. Credit "Built with Muse Spark Open" (Muse-Lizenz, `map.license`). Zahlen nur mit Messung. Am Ende `docs/roadmap.md` (nur Mapping-Abschnitt) abhaken, Digest `docs/session-<datum>-mapping.md` schreiben, nach Noas Freigabe committen.
