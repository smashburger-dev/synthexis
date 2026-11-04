# Session 2026-11-04: Finger-Bugs aus dem FL-Test

Drei Bugs aus Noas FL-Test vom 11-04. Bug 3 ist behoben. Bug 2 ist zur
Hälfte behoben: die Kalibrierung verdirbt keine Schwellen mehr und meldet,
was sie nicht lösen kann. Bug 1 ist mit Zahlen erklärt, lässt sich aber im
Engine-Code nicht beheben. Dafür braucht das Handmodell neue
Trainingsdaten.

Geänderte Dateien: `plugin/synthexis/src/engine/geom.rs` (SideTracker),
`plugin/synthexis/src/calib.rs` (neu geschrieben),
`plugin/synthexis/src/engine/perception.rs` und `engine/mod.rs`
(Vision-Probe entfernt). Messungen und Skripte in `probes/finger-shapes/`
(`sides.md`, `sides_sim.py`, `sides_switch.py`, `calib_replay.py`,
Nachtrag in `RESULT.md`). Keine UI-Datei angefasst; was die UI übernehmen
sollte, steht in `finger-ui-needs.md` im Notizordner der MIDI-Session.
Nichts committet, nichts in FL installiert.

## Bug 1: Mittelfinger allein hebt Zeigefinger (und kleinen Finger)

Ursache: Das Handmodell (Student) liest den Mittelfinger allein als
Zeigefinger. Dein Kalibrier-Log vom 11-04 zeigt es auf beiden Händen:

| Pose | Hand | Zeigefinger Student / Geometrie | Mittelfinger Student / Geometrie |
|---|---|---|---|
| Mittelfinger allein | L | 0,98 / 0,93 | 0,05 / 0,10 |
| Mittelfinger allein | R | 0,96 / 1,00 | 0,01 / 0,03 |
| Ringfinger allein | L | 0,96 / 0,88 | 0,00 / 0,14 |
| Ringfinger allein | R | 0,70 / 0,99 | 0,00 / 0,10 |

Die Geometrie hilft nicht, weil sie aus den Keypoints desselben Modells
rechnet: das Modell legt die Zeigefinger-Kette auf den gehobenen Finger.
Apple Vision auf denselben Frames sah den Zeigefinger ebenfalls bei 0,81
und 0,94.

Warum das Modell das tut: die Aufnahmen S1 bis S4 enthalten 910 s
gelabelte Handposen in 12 Masken. Davon 0 s mit Mittelfinger oben und
Zeigefinger unten, 0 s mit Ringfinger oben ohne Zeige- und Mittelfinger.
Die nächste Einfinger-Pose, die das Modell kennt, ist der Zeigefinger
(96 s). Also landet jede Einfinger-Pose dort.

Den kleinen Finger hat die alte Kalibrierung dazugeholt: sie setzte die
Schwelle des linken kleinen Fingers auf 0,22. Mit den alten Schwellen las
die linke Ringfinger-Pose als Zeigefinger plus kleiner Finger (01001),
nachgerechnet mit `calib_replay.py noa`.

Eine Schwelle kann "Zeigefinger 0,97, Mittelfinger 0,05" nicht in
"Mittelfinger" umdrehen. Neues Training (die 20 fehlenden Masken
aufnehmen) fällt vor der Abgabe weg: 4 bis 6 Stunden mit Risiko für die
Posen, die heute gehen.

Recherche (zwei Agenten, Quellen in den Notizen der Session): eine
Quelle beschreibt genau diesen Fehler nicht. Am nächsten kommen das
fingerpose-README (MediaPipe-basiert, Probleme mit einem einzelnen
gestreckten Finger) und DIGIT (arXiv 2107.00434, Ähnlichkeit der Finger
als Hauptfehler). Der empfohlene Weg ohne Training ist ein
Nearest-Centroid pro Person auf den Keypoints, mit dem Label der
gezeigten Kalibrier-Pose statt dem, was das Modell daraus liest
(SimpleShot, arXiv 1911.04623; Jackknife, CHI 2017). Er greift nur aus
der Verwechslungsgruppe heraus, mit Marge, und schaltet sich ab, wenn
die Kalibrierung selbst keine Trennung zeigt. Reine Schwellen und
Gelenkwinkel helfen nicht, weil die Keypoints die gestreckte Kette selbst
als Zeigefinger beschriften. Offen bleibt auch, ob dein Zeigefinger in
der Mittelfinger-Pose halb mitkam: Apple Vision las ihn bei 0,81 und
0,94.

Umgesetzt: Skelett-Vorlagen pro Person. Auch wenn das Modell
den gehobenen Finger "Zeigefinger" nennt, kann sein Skelett für
Mittelfinger allein anders liegen als für Zeigefinger allein (wo die
gehobene Kette ansetzt, wie die gefalteten Finger liegen). Die
Kalibrierung speichert pro Pose das normierte Skelett (Handgelenk im
Ursprung, gedreht und auf Handflächenlänge skaliert). Für jede
fehlgelesene Pose prüft sie auf zurückgehaltenen Frames (erste Hälfte
lernt, zweite Hälfte wird getestet), ob die Skelette beider Posen
auseinanderliegen. Erst ab 90 % richtig zugeordneter Frames wird die
Vorlage aktiv (`calib::MIN_APART`). Zur Laufzeit ersetzt die Engine die
gelesene Maske 01000 durch die Pose der nächsten Vorlage, aber nur, wenn
diese deutlich näher ist (Abstand höchstens 0,8 des nächsten,
`TEMPLATE_MARGIN`). Dazwischen bleibt die letzte Entscheidung stehen.
Trennen sich die Skelette nicht, bleibt alles wie vorher, der
Zeigefinger und damit der Pong-Schuss bleiben unberührt.

Ob es bei dir greift, zeigt erst eine Kalibrierung: das Log schreibt pro
Fehllesung `skeletons apart <Wert>, template on|off`. Offline lässt es
sich nicht messen, weil keine Aufnahme Mittelfinger allein enthält. Die
Vorlagen leben nur im Speicher; nach einem Neustart von FL oder nach
"Reset" braucht es eine neue Kalibrierung.

Folge für Pong: Mittelfinger allein ergibt die Maske 01000, also feuert
auch der Pong-Schuss.

## Bug 2: Kalibrierung ändert nichts

Drei Ursachen, gemessen im Log vom 11-04:

1. Die Kalibrierung startete Apple Vision als Vergleichsprobe im
   Perception-Thread. Das Laden blockierte ihn etwa 8 s (57 Frames in
   9,7 s statt rund 290). Faust und offene Hand bekamen deshalb 0 und 1
   Frames, der Daumen 11 bis 13 statt etwa 30.
2. Bug 1: Wenn eine Pose als falscher Finger gelesen wird, hilft keine
   Schwelle pro Finger.
3. Die alte Regel nahm den Median über alle Frames, in denen ein Finger
   gefaltet sein sollte. Mittel- und Ringfinger-Pose lieferten dort
   Zeigefinger-Werte von 0,85 bis 0,98 und zogen den Median hoch.
   Ergebnis: rechter Zeigefinger 0,85 (er brauchte danach 0,95 zum
   Aufleuchten), linker Zeigefinger 0,5, linker kleiner Finger 0,22.

Fix:
- Vision-Probe entfernt (`CALIBRATING`, `FINGER_VISION`). Ihre Frage ist
  beantwortet: Vision liest die Mittelfinger-Pose auch als Zeigefinger.
- Eine Pose wartet nach ihren 2,9 s, bis eine Hand 20 Frames geliefert
  hat, höchstens 8 s. Ein Stocken kostet keine Pose mehr.
- Neue Regel pro Finger: Median pro Pose statt gepoolter Frames. Schwelle
  in der Mitte zwischen der eigenen Pose (und der offenen Hand) und der
  höchsten Pose mit gefaltetem Finger. Eine gefaltete Pose, die den
  Finger klar als gezeigt liest (über 0,5 und näher am gezeigten Wert),
  zählt als Fehllesung und bleibt draußen.
- `Outcome::misread` listet jede Pose, die mit den neuen Schwellen als
  andere Maske gelesen wird. Das Log schreibt dazu eine Zeile pro
  Fehllesung mit dem Skelett-Abstand.

Auf deinem Lauf vom 11-04 ergibt die neue Regel diese Schwellen
(Daumen bis kleiner Finger):

| Hand | alt | neu |
|---|---|---|
| L | 0,37 0,50 0,50 0,50 0,22 | 0,40 0,66 0,50 0,50 0,50 |
| R | 0,50 0,85 0,50 0,50 0,49 | 0,53 0,53 0,50 0,50 0,57 |

Fehllesungen auf beiden Händen: Mittelfinger allein und Ringfinger allein
lesen als Zeigefinger. Dazu links der kleine Finger allein: sein Wert
(0,44) liegt nur 0,19 über dem der Ringfinger-Pose (0,25). Das passt
nicht um das Totband des Gates (±0,1) herum, also bekommt er keine
eigene Schwelle und wird gemeldet. Die alte Schwelle 0,22 ließ ihn in
der Ringfinger-Pose an.

Zwei Fehler in meiner ersten Fassung der neuen Regel hat die
Gegenprüfung gefunden, beide behoben:
- Eine Schwelle verlangte nur 0,12 Abstand zwischen gezeigt und
  gefaltet. Das Gate schaltet aber erst bei Schwelle ±0,1. Unter 0,2
  Abstand leuchtet ein Finger aus seiner eigenen Pose nie oder geht nicht
  mehr aus. Jetzt gilt: mindestens 0,2, sonst 0,5 und Meldung.
- Die Fehllese-Prüfung las die Posen ohne dieses Band. Jetzt liest sie
  wie das Gate: an erst über Schwelle + 0,1, aus erst unter Schwelle − 0,1.

Replay auf den Aufnahmen (Regel vorher festgelegt: neu mindestens so gut
wie alt und höchstens 0,005 unter fester Schwelle 0,5). Block A enthält
fünf der sieben Kalibrier-Posen. Kalibriert auf je 2 s pro Pose,
bewertet auf allen Block-A-Frames als exakte Maske:

| | S1 bis S3 (15.433 Frames) | S4, einmal (3.218 Frames) |
|---|---|---|
| fest 0,5 | 0,920 | 0,895 |
| alte Regel | 0,922 | 0,894 |
| neue Regel | 0,920 | 0,894 |

Das Kriterium "neu mindestens so gut wie alt" verfehlt die neue Regel um
0,002. Auf den Aufnahmen gibt es keine fehlgelesene Pose, also liegen alle
Regeln bei Schwellen um 0,5 und innerhalb von 0,2 Punkten. Der Unterschied
zeigt sich nur auf deinem Lauf.

## Bug 3: Rechte Hand als linke, bis die andere Hand kommt

Ursache: `SideTracker` gibt jeder Hand eine Spur und summiert ihre
Seiten-Stimmen bis ±40. Nach ein bis zwei Sekunden ist die Spur satt.
Geht die linke Hand raus und kommt die rechte innerhalb von 0,5 s an
ungefähr derselben Stelle rein (weniger als 320 px entfernt), übernimmt
die rechte Hand die Spur der linken. Sie braucht dann etwa 30 eigene
Frames (1 s), bei schwächeren Stimmen länger. Mit zwei Händen im Bild
teilt die Engine nach Bildseite, deshalb "repariert" die andere Hand es.

Die Stimmen selbst stimmen in 98,2 bis 99,7 % der Frames. Die Aufnahmen
verdecken den Bug, weil Block A jede Hand auf ihrer Seite hält. Die
x-Bereiche beider Hände überlappen aber stark.

Messung (Regel vorher festgelegt, Details in `probes/finger-shapes/sides.md`):
1.728 nachgestellte Handwechsel an derselben Stelle aus S1 bis S3.

| | falsche Frames nach Wechsel, Median / p90 / max | Einhand-Frames falsch |
|---|---|---|
| vorher (Cap 40) | 30 / 37 / 50 | 0,20 % |
| neu (Cap 3, Lücke 150 ms) | 1 / 2 / 10 | 0,25 % |
| S4 einmal, vorher | 28 / 36 / 50 | 0,37 % |
| S4 einmal, neu | 1 / 2 / 6 | 0,28 % |

Mein Kriterium "max höchstens 6 Frames" erfüllt keine Variante auf S1 bis
S3. Selbst ganz ohne Gedächtnis liegt das Maximum bei 13, weil einzelne
Segmente mit etwa zehn falschen Stimmen beginnen. Das Kriterium war
schlecht gewählt.

Fix: `SIDE_CAP` 40 auf 3, neu `SIDE_GAP_NS` 150 ms (eine Spur, die länger
nicht gesehen wurde, wird nicht übernommen).

## Tests

- `engine::geom::tests::a_hand_taking_the_other_hands_place_gets_its_own_side`
- `calib::tests::a_pose_read_as_another_finger_is_reported_and_left_out`
  (dein Lauf vom 11-04 als Eingabe)
- `calib::tests::a_pose_waits_for_frames_when_perception_stalls`
- `calib::tests::skeletons_of_two_poses_are_told_apart`
- `calib::tests::thresholds_sit_between_folded_and_shown` (angepasst)

- `calib::tests::middle_alone_gets_a_template_only_when_its_skeleton_differs`

`./guarded-cargo.sh test -p synthexis --lib`: 284 grün. Ohne Vorlagen
liefert das Release-Binary auf S4 dieselben Seiten und Masken wie vorher
(4.589 von 4.589 Frames). Clippy: keine
Fehler, keine neuen Warnungen.

## Testschritte für Noa

Erst nach dem nächsten Install durch die UI-Session.

1. Seiten: Linke Hand 3 s zeigen, runter, sofort die rechte Hand an
   dieselbe Stelle. Die Fingeranzeige soll innerhalb eines Augenblicks
   auf R springen. Umgekehrt genauso. Bestanden, wenn es in 9 von 10
   Wechseln sofort stimmt.
2. Kalibrierung: "Calibrate fingers" starten und die Posen zeigen. Die
   Faust und die offene Hand sollen jetzt volle 3 s bekommen. Danach in
   `~/Library/Logs/synthexis.log` nach `calib:` suchen:
   - `pose 00000` und `pose 11111` haben mehr als 20 Frames.
   - Zeilen `reads as` nennen die Fehllesungen, erwartet: `00100 reads as
     01000` und `00010 reads as 01000` auf beiden Händen.
   - Hinter jeder Fehllesung steht `skeletons apart <Wert>, template
     on|off`. Bei `on` probierst du Mittelfinger allein: er soll jetzt
     als Mittelfinger leuchten. Danach Zeigefinger allein und einmal
     schräg gehalten: er muss Zeigefinger bleiben, sonst sofort "Reset".
     Bei `off` bleibt Mittelfinger allein ein Zeigefinger.
3. Nach der Kalibrierung Zeigefinger allein, dann kleiner Finger allein
   zeigen, je 3 s. Beide sollen sauber leuchten, ohne Nachbarfinger.
4. Mittelfinger allein bleibt falsch (leuchtet als Zeigefinger). Das ist
   erwartet bis zum neuen Training.

## Offen

- Neue Aufnahme mit den 20 fehlenden Masken und neues Training des
  Hand-Students. Das ist der Fix für Bug 1.
- UI: Fehllesungen nach der Kalibrierung anzeigen (`finger-ui-needs.md`).
- Die neue Kalibrierung speichert ihre Schwellen wie bisher. Einen alten
  Lauf mit 0,85 am rechten Zeigefinger behebt erst eine neue
  Kalibrierung oder "Reset".
