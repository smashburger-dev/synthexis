"""Generate spec/recording-protocol.json, the script the recorder app plays.

Each segment shows one instruction for a fixed time. Its labels are known by
construction, so no labeler has to guess gestures. Frames inside the start
and end margins are not used as labels (Noa is still switching poses).

Run: python3 tools/make_protocol.py
"""

import json
from pathlib import Path

FINGER_STATES = [  # bits: thumb, index, middle, ring, pinky
    ("00000", "Faust"),
    ("10000", "nur Daumen"),
    ("01000", "nur Zeigefinger"),
    ("11000", "Daumen + Zeigefinger"),
    ("01100", "Zeige- + Mittelfinger"),
    ("11100", "Daumen + Zeige- + Mittelfinger"),
    ("01110", "Zeige- + Mittel- + Ringfinger"),
    ("01111", "vier Finger, Daumen eingeklappt"),
    ("11111", "offene Hand"),
    ("00001", "nur kleiner Finger"),
    ("10001", "Daumen + kleiner Finger"),
    ("01001", "Zeigefinger + kleiner Finger"),
]
NAME = dict(FINGER_STATES)
SIDE = {"R": "Rechte Hand", "L": "Linke Hand"}
OTHER = {"R": "L", "L": "R"}
MOVE = "Halten, dabei langsam durchs Bild bewegen, mal nah, mal fern, Handgelenk drehen."

BOTH = [("11000", "11111"), ("01000", "00000"), ("11111", "11111"),
        ("00000", "00000"), ("01100", "10000"), ("10001", "01001")]

COUNT = ["01000", "01100", "01110", "01111", "11111", "01111", "01110", "01100", "01000"]

FACE = [
    ("neutral", 6, "Neutral in die Kamera schauen, Hände unten."),
    ("mouth_sweep", 12, "Mund langsam auf und zu, 4 s pro Zyklus, 3 Zyklen."),
    ("mouth_wide", 6, "Mund weit offen halten."),
    ("blink", 6, "Normal blinzeln, etwa einmal pro Sekunde."),
    ("wink_L", 6, "Linkes Auge zu, rechtes offen."),
    ("wink_R", 6, "Rechtes Auge zu, linkes offen."),
    ("eyes_closed", 6, "Beide Augen zu halten."),
    ("smile", 6, "Breit lächeln."),
    ("yaw_sweep", 12, "Kopf langsam links und rechts drehen, 4 s pro Zyklus."),
    ("pitch_sweep", 12, "Kopf langsam hoch und runter nicken, 4 s pro Zyklus."),
    ("roll_sweep", 12, "Kopf zur Schulter neigen, links und rechts, 4 s pro Zyklus."),
    ("turned_mouth", 12, "Kopf 45 Grad zur Seite, dabei Mund auf und zu."),
]

BODY = [
    ("arms_sweep", 12, "Beide Arme langsam hoch und runter."),
    ("lean_sweep", 12, "Oberkörper links und rechts neigen."),
    ("step_back", 12, "Einen Schritt zurück, Oberkörper ganz im Bild, Arme bewegen."),
]

OBJECTS = [
    ("mug", "Tasse"),
    ("phone", "Handy"),
    ("pen", "Stift oder Drumstick"),
    ("ball", "Ball oder Obst"),
    ("demo_object", "dein Demo-Objekt"),
]
OBJ_MOVE = "Halten, drehen, durchs Bild bewegen, ablegen, wieder aufnehmen, kurz aus dem Bild."

HARD = [
    ("hand_over_face", "Rechte Hand vor das Gesicht halten und bewegen."),
    ("hands_overlap", "Beide Hände überlappen, Finger verschränken, lösen."),
    ("fast_moves", "Hände schnell bewegen, wechselnde Gesten."),
    ("hand_edge", "Hand halb aus dem Bildrand, rein und raus."),
    ("backlit", "Wenn möglich: Licht von hinten (Fenster/Lampe hinter dir), Gesten zeigen."),
    ("dim", "Licht dimmen oder Lampe aus, Gesten zeigen."),
]


FREESTYLE = [
    ("free_hands", 30, "Freestyle Hände: eigene Gesten und Positionen, die dir einfallen."),
    ("free_face", 20, "Freestyle Gesicht und Kopf: eigene Ausdrücke und Bewegungen."),
    ("free_play", 30, "Freestyle: so tun, als würdest du gerade einen Track spielen."),
]

BLOCK_TITLES = {
    "A": "A Fingerhaltungen", "B": "B Langsame Zyklen und Zählen", "C": "C Gesicht und Kopf",
    "D": "D Körper", "E": "E Objekte", "F": "F Schwierige Fälle", "G": "G Freestyle",
}


def seg(block, sid, dur, text, margin=(0.7, 0.3), **labels):
    return {"id": sid, "block": block, "duration_s": dur, "instruction_de": text,
            "margin_s": list(margin), "labels": labels}


def finger_block(hold_s):
    out = []
    for side in ("R", "L"):
        for bits, name in FINGER_STATES:
            out.append(seg("A", f"A_{side}_{bits}", hold_s,
                           f"{SIDE[side]}: {name}. {MOVE} {SIDE[OTHER[side]]} aus dem Bild.",
                           **{f"hand_{side}": bits, f"hand_{OTHER[side]}": "absent"}))
    for r, l in BOTH:
        out.append(seg("A", f"A_both_{r}_{l}", hold_s,
                       f"Beide Hände. Rechts: {NAME[r]}. Links: {NAME[l]}. {MOVE}",
                       hand_R=r, hand_L=l))
    return out


def ramp_block():
    out = []
    for side in ("R", "L"):
        out.append(seg("B", f"B_{side}_open", 16,
                       f"{SIDE[side]}: langsam von Faust zu offen und zurück, 4 s pro Zyklus, im Takt der Anzeige.",
                       margin=(0.5, 0.0), ramp={"signal": f"hand.{side}.openness",
                                                "shape": "triangle", "period_s": 4, "start": "closed"}))
        out.append(seg("B", f"B_{side}_spread", 16,
                       f"{SIDE[side]}: offene Hand, Finger langsam spreizen und schließen, 4 s pro Zyklus.",
                       margin=(0.5, 0.0), ramp={"signal": f"hand.{side}.spread",
                                                "shape": "triangle", "period_s": 4, "start": "closed"}))
        for i, bits in enumerate(COUNT):
            out.append(seg("B", f"B_{side}_count{i}", 2,
                           f"{SIDE[side]} zählen: {NAME[bits]}.", margin=(0.5, 0.2),
                           **{f"hand_{side}": bits}))
    return out


def protocol(name, hold_s, objects, include_body, include_ramps):
    segs = finger_block(hold_s)
    if include_ramps:
        segs += ramp_block()
    segs += [seg("C", f"C_{k}", d, t, face=k) for k, d, t in FACE]
    if include_body:
        segs += [seg("D", f"D_{k}", d, t, body=k) for k, d, t in BODY]
    segs += [seg("E", f"E_{k}", 20, f"Objekt: {n}. {OBJ_MOVE}", objects=[k]) for k, n in objects]
    segs += [seg("F", f"F_{k}", 8, t, hard_case=k) for k, t in HARD]
    if include_body:
        # No gesture labels: these frames only get SAM 3.1 masks.
        segs += [seg("G", f"G_{k}", d, t, freestyle=k) for k, d, t in FREESTYLE]
    total = sum(s["duration_s"] for s in segs) + len(segs) * 1.0  # 1 s gap with next prompt
    return {"name": name, "gap_s": 1.0, "total_s": total, "segments": segs}


def markdown(protocols):
    lines = ["# Aufnahme-Skript", "",
             "Generiert aus `spec/recording-protocol.json` von `tools/make_protocol.py`. "
             "Die Recorder-App zeigt dir jede Zeile groß an, mit Countdown. Diese Liste ist zum Vorbereiten.", ""]
    for key, p in protocols.items():
        lines += [f"## Protokoll `{key}`: {len(p['segments'])} Abschnitte, {p['total_s'] / 60:.1f} min", ""]
        block = None
        for i, s in enumerate(p["segments"], 1):
            if s["block"] != block:
                block = s["block"]
                lines += ["", f"### {BLOCK_TITLES[block]}", "", "| # | Sekunden | Anweisung |", "|---|---|---|"]
            lines.append(f"| {i} | {s['duration_s']} | {s['instruction_de']} |")
        lines.append("")
    return "\n".join(lines)


def main():
    out = {
        "version": 1,
        "finger_bits": "thumb,index,middle,ring,pinky; 1 = extended",
        "protocols": {
            "full": protocol("full", 6, OBJECTS, True, True),
            "short": protocol("short", 4, OBJECTS[:3], False, False),
        },
    }
    root = Path(__file__).resolve().parents[1]
    (root / "spec" / "recording-protocol.json").write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n")
    (root / "docs" / "recording-script.md").write_text(markdown(out["protocols"]))
    for k, p in out["protocols"].items():
        print(f"{k}: {len(p['segments'])} segments, {p['total_s'] / 60:.1f} min")


if __name__ == "__main__":
    main()
