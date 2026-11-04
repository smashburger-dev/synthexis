# Session 2026-11-04: Kopf und Gesicht, Stufe 1

Ergebnis: Mund, beide Augen einzeln und die drei Kopfwinkel kommen von der Kamera bis zur Route. Die Kopfwinkel kommen jetzt aus einer echten Pose (POS auf sieben starren Vision-Landmarks), relativ zur laufenden Ruhepose. Damit fallen die Kamerafehler weg, die vorher S2 (Pitch neutral 1.00) und S3 (Yaw neutral 0.96) unbrauchbar machten. Jede Achse überstreicht in ihrem Sweep 0..1. Offen bleibt das Übersprechen im Roll-Sweep: Wer den Kopf neigt, bewegt Yaw und Pitch mit. Parser und Completion verstehen die Kopf-Sätze auf Deutsch und Englisch. Deutsche Richtungswörter im Zielteil ("senkt", "sinkt", "nimmt raus") liest der Parser in 5 von 12 Sätzen falsch.

## Regeln vor der Messung

Diese Regeln standen fest, bevor ich die erste Zahl gesehen habe. Gemessen wird mit dem Bench (`plugin/synthexis/src/engine/bench.rs`, Backend `hybrid`) auf Block C von S1 bis S3. Jede Aufnahme startet 60 s vor Block C, damit sich `FaceAdapt` einpendelt wie live. S4 wird am Ende einmal mit der eingefrorenen Konfiguration bewertet. Es zählt der innere Teil jedes Segments (Margins aus `segments.json`). "Median" heißt Median über die Frames des Segments, "Spanne" p95 minus p5.

| Signal | Regel (muss in S1, S2 und S3 gelten) |
|---|---|
| alle | Gesicht in Block C gesehen ≥ 0.95 der Frames |
| alle | in `C_neutral` Median der Frame-zu-Frame-Änderung ≤ 0.01 |
| `mouth.open` | Median neutral ≤ 0.10, Median `C_mouth_wide` ≥ 0.80, Spanne in `C_mouth_sweep` ≥ 0.70 |
| `mouth.open` | geschlossener Mund bleibt zu: p90 ≤ 0.30 in `C_smile`, `C_yaw_sweep`, `C_pitch_sweep`, `C_roll_sweep` |
| `eye.L.open`, `eye.R.open` | Median neutral ≥ 0.60, Median `C_eyes_closed` ≤ 0.20 |
| `eye.L.open` | `C_wink_L`: L-Median ≤ 0.30 und R-Median ≥ 0.50 |
| `eye.R.open` | `C_wink_R`: R-Median ≤ 0.30 und L-Median ≥ 0.50 |
| `eye.*.open` | `C_blink`: p5 ≤ 0.20 auf beiden Augen (der Lidschlag kommt durch) |
| `eye.*.closed` | ist 1 - open (Code-Check, keine eigene Messung) |
| `head.yaw` | Spanne in `C_yaw_sweep` ≥ 0.60 mit p5 < 0.35 und p95 > 0.65; Spanne neutral ≤ 0.10; Median neutral in 0.35..0.65 |
| `head.pitch` | Spanne in `C_pitch_sweep` ≥ 0.50 mit p5 < 0.40 und p95 > 0.60; Spanne neutral ≤ 0.10; Median neutral in 0.35..0.65 |
| `head.roll` | Spanne in `C_roll_sweep` ≥ 0.60 mit p5 < 0.35 und p95 > 0.65; Spanne neutral ≤ 0.10; Median neutral in 0.35..0.65 |
| Kopfwinkel | Übersprechen: in jedem fremden Sweep Spanne ≤ 0.40 (yaw in `C_pitch_sweep` und `C_roll_sweep`, pitch in `C_yaw_sweep` und `C_roll_sweep`, roll in `C_yaw_sweep` und `C_pitch_sweep`) |

Parser und Completion, ebenfalls vor der Messung festgelegt:

| Prüfung | Regel |
|---|---|
| Kopf-Sätze (`probes/head-signals/sentences.tsv`, je 15 DE und EN, später je 17) | mit dem Gesten-Chip, den die Completion anbietet (bei MIDI-Zielen auch dem Pattern-Chip), landet jeder Satz auf Quelle, Seite und Zielart |
| dieselben Sätze, ohne Chip getippt | mindestens 12 von 15 je Sprache |
| Richtung (`probes/head-signals/direction.tsv`, je 12 DE und EN, neue Sätze) | INV stimmt in mindestens 11 von 12 je Sprache |
| Completion (`plugin/complete/tests/head_vocab.rs`) | jeder getippte Anfang einer Kopf-Geste bietet als erste Geste die richtige Quelle an, je 12 DE und EN |

Die Richtung (links gegen rechts, oben gegen unten) geben die Labels nicht her. Ich prüfe sie an der Geometrie in den Unit-Tests von `geom.rs` und vergleiche sie mit dem Ontology-Text (yaw 0 = zur Linken der Person gedreht, pitch 0 = Blick nach unten, roll 0 = zur linken Schulter geneigt).

## Was sich geändert hat

Kopfpose (`engine/geom.rs` `head_points`, `HeadPose`; `engine/vision.rs` `FaceLandmarks::run`):

- Vorher kamen Yaw, Pitch und Roll aus der Lage der Augen in der Gesichtsbox des Seg-Students. Diese Box schwankt mit Haaren und Hals, und die Augenlinie kippt im Bild, sobald die Kamera von oben auf einen gedrehten Kopf schaut.
- Jetzt: sieben starre Punkte aus den Vision-Landmarks (Augenmitten, Nasenspitze, Nasenwurzel, beide Nasenflügel, Subnasale), ausgewählt im Koordinatensystem der Augenlinie. Kein Mund- und kein Kinnpunkt, damit Sprechen und Lächeln die Pose nicht bewegen. Ein generisches 3D-Gesicht in Augenabständen, Pose per POS (Least Squares der skalierten Rotationszeilen), Winkel relativ zur Ruhepose: dem elementweisen Median der Posen der letzten 30 s, wieder orthonormiert. Damit fallen Kamerahöhe, Kameraseite und Kamerarolle heraus.
- Gate: Liegt der RMS-Rest des Fits über 0.05 Augenabständen, hält der Kopf seinen letzten Wert (`seen_head` false). Neutrale Frames passen mit 0.008. Frames mit Ausreißern (Yaw springt im Pitch-Sweep um 40 bis 60°) liegen bei 0.1 bis 0.5.
- Vollausschlag: Yaw ±45°, Pitch ±30°, Roll ±35°.
- Geprüft und verworfen: den Roll des letzten Frames an den vorgegebenen Gesichtsrahmen für Vision geben. Das senkte den p90-Rest des Fits in den Roll-Sweeps von S1 und S3 von 0.037 / 0.044 auf 0.030 / 0.033. Ich hatte ihn beim Suchen nach dem S4-Absturz entfernt, der Absturz kam aber vom fehlenden Autorelease-Pool (unten). Mit Hinweis bestand S1 38 statt 37 Regeln, S2 34 statt 33. Er kann zurück und braucht dann einen neuen Messlauf auf S1 bis S4.
- Zwei Fehler auf dem Weg: Der unterste Punkt von `noseCrest` ist ein seitlicher Nasenflügel, nicht die Spitze (Yaw neutral +36°). Und der äußerste Punkt der Nasenkontur springt zwischen Flügel und Nasenrücken (Pitch springt um 10°). Beides löst die Auswahl auf der Mittellinie und im unteren Viertel.

Vision-Backend (Fallback ohne Students): Seine eigenen Yaw- und Pitch-Werte liefen gegen die Ontology. Die Yaw korreliert in S1 bis S3 mit +0.91 mit dem Versatz der Nasenspitze nach Bildrechts (= zur Linken der Person gedreht, also Yaw 0). Die Pitch korreliert mit +0.83 mit dem Abstand der Spitze unter den Augen (= Blick nach unten, also Pitch 0). Beide Vorzeichen sind jetzt gedreht. Vision liefert Yaw nur in 45°-Stufen (3 Werte in S1) und Roll in 30°-Stufen (5 Werte), Pitch kontinuierlich.

Emitter (`engine/perception.rs`): Die Kopfwinkel gelten als gesehen, wenn die Pose den Frame nimmt (`seen_head`), statt wenn beide Augen da sind.

Parser (`plugin/parser/src/pin.rs`): Ein Ziel-Chip, für den keine Route ein freies Ziel hat, geht jetzt an die nächste Route mit Quelle und ohne Ziel, statt eine eigene Route ohne Quelle zu bekommen. Liest der Parser die Chip-Wörter als Kanal (`TRACK =kick`), fällt der Kanal weg und das Pattern behält den Namen (`T:midi.pattern:kick`). Das betraf "blink left to play the kick".

Completion (`plugin/complete/src/context.rs`): Eine Anfrage darf 5 statt 3 Wörter umfassen. "Kopf nach rechts dre" (4 Wörter) und "turn my head to the ri" (5) fanden vorher keine Geste; 790 der 2.728 Gesten-Phrasen haben 4 oder 5 Wörter. "links" und "rechts" zählen als Seitenwort wie "linke", damit "wenn ich links zwink" eine Geste anbietet. Keine Gesten-Phrase beginnt mit diesen Wörtern.

## Messung: Gesicht und Kopf

Basis = das Release-Binary von 14:48 vor meinen Änderungen, neu = mit Pose (Debug-Build). Werte S1 / S2 / S3, S4 einmal am Ende. Regeln bestanden (von 43): Basis 34 / 31 / 33 / 30, neu 37 / 33 / 36 / 37 (S1 / S2 / S3 / S4).

| Kennzahl (Regel) | Basis S1 / S2 / S3 / S4 | neu S1 / S2 / S3 / S4 |
|---|---|---|
| yaw neutral Median (0.35..0.65) | 0.43 / 0.48 / **0.96** / 0.42 | 0.51 / 0.48 / 0.43 / 0.49 |
| pitch neutral Median (0.35..0.65) | 0.51 / **1.00** / 0.47 / **0.80** | 0.45 / 0.56 / 0.62 / 0.58 |
| pitch neutral Spanne (≤ 0.10) | 0.06 / **0.55** / 0.04 / **0.23** | 0.08 / **0.11** / 0.05 / 0.05 |
| Sweep-Spanne yaw (≥ 0.60) | 1.00 / 1.00 / 1.00 / 1.00 | 1.00 / 1.00 / 0.80 / 1.00 |
| Sweep-Spanne pitch (≥ 0.50) | 1.00 / 1.00 / 1.00 / 0.69 | 1.00 / 1.00 / 1.00 / 1.00 |
| Sweep-Spanne roll (≥ 0.60) | 1.00 / 1.00 / 1.00 / 1.00 | 1.00 / 1.00 / 0.85 / 0.98 |
| roll im Yaw-Sweep (≤ 0.40) | **1.00 / 0.77 / 0.71 / 1.00** | 0.17 / **0.51** / 0.23 / 0.25 |
| pitch im Yaw-Sweep (≤ 0.40) | **0.40 / 0.59 / 0.59 / 0.56** | 0.27 / 0.12 / 0.19 / 0.17 |
| yaw im Pitch-Sweep (≤ 0.40) | **0.43** / 0.25 / **0.48** / 0.39 | 0.39 / 0.34 / 0.30 / 0.27 |
| roll im Pitch-Sweep (≤ 0.40) | **0.49 / 0.41 / 0.74** / 0.39 | **0.40 / 0.56 / 0.42** / 0.31 |
| yaw im Roll-Sweep (≤ 0.40) | **0.80 / 0.63 / 0.49 / 1.00** | **0.73 / 0.91 / 0.74 / 0.98** |
| pitch im Roll-Sweep (≤ 0.40) | **0.66 / 0.45** / 0.31 / **0.85** | **0.60 / 0.60 / 0.57 / 0.61** |

Fett = Regel verfehlt.

Was nicht besteht, mit Zahlen:

- Roll-Sweep: Yaw steigt mit dem Roll, Steigung 0.43 / 0.90 / 0.87 (Grad Yaw pro Grad Roll). Umgekehrt bringt der Yaw-Sweep kaum Roll mit (-0.16..+0.06). Die Kopplung ist also einseitig, so wie bei der gekoppelten Bewegung der Halswirbelsäule (Seitneigung dreht den Kopf mit). Vision misst im Roll-Sweep einen Pitch-Ausschlag von 0.72 / 0.88 seines Bereichs (S1 / S2), seine Pitch ist kontinuierlich. Ein Teil des Übersprechens steckt also in der Bewegung selbst. Ohne Bewegungssensor als Referenz lässt sich der Rest nicht vom Schätzfehler trennen. Eine Entkopplung (Yaw minus k mal Roll, k 0.5 bis 0.85) habe ich offline geprüft: Sie drückt Yaw im Roll-Sweep in S3 auf 0.22, hebt aber Yaw im Pitch-Sweep auf 0.47 bis 0.62 und hilft S1 nicht. Deshalb ist sie nicht drin.
- roll im Pitch-Sweep S1 0.40, S2 0.56, S3 0.42 und roll im Yaw-Sweep S2 0.51: Auch das kann echte Bewegung sein. Vision gibt Roll nur in 30°-Stufen und taugt hier nicht als Referenz.
- pitch neutral Spanne S2 0.106, knapp über 0.10 (Basis 0.55). S2 ist die Sitzung mit Brille.
- Der Yaw-Sweep in S3 erreicht 0.80 statt 1.0, der Roll-Sweep 0.85: S3 ist von der Seite gefilmt, die Ruhepose steht schräg, und die Drehung zur Kamera hin endet früher.

Mund (`mouth.open`, unverändert): neutral 0.00, weit offen 1.00 / 1.00 / 0.86, Sweep-Spanne 1.00 / 1.00 / 0.80, bei gedrehtem Kopf mit Mundbewegung p10 0.00 und p90 1.00. Nicht bestanden: Lächeln öffnet das Signal auf p90 0.32 / 0.48 / 0.59, weil die Lippen beim Lächeln auseinandergehen und das Signal die Lücke zwischen den Innenlippen misst. In S3 öffnet der Yaw-Sweep es auf p90 0.44, in S4 der Pitch-Sweep auf 0.35. Wer Mund und Lächeln getrennt steuern will, braucht ein Gate (Mund nur bei kleinem Lächeln). Das ist nicht gebaut.

Augen (`eye.L.open`, `eye.R.open`, unverändert, `FaceAdapt`): Augen zu 0.00 auf beiden Augen außer S2 rechts (1.00, Brille), Lidschlag p5 ≤ 0.15 überall. Zwinkern: S1 rechts 0.48 statt ≤ 0.30, S2 links 0.64. S3 trennt beide Seiten sauber (0.00 gegen 1.00). Zittern im Ruhezustand 0.03 bis 0.06 pro Frame auf einem Auge je Sitzung, aus dem Pixel-Term von `FaceAdapt`. Der One-Euro-Filter jeder Route glättet das. `eye.*.closed` ist `1 - open` (`FaceFeatures::values`).

S4 (einmal, eingefrorene Konfiguration): 37 von 43 Regeln (Basis 30). Bei den Kopfwinkeln verfehlt nur das Übersprechen im Roll-Sweep (yaw 0.98, pitch 0.61). Mund und Augen sind unverändert: Lächeln öffnet den Mund auf p90 0.46, Zwinkern links lässt das rechte Auge auf 0.45 sinken, rechts das linke auf 0.40. Der Debug-Build brach in S4 bei 219.3 s ab (nächster Abschnitt). Die Werte ab dem Yaw-Sweep stammen deshalb aus einem zweiten Lauf ab 167 s, mit 60 s Vorlauf vor dem Pitch-Sweep. Ein voller Lauf nach dem Fix gibt dieselben Zahlen.

## Absturz im Debug-Build: fehlende Autorelease-Pools

Ein Debug-Build brach auf S4 bei 219.3 s ab ("Rust cannot catch foreign exceptions"), der Originalcode auch, das Release-Binary nicht. lldb zeigt den Wurf: eine C++-Exception in CoreML/Espresso (`E5RT::IOBufferObject` beim Anlegen eines Ausgabepuffers, aus `coreml.rs` `predict`). Vision war es nicht, deshalb hilft `objc2::exception::catch` nicht: Es fängt nur Objective-C-Exceptions, der Abbruch blieb.

Die Ursache: Der Wahrnehmungs-Thread, der Landmark-Thread und der Bench liefen ohne Autorelease-Pool. CoreML und Vision legen ihre Ergebnisse in den Pool des Threads, und ohne eigenen Pool bleiben sie bis zum Thread-Ende liegen. Spitzen-Footprint auf S4:

| Build | nach 1.140 Frames | nach 3.239 Frames | ganzes S4 (5.218) |
|---|---|---|---|
| Release ohne Pool (14:48) | 49 MB | 52 MB | läuft durch |
| Debug ohne Pool | 1,46 GB | 4,08 GB | Abbruch bei 3.818 |
| Debug mit Pool pro Frame | 51 MB | 52 MB | 49 MB, läuft durch |

Im Release gibt die Laufzeit die meisten Objekte ohne Pool zurück, live in FL ist es also bisher gut gegangen. Jetzt hat jeder Frame seinen Pool (`perception.rs` `run` und `LandmarkWorker`, `bench.rs`). Die Pong-Session hat auf meine Bitte das `exception`-Feature von objc2 in `Cargo.toml` eingeschaltet. Mein Code braucht es nicht mehr. Ob es bleibt, entscheidet die Redesign-Unit, der `Cargo.toml` gehört. Die S4-Zahlen oben stammen aus zwei Läufen vor dem Fix. Ein voller Lauf mit Pool bestätigt sie (37 von 43 Regeln, Yaw im Roll-Sweep 0.98, Pitch im Roll-Sweep 0.62).

## Messung: Parser, Completion, Resolver

- Kopf-Sätze mit Chip (`plugin/parser/tests/head_live.rs`): 30 von 30 landen auf Quelle, Seite und Zielart (Plugin-Parameter, Signal-Makro oder MIDI-Part). Vor der Pin-Änderung waren es 28: "blink left to play the kick" und "wink right to play the hi-hat" bekamen den Pattern-Chip als eigene Route ohne Quelle.
- Ohne Chip getippt: DE 15 von 15, EN 13 von 15. Die zwei EN-Fehler sind dieselben zwei Sätze: Der Parser liest "the kick" als Kanalnamen. Mit "a kick" oder "the kick pattern" stimmt es.
- Nachtrag nach Noas Antwort: vier Tilt- und Turn-Sätze dazu ("tilt my head to the left", "Kopf zur Schulter neige", "turn my head up", "Kopf nach oben drehe"). Jetzt 34 von 34 mit Chip, getippt DE 17 von 17, EN 15 von 17.
- Richtung (`probes/head-signals/direction.tsv`): EN 11 von 12, DE 7 von 12, Regel verfehlt. Falsch auf Deutsch: "sinkt der Cutoff", "nimmt den Drive raus", "Kopf nach unten dreht auf", "Kopf hoch senkt", "Mund schließe, wird es lauter". In einer Vorprobe kamen dazu "Mund auf senkt die Lautstärke" und "linkes Auge zu macht den Filter zu". Das betrifft jede Quelle, nicht nur den Kopf. Der Parser bräuchte deutsche Trainingssätze mit Senken-Verben im Zielteil oder eine lexikalische Richtungsregel, die gegen das Dev-Set geprüft wird. Beides habe ich nicht angefangen.
- Completion (`plugin/complete/tests/head_vocab.rs`): 18 DE und 18 EN getippte Anfänge bieten als erste Geste die richtige Quelle, das Auge übernimmt die Seite davor ("das linke Auge", "links zwink", "my left eye").
- Tilt und Turn, wie Noa sie meint: turn ist hoch, runter, links, rechts (Yaw, Pitch), tilt und neigen ist der Kopf zur Schulter (Roll). Im Glossar fehlte "tilt my head to the left", deshalb bot "when I tilt my head to the le" Yaw an ("turn head to the left"). `tools/mapping/export_completion.py` ergänzt jetzt 24 Kopf-Phrasen (`HEAD`, DE und EN, mit Richtung) und streicht "Kopf nach rechts neigen drehen" aus Yaw. Der neue Export unterscheidet sich vom alten nur in diesen 25 Zeilen. Er liegt in `plugin/assistant/completion/vocab.json` und `data/mapping/completion/vocab.json`. Außerdem darf eine Anfrage jetzt 6 Wörter umfassen, weil "tilt my head to the le" ab "tilt" sechs Wörter hat. 963 der 2.751 Gesten-Phrasen haben 4 bis 6 Wörter, der Latenz-Test besteht. "tilt my head up/down" bleibt Pitch, die Richtung sagt dort, was gemeint ist.
- Resolver: Die Parameter-Ziele binden in `head_live.rs` an die Parameter eines Test-Synths, `T:macro:N` geht an Signal N, MIDI-Ziele werden Parts. Ohne Seite macht `assign.rs` aus einem Auge keine Route und fragt "welches Auge?". So bleibt es: Blinzeln ohne Seite heißt nicht beide Augen, weil man oft nur mit einem blinzelt (Noa, 11-04).

## Wie man nachmisst

```
cd plugin && CARGO_TARGET_DIR=<ziel> ./guarded-cargo.sh build -p synthexis --bin synthexis
SYNTHEXIS_PERCEPTION=hybrid SYNTHEXIS_BENCH=../data/recordings/2026-10-27-S1/cam0.mov \
  SYNTHEXIS_BENCH_START=469 SYNTHEXIS_BENCH_SECONDS=642 SYNTHEXIS_BENCH_OUT=head_S1.jsonl <ziel>/debug/synthexis
python3 ../probes/head-signals/score.py head_S1.jsonl ../data/recordings/2026-10-27-S1/segments.json
```

Startzeiten mit 60 s Vorlauf: S1 469..642, S2 275..448, S3 275..448, S4 92..266. `poselab.py` spielt `HeadPose` offline mit anderen Gates und Bereichen nach (`simulate`). `headlab.py` ist die Prüfung der Vorzeichen des Vision-Backends.

```
SYNTHEXIS_PARSER_DIR=../data/mapping/runtime ./guarded-cargo.sh test -p synthexis-parser --test head_live -- --nocapture
./guarded-cargo.sh test -p synthexis-complete --test head_vocab
```

## Offen

- Übersprechen im Roll-Sweep (siehe oben). Ein Kopfhörer-Sensor (`src.headphones`, F13) wäre die Referenz.
- Deutsche Richtungswörter im Parser.
- Ruhepose als Kalibrierung statt laufendem Median: Wer den Kopf länger als etwa 15 s gedreht hält, verschiebt die Mitte.

# Stufe 2: Kopf-Sektion und eingebautes Preset "Head"

## Was es gibt

- Drittes eingebautes Preset "Head" (deutsch "Kopf") nach Hands und Pong, im Preset-Picker und im "+"-Menü (`preset.rs` `Section::Head`). Wie Hands bringt es keinen eigenen Inhalt mit. Zur Sektion gehören die behaltenen Zeilen mit einer Quelle aus Mund, Gesicht, Auge oder Kopf (`mouth.`, `face.`, `eye.`, `head.`) und die MIDI-Binds auf diesen Signalen.
- Verhaltensänderung: Gesichts-Routen waren vorher "keiner Sektion zugehörig" und überlebten jeden Preset-Wechsel. Jetzt öffnen sie die Kopf-Sektion (Inhalt zeigt sich immer, R5.7). Ein eingebautes Preset, das den Kopf schließt, fragt vorher: "Head goes. Click again". Schließen per `x` fragt "Close Head? N routes go". Zurück stellt alles wieder her.
- Die Sektion (`head_ui.rs`): ein Kopf im Punktraster der Hände (160 × 160 Punkte zu 1,5 pt), nur Linien mit wechselnder Stärke, Licht von oben links. Kontur mit Schläfe, Wangenknochen, Kieferwinkel und Kinn, Ohren, Mandelaugen mit Ober- und Unterlid, Lidfalte und Iris, Brauen, Nasenrücken, Nasenspitze und Nasenflügel, ein schmaler Mund mit Amorbogen, Hals mit den zwei Halsmuskeln, Drosselgrube, Schultern und Schlüsselbeinen. Spiegelansicht wie bei den Händen: das linke Auge der Person ist links.
- Die Figur folgt der Kamera: Drehen verschiebt die Gesichtszüge über eine Kugel (das ferne Auge wird schmal), Nicken hebt und senkt sie, Neigen kippt den Kopf um den Halsansatz, der Hals folgt zu 30 %, die Schultern bleiben. Der Mund öffnet sich, Lächeln hebt die Mundwinkel, ein geschlossenes Auge wird zur Linie, seine Braue senkt sich, ein offener Mund hebt beide Brauen. Ohne Gesicht im Bild steht die Figur gedimmt geradeaus und die Zeile darunter sagt "No face in view".
- Klickbar sind Kopf, linkes Auge (mit Braue), rechtes Auge (mit Braue) und Mund, auf der Figur, über die vier Chips darunter oder über ihre Knoten. Ein Klick hebt den Teil im selben Frame hervor: Linien in Kopffarbe (Violett, neben Orange und Cyan der Hände) und breiter. Teile mit Routen zeigen einen Knoten mit dem Ziel ("Filter 1 Freq", "Snare" oder "2 routes"), der gewählte ohne Route "Empty".
- Unter der Figur stehen der Live-Zustand ("Turned left · Mouth open · Right eye closed") und für den gewählten Teil seine Werte ("Turn 0.62 · Nod 0.48 · Tilt 0.51"), was er steuert, und Beispielsätze, die Parser und Completion verstehen.
- Die Live-Werte kommen aus `engine::FACE`, das der Emitter bei jedem Gesicht schreibt.

## Testschritte für Noa (FL)

1. synthexis öffnen. Im Preset-Picker "Head" wählen. Erwartet: nur die Kopf-Sektion ist offen, Toast "Opened Head". Der Picker zeigt "Head".
2. Vor die Kamera setzen und geradeaus schauen. Erwartet: die Figur wird hell, die Zeile darunter sagt "Facing ahead".
3. Den Kopf nach links drehen, dann nach rechts. Erwartet: die Gesichtszüge wandern zur selben Bildschirmseite wie im Spiegel, das ferne Auge wird schmal. Die Zeile sagt "Turned left" bzw. "Turned right".
4. Nach oben und nach unten schauen. Erwartet: die Gesichtszüge wandern hoch bzw. runter, "Looking up" / "Looking down".
5. Den Kopf zur linken, dann zur rechten Schulter neigen. Erwartet: der Kopf kippt zur selben Seite wie im Spiegel, die Schultern bleiben waagerecht, "Tilted left" / "Tilted right".
6. Den Mund öffnen und schließen, dann lächeln. Erwartet: der Mund geht auf, die Brauen heben sich etwas, beim Lächeln gehen die Mundwinkel hoch.
7. Links zwinkern, dann rechts. Erwartet: das Auge auf derselben Bildschirmseite wird zur Linie, seine Braue senkt sich, "Left eye closed" / "Right eye closed".
8. Auf den Mund klicken. Erwartet: er leuchtet violett, darunter steht "Mouth", seine Werte, "Drives nothing yet" und Beispielsätze. Dasselbe mit den Augen und dem Kopf, auch über die Chips unter der Figur.
9. Im Chat-Feld tippen: "wenn ich den Mund öffne, öffne den Filter" (oder einen der Beispielsätze) und behalten. Erwartet: am Mund erscheint ein Knoten mit dem Filter-Parameter, unter "Drives:" steht die Zeile.
10. "tilt my head to the left to lower the cutoff" tippen. Erwartet: die Completion bietet "tilt my head to the left" an, die Route landet am Kopf.
11. Im Preset-Picker "Hands" wählen. Erwartet: die Zeile fragt "Head goes. Click again". Erst der zweite Klick schließt den Kopf und entfernt seine Routen. "Undo" holt sie zurück.
12. Über "+" "Head" zu Hands dazunehmen. Erwartet: beide Sektionen sind offen, Hands zuerst.
13. Das `x` der Kopf-Sektion zweimal klicken. Erwartet: beim ersten Klick "Close Head? N routes go", beim zweiten ist die Sektion zu.
14. Auf Deutsch umstellen (Einstellungen) und 1 bis 8 kurz wiederholen. Erwartet: "Kopf", "Linkes Auge", "Nach links gedreht" und so weiter.

## Geprüft

- Tests: synthexis 397 (davon 7 in `head_ui.rs`, 1 neuer Picker-Test, 1 neuer Preset-Test), Parser 42, Completion 43 + 3, Clippy ohne Fehler.
- Screenshots aus dem Standalone (`SYNTHEXIS_UI_SHOT=f` Kopf leer, `g` Kopf mit Snare am Mund, gedreht, geneigt, rechtes Auge zu; `d` mit dem "+"-Menü), EN und DE: Figur, Knoten, Chips, Live-Zeile, Werte und der Eintrag "Head" im "+"-Menü stimmen. Mit echter Kamera ist die Sektion noch nicht gesehen (der Standalone wartete auf die Kamerafreigabe).
- Nicht installiert, nicht in FL geprüft.
