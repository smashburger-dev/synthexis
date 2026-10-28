# fl-script-bridge RESULT (von Noa)

Send back: den Script-Output von `dump` + `sweep-rec` (Copypaste) + Felder.

| Feld | Antwort |
|------|---------|
| Script im MIDI-Settings Dropdown sichtbar? | |
| dump: Plugin-Name + Param-Count + Liste gekommen? | |
| sweep (CC20/setParamValue): Regler bewegt? Automation aufgezeichnet? | |
| sweep-rec (CC23/processRECEvent): Regler bewegt? Automation aufgezeichnet? | |
| `[bridge]` Zeilen mit API-Oberfläche (REC_*, plugins API) — bitte paste | |
| Auffälligkeiten | |

Was der Code aufruft: CC20 → `plugins.setParamValue(P, v/127, chan)`;
CC23 → `general.processRECEvent(REC_PlugBase + chan<<16 + P, v*65535/127,
REC_FromMIDI)` (Formel im Script kommentiert — falls falsch, liefert der
Dump die echten REC_*-Konstanten).

## Ergebnis 2026-10-28 (gemessen über die FL-Bridge, ohne IAC)

Statt des CC-Skripts dieser Probe lief die SysEx-Bridge aus `probes/fl-camera-plugin/bridge/` (eigene virtuelle CoreMIDI-Ports, kein IAC nötig). Skript: `probes/fl-camera-plugin/bridge/p3_set.py`, Ergebnis `bridge/runs/p3_set_pattern40.json` (nicht im Repo).

| Feld | Antwort |
|------|---------|
| Script im MIDI-Settings Dropdown sichtbar? | ja ("synthexis bridge", FL MIDI scripting version 45) |
| Plugin-Name, Param-Liste | ja: `channels` liefert Kanal- und Plugin-Namen, `params` die Parameternamen des VST3 (Signal 1, Drive, Source, Path, Gesture) |
| setParamValue: Regler bewegt? Automation aufgezeichnet? | ja und ja: 184 Schreibvorgänge mit 20 Hz als Dreieck während der Aufnahme (Pattern 40), danach Abspielen ohne Skript: Spanne 0,987, 127 verschiedene Werte; Kontrolle vorher flach (0,000) |
| processRECEvent | nicht getestet (setParamValue reicht) |

Einschränkung: Das Ziel-Plugin war synthexis probe (VST3 im Fruity Wrapper, aus FLs Sicht ein Drittanbieter-Plugin). Ein kommerzielles Drittanbieter-Plugin wurde nicht getestet.
