# fl-resize: Checkliste für Noa

Frage: Nimmt FL Studio 2026 eine Fenstergröße an, die das Plugin selbst anfragt? Das neue synthexis-Fenster plant seine Höhe nach den offenen Sections und fragt den Host danach (Spec `docs/design/ui-redesign.md`, Abschnitt 2.8). Die Probe fragt dreimal: 1200 x 720, 1200 x 820, 1200 x 900.

Sie misst zwei Größen, weil nur eine davon zählt:

- egui view: die eigene NSView des Plugins. egui-baseview vergrößert sie bei jeder Anfrage selbst, auch wenn FL ablehnt. Im VST3-Format sagt sie darum nichts über FL.
- host view: die NSView, die FL dem Plugin gibt. Die zählt. Bleibt sie klein, schneidet FL das Plugin ab.

Urteil pro Anfrage: "reached after N ms, stayed" heißt angekommen innerhalb von 500 ms und 3 s gehalten. "refused" heißt nach 500 ms nicht da. "late" heißt später als 500 ms angekommen; das Fenster behandelt das wie eine Ablehnung. "snapped back" heißt angekommen und wieder zurückgesprungen.

## Vorbereitung (einmal)

1. FL Studio beenden.
2. `probes/fl-resize/install.sh` ausführen. Es baut die Probe, signiert sie ad hoc und kopiert `fl-resize-probe.vst3` und `fl-resize-probe.clap` nach `~/Library/Audio/Plug-Ins/VST3` und `.../CLAP`. Bei laufendem FL bricht es ab.
3. FL starten, Options → Manage plugins → Find installed plugins. "fl resize probe" muss als VST3 und als CLAP erscheinen.
4. Neues leeres Projekt.

## Ablauf VST3

1. "fl resize probe" (VST3) in den Channel Rack laden, Editor öffnen. Oben steht `api Vst3` und die Build-Kennung.
2. "Run all three" klicken und etwa 12 s warten, bis drei Zeilen in der Tabelle stehen.
3. Dabei hinsehen: Wächst FLs Plugin-Fenster sichtbar mit? Ist bei 1200 x 900 der untere Rand des Plugins zu sehen, oder schneidet FL ab?
4. Editor schließen und wieder öffnen. Welche Größe hat das Fenster jetzt? (Die Probe speichert die zuletzt angefragte Größe im Projekt.)
5. "Back to 1000 x 640" klicken. Wird das Fenster wieder kleiner?

## Ablauf CLAP

Dasselbe mit "fl resize probe" (CLAP) in einem zweiten Kanal. Bei CLAP gibt FL eine echte Antwort zurück; lehnt FL ab, bleibt auch die egui view klein.

## Was du zurückschickst

| Format | Anfrage | egui view | host view | Rand sichtbar? | nach Schließen und Öffnen |
|---|---|---|---|---|---|
| VST3 | 1200 x 720 | | | | |
| VST3 | 1200 x 820 | | | | |
| VST3 | 1200 x 900 | | | | |
| VST3 | zurück auf 1000 x 640 | | | | n/a |
| CLAP | 1200 x 720 | | | | |
| CLAP | 1200 x 820 | | | | |
| CLAP | 1200 x 900 | | | | |
| CLAP | zurück auf 1000 x 640 | | | | n/a |

Dazu die Werte "pixels_per_point / native" und "visible height" aus dem Fenster und den Inhalt von `~/Library/Logs/fl-resize-probe.log` ab der Zeile `fl resize probe v0.1.0`. Die Zeilen `size() -> W x H` zeigen, welche Größe der Wrapper FL meldet; direkt nach jedem `ask` muss dort die neue Größe stehen.

## Standalone, am 2026-11-04 geprüft

Der Standalone braucht FL nicht. So lief der Test (die App vorher aus `target/bundled` in einen Ordner außerhalb des Repos kopieren und ad hoc signieren):

```
FL_RESIZE_PROBE_AUTO=1 FL_RESIZE_PROBE_EXIT=1 ./fl-resize-probe.app/Contents/MacOS/fl-resize-probe -b dummy
```

`FL_RESIZE_PROBE_AUTO=1` stellt die drei Anfragen beim Öffnen von selbst, `FL_RESIZE_PROBE_EXIT=1` beendet nach dem letzten Urteil, `-b dummy` lässt das Audiogerät weg.

| Build | Bildschirm | 1200 x 720 | 1200 x 820 | 1200 x 900 |
|---|---|---|---|---|
| mit den Vendor-Patches | Retina, ppp 2 | host 51 ms, gehalten | host 29 ms, gehalten | host 29 ms, gehalten |
| mit den Vendor-Patches | extern, ppp 1 | host 40 ms, gehalten | host 29 ms, gehalten | host 35 ms, gehalten |
| nur Patch 1 (upstream-Reihenfolge) | extern, ppp 1 | host bleibt 1000 x 640 | host bleibt 1200 x 720 | host bleibt 1200 x 820 |

Ohne Patch 2 läuft jedes Fenster eine Anfrage hinterher, genau wie die Spec es vorhersagt; die egui view meldet trotzdem jedes Mal "reached". Deshalb zählt die host view. `standalone-1200x900.png` ist die Aufnahme des Fensters (nur dieses Fenster, `screencapture -l`) nach der dritten Anfrage.

## Hinweise

- Bauen nur über `./guarded-cargo.sh` in diesem Ordner. Die Probe nutzt das gepatchte nih-plug aus `plugin/vendor` per Pfad; die Patches stehen in `plugin/vendor/PATCHES.md`.
- Das Plugin gibt Stille aus und hat keine Parameter.
- Tests der Urteilslogik: `./guarded-cargo.sh test -p fl-resize-probe --lib`.
