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
