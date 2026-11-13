# Übergabe: Bridges für Logic Pro und Ableton Live (13.11.)

Diesen Block in eine Session auf Noas Mac kopieren, geöffnet in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`. Die Bridges laufen lokal, weil das Plugin nur auf macOS baut und Logic und Live dort laufen.

---

Du arbeitest an **synthexis**. Ziel dieser Session: die Bridges für Logic Pro und Ableton Live bauen und in beiden DAWs durchtesten. Lies zuerst `AGENTS.md`, dann `docs/host-support.md` (Plan, Schritte 5 bis 13), `docs/session-2026-11-13-stock-labels.md` (letzter Stand) und die beiden Probes `probes/ableton-stock/RESULT.md` und `probes/logic-stock/RESULT.md`. Lade `build-rust`. Baue und teste nur über `plugin/guarded-cargo.sh`.

**Was schon da ist:**
- FL läuft komplett über `plugin/fl/device_synthexis_bridge.py` (SysEx-JSON, Protokoll 5). Plugin-Identität ist FL-förmig (`fl::Key`, `fx:`/`gen:`-Ids in Routen, Presets, Profilen, Pin).
- PR 7 (`claude/logic-au-probe-105c23`, offen): Kamera-Schutz, MIDI-Input-Bus für Live (`MIDI_INPUT = Basic`), AUv2-Build über clap-wrapper. `auval` besteht; in Logic selbst noch nicht geladen (`probes/logic-au/RESULT.md`). Zuerst mergen oder darauf aufbauen, und `Cargo.toml` auf 0.1.3 heben (CHANGELOG und README stehen schon auf 0.1.3).
- PR 9 (gemergt): Labels für 71 Live-Geräte und 87 Logic-Plug-ins in `plugin/assistant/completion/known.tsv`. Der Vertrag für die Bridges:
  - Live: Plugin-Schlüssel `"ableton:" + Device.class_name`, Parametername `DeviceParameter.original_name` (nicht `name`: Presets benennen Geräte um, Meld hat `MeldVoice_*`-Ids). `Classes::of` in `sense.rs` bekommt genau diese Strings.
  - Logic: Plugin-Schlüssel `"logic:" + Plug-in-Name` wie im Logic-Menü; die Parameternamen sind Beschriftungen aus Apples Handbuch, Logics Automationsnamen sind noch ungeprüft.

**Reihenfolge (aus `docs/host-support.md`):**
1. Vorab auf dem Mac: `cd plugin && ./guarded-cargo.sh test -p synthexis` (für PR 9 nicht gelaufen, weil die Cloud nicht baut).
2. HostBridge-Refactor (Schritt 5): Trait mit `link`, `view`, `want_params`, `write`, `notes`, `caps`; `fl.rs` unverändert hinter `FlBridge`; versionierter `TargetKey { Fl, Ableton, Learn }`, der alte `fx:`/`gen:`-Ids migriert.
3. Bridge-Auswahl (Schritt 6): Hostname aus VST3 `IHostApplication::getName` / CLAP `host->name`, manueller Override in den Einstellungen.
4. LearnBridge (Schritt 7) für Logic und Live: CoreMIDI-Quellen "synthexis control" (CC) und "synthexis notes", CC = round(v*127), Wiggle-Knopf pro Route für den Learn-Modus, All-Notes-Off beim Stopp. In Logic gegen die Checkliste in `docs/host-support.md` testen, mit Kanal-EQ als erstem Ziel.
5. Logic-Probe für Automationsnamen: einen Plug-in öffnen, die Automationsparameter-Liste und Controller Assignments lesen und mit `tools/mapping/logic/manual_names.tsv` vergleichen; Ergebnis nach `probes/logic-stock/RESULT.md`.
6. AbletonBridge (Schritt 11): Remote Script `plugin/live/synthexis_bridge/`, gleiches F0 7D JSON F7 Protokoll 5 mit 16-KB-Seiten, LOM-Inventar (Tracks, Geräte, `class_name`, `original_name`, Index, min, max, Wertetexte), Fokus-Listener, Schreiben über script-gebaute MIDI-Mappings. Zuerst gegen nachgebaute `Live`-Module testen wie `plugin/fl/test/`, dann in einer Live-12-Suite-Testversion (30 Tage gratis). Offene Frage: nimmt Live Automation auf, wenn ein Script `DeviceParameter.value` setzt? Messen.
7. Mit dem Live-Dump die 124 UNVERIFIED-Namen und die fehlenden Parameter in `tools/mapping/ableton/` nachziehen.
8. README-Tabelle "DAW support" und `docs/host-support.md` nur mit gemessenen Ergebnissen aktualisieren.

**Offen außerhalb der Bridges:** Kontakts `#000`-Parameter brechen den `known.tsv`-Parser in `sense.rs` (als eigener Task vorgeschlagen). Die FL-Stock-Synths bekommen ihre Labels in der Cloud-Session aus `tools/mapping/fl/stock_names.py`.

**Regeln wie immer:** keine Secrets, keine Aufnahmen ins Repo, ein Cargo-Prozess zur Zeit, Zahlen nur mit Messung (Median, p95), sonst Hypothese; nichts mergen ohne Noa.
