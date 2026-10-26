# fl-script-bridge — Checkliste für Noa (P3)

1. **IAC aktivieren**: Audio-MIDI-Setup → MIDI-Studio → IAC Driver →
   "Device is online" anhaken (Bus 1).
2. FL Studio neu starten (Script liegt schon unter
   `Settings/Hardware/synthexis bridge probe/`).
3. **MIDI settings**: IAC Driver Bus 1 als Input aktivieren und ihm den
   "synthexis bridge probe"-Controller-Typ zuweisen (drop-down neben dem
   Input).
4. Drittanbieter-Synth auf einen Kanal laden und **Kanal selektieren**.
5. `uv run send_cc.py dump` — Script-Output-Fenster prüfen: Plugin-Name,
   Param-Count, 200 Params gelistet?
6. `uv run send_cc.py set-index 5` (oder ein passender Index aus dem Dump).
7. `uv run send_cc.py sweep` — bewegt sich der Regler im Plugin-GUI?
8. FL auf Automation-Recording stellen, `sweep` nochmal laufen lassen.
9. `uv run send_cc.py sweep-rec` — dieselbe Frage für den REC-Pfad.
10. Script-Output als Copypaste zurück + Antworten in RESULT.md.

Headless geprüft: IAC offline (`raw_iac_ports.log`), Script nach Hardware-
Ordner installiert, send_cc.py läuft (uv, mido+python-rtmidi).
