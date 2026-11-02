# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pedalboard==0.9.26", "zstandard", "cbor2"]
# ///
"""Serum 2 arpeggiator from the host side, measured with pedalboard.

  uv run probes/plugin-states/serum2_arp.py [section ...]

Sections (default: all):
  text    write raw values to Arp Rate / Range / ..., read raw_value and
          string_value back
  state   decode the saved processor state (XferJson: JSON header, zstd,
          CBOR) and diff it after host writes
  rate    render a held chord with the arp on, arp step length against
          the Arp Rate raw value
  range   render a held chord, distinct arp pitches against Arp Range
  transpose  Shift sweep in semitones, Range sweep in repeats
  bank    the VST3 program-change parameter "Bank" (Prog 1..128)
  slots   write 12 different slot shapes into the state, read them back,
          and trigger slots with MIDI notes 0..11 (Serum's default
          trigger octave)
  presets scan the factory .SerumPreset files (same container): arp use,
          slot shapes and slot parameters (no plugin instance needed)
"""

import hashlib
import json
import re
import struct
import sys
from collections import Counter
from pathlib import Path

import cbor2
import numpy as np
import pedalboard
import zstandard

SR = 48000
PLUGIN = "/Library/Audio/Plug-Ins/VST3/Serum2.vst3"
PRESETS = Path("/Library/Audio/Presets/Xfer Records/Serum 2 Presets")
B64 = ".ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+"
CHORD = (60, 64, 67)


# --- JUCE / Xfer state containers ------------------------------------

def b64dec(s: str) -> bytes:
    """JUCE MemoryBlock::fromBase64Encoding ("<size>.<chars>")."""
    n, enc = s.split(".", 1)
    out = bytearray(int(n))
    for i, c in enumerate(enc):
        v = B64.index(c)
        for b in range(6):
            pos = i * 6 + b
            if pos // 8 < len(out) and (v >> b) & 1:
                out[pos // 8] |= 1 << (pos % 8)
    return bytes(out)


def b64enc(data: bytes) -> str:
    nbits = len(data) * 8
    chars = []
    for i in range((nbits + 5) // 6):
        v = 0
        for b in range(6):
            pos = i * 6 + b
            if pos < nbits and data[pos // 8] >> (pos % 8) & 1:
                v |= 1 << b
        chars.append(B64[v])
    return f"{len(data)}." + "".join(chars)


def xfer_read(blob: bytes) -> tuple[dict, int, object]:
    """XferJson\\0, u64 header length, JSON header, u32 body length,
    u32 version, zstd(CBOR body)."""
    assert blob[:9] == b"XferJson\x00", blob[:9]
    hlen = struct.unpack("<Q", blob[9:17])[0]
    meta = json.loads(blob[17:17 + hlen])
    _, ver = struct.unpack("<II", blob[17 + hlen:25 + hlen])
    body = zstandard.ZstdDecompressor().decompress(
        blob[25 + hlen:], max_output_size=50_000_000)
    return meta, ver, cbor2.loads(body)


def xfer_write(meta: dict, ver: int, obj: object) -> bytes:
    """Inverse of xfer_read; the header hash is md5 of the zstd bytes."""
    body = cbor2.dumps(obj)
    z = zstandard.ZstdCompressor().compress(body)
    meta = {**meta, "hash": hashlib.md5(z).hexdigest()}
    head = json.dumps(meta, separators=(",", ":")).encode()
    return (b"XferJson\x00" + struct.pack("<Q", len(head)) + head
            + struct.pack("<II", len(body), ver) + z)


def component(p, tag: str = "IComponent") -> bytes:
    x = p.raw_state[8:].decode("utf-8", "replace")
    m = re.search(rf"<{tag}>(.*?)</{tag}>", x, re.S)
    return b64dec(m.group(1).strip())


def set_component(p, blob: bytes, tag: str = "IComponent") -> None:
    raw = p.raw_state
    x = raw[8:].decode("utf-8")
    x = re.sub(rf"<{tag}>.*?</{tag}>",
               lambda _: f"<{tag}>{b64enc(blob)}</{tag}>", x, flags=re.S)
    body = x.encode("utf-8")
    p.raw_state = raw[:4] + struct.pack("<I", len(body) - 1) + body


def diff(a, b, path="") -> list[str]:
    if isinstance(a, dict) and isinstance(b, dict):
        out = []
        for k in sorted(set(a) | set(b), key=str):
            out += diff(a.get(k), b.get(k), f"{path}/{k}")
        return out
    return [] if a == b else [f"{path}: {str(a)[:60]} -> {str(b)[:60]}"]


# --- audio --------------------------------------------------------------

def load():
    p = pedalboard.load_plugin(PLUGIN, plugin_name="Serum 2")
    p.parameters["arp_enable"].raw_value = 1.0
    return p


def render(p, notes, dur=4.0, extra=()):
    msgs = [(bytes([0x90, n, 100]), 0.0) for n in notes]
    msgs += list(extra)
    msgs += [(bytes([0x80, n, 0]), dur - 0.3) for n in notes]
    a = p(msgs, duration=dur, sample_rate=SR, num_channels=2, reset=True)
    return a.mean(axis=0)


def track(x, hop=0.01, win=0.03):
    """Autocorrelation pitch per frame as MIDI note; None when silent.
    Arp notes on the init saw are legato, so steps show up as pitch
    changes, not as energy onsets."""
    out = []
    lo, hi = int(SR / 1000), int(SR / 50)
    for t in np.arange(0, len(x) / SR - win, hop):
        seg = x[int(t * SR):int((t + win) * SR)]
        if np.sqrt((seg ** 2).mean()) < 1e-3:
            out.append(None)
            continue
        seg = seg - seg.mean()
        ac = np.correlate(seg, seg, "full")[len(seg) - 1:]
        k = lo + int(np.argmax(ac[lo:hi]))
        out.append(round(69 + 12 * np.log2(SR / k / 440)))
    return out


def runs(tr, hop=0.01):
    """Collapse a pitch track into [pitch, seconds] runs."""
    out = []
    for n in tr:
        if n is None:
            continue
        if out and out[-1][0] == n:
            out[-1][1] += hop
        else:
            out.append([n, hop])
    return [[n, round(d, 2)] for n, d in out]


# --- sections -------------------------------------------------------------

ARP = ("arp_rate", "arp_range", "arp_shift", "arp_offset", "arp_repeats",
       "arp_gate", "arp_chance", "arp_retrig_rate", "arp_transpose")


def sec_text(p):
    out = {}
    for name in ARP:
        q = p.parameters[name]
        rows = []
        for raw in np.linspace(0, 1, 21):
            q.raw_value = float(raw)
            rows.append((round(float(raw), 3), round(q.raw_value, 4),
                         q.string_value))
        q.raw_value = 0.5
        snapped = sum(1 for a, b, _ in rows if abs(a - b) > 1e-4)
        out[name] = {"label": q.label, "range": q.range,
                     "snapped": snapped, "samples": rows[::5]}
    return out


def sec_state(p):
    meta, _, base = xfer_read(component(p))
    out = {"keys": sorted(base)[:12], "Arp0": base.get("Arp0"),
           "ArpClip0": {k: v for k, v in base["ArpClip0"].items()
                        if k != "clip"}}
    for name, raw in (("arp_rate", 0.9), ("arp_range", 1.0),
                      ("arp_gate", 0.1), ("a_level", 0.2)):
        old = p.parameters[name].raw_value
        p.parameters[name].raw_value = raw
        render(p, CHORD, dur=0.5)
        _, _, st = xfer_read(component(p))
        out[f"{name}={raw}"] = diff(base, st)[:6]
        p.parameters[name].raw_value = old
    return out


def sec_rate(p):
    """Fine sweep around 0 %: does the step length move in note values?"""
    rows = []
    for raw in np.linspace(0.3, 0.62, 33):
        p.parameters["arp_rate"].raw_value = float(raw)
        r = runs(track(render(p, CHORD, dur=6.0), hop=0.005, win=0.02),
                 hop=0.005)[1:-1]  # drop partial ends
        step = float(np.median([d for _, d in r])) if len(r) > 1 else None
        rows.append({"raw": round(float(raw), 3),
                     "text": p.parameters["arp_rate"].string_value,
                     "steps": len(r), "step_s": step,
                     "beats_at_120": round(step / 0.5, 4) if step else None})
    p.parameters["arp_rate"].raw_value = 0.5
    return {"distinct_step_s": sorted({r["step_s"] for r in rows
                                       if r["step_s"]}), "rows": rows}


def sec_range(p):
    """Range repeats the pattern transposed by Shift; both are offsets."""
    rows = []
    for shift in (0.5, 0.55, 0.6, 0.75, 1.0):
        for rng in (0.5, 0.6, 0.75, 1.0):
            p.parameters["arp_shift"].raw_value = shift
            p.parameters["arp_range"].raw_value = rng
            r = runs(track(render(p, CHORD, dur=4.0)))
            rows.append({"shift": p.parameters["arp_shift"].string_value,
                         "range": p.parameters["arp_range"].string_value,
                         "pitches": sorted({n for n, _ in r}),
                         "sequence": [n for n, _ in r][:16]})
    return rows


def sec_transpose(p):
    """Shift sweep (Range +20 %): semitones of the first repeat. Range
    sweep (Shift fixed): number of pattern repeats."""
    base = set(CHORD)
    shift = []
    p.parameters["arp_range"].raw_value = 0.6
    for raw in np.linspace(0.5, 0.8, 31):
        p.parameters["arp_shift"].raw_value = float(raw)
        pitches = {n for n, d in runs(track(render(p, CHORD))) if d >= 0.05}
        up = sorted(n - 60 for n in pitches - base if n > 60)
        shift.append((round(float(raw), 3),
                      p.parameters["arp_shift"].string_value,
                      up[0] if up else 0))
    rng = []
    p.parameters["arp_shift"].raw_value = 0.6  # +20 %
    for raw in np.linspace(0.5, 0.8, 16):
        p.parameters["arp_range"].raw_value = float(raw)
        seq = [n for n, d in runs(track(render(p, CHORD, dur=6.0)))
               if d >= 0.05]
        rng.append((round(float(raw), 3),
                    p.parameters["arp_range"].string_value,
                    len(set(seq)), seq[:12]))
    return {"shift_semitones": shift, "range_distinct_pitches": rng}


def sec_bank(p):
    out = []
    meta0 = xfer_read(component(p, "IEditController"))[0]
    x0 = render(p, CHORD, dur=1.0)
    for k in (0, 5, 40):
        p.parameters["bank"].raw_value = k / 127
        x = render(p, CHORD, dur=1.0)
        meta = xfer_read(component(p, "IEditController"))[0]
        out.append({"raw": round(k / 127, 4),
                    "string": p.parameters["bank"].string_value,
                    "preset_name": meta.get("presetName"),
                    "audio_diff_db": round(float(20 * np.log10(
                        np.abs(x - x0).max() + 1e-12)), 1)})
    out.insert(0, {"preset_name_before": meta0.get("presetName")})
    return out


# shape names as factory .SerumPreset files store them (kParamShape)
SLOT_SHAPES = ["Up", "Down", "UpDown", "Chord", "Converge", "Played",
               "DownUp", "ThumbUD", "Down", "Chord", "UpDown", "Down"]


def sec_slots(p):
    """Write 12 different slot shapes into the processor state, then hold
    the chord after a trigger note k (0..11) and read the note order."""
    meta, ver, st = xfer_read(component(p))
    for i, shape in enumerate(SLOT_SHAPES):
        st[f"ArpClip{i}"] = {"name": f"Slot {i + 1}",
                             "plainParams": {"kParamShape": shape}}
    set_component(p, xfer_write(meta, ver, st))
    _, _, back = xfer_read(component(p))
    shapes = [(back[f"ArpClip{i}"].get("plainParams") or {})
              .get("kParamShape") if isinstance(
                  back[f"ArpClip{i}"].get("plainParams"), dict) else
              back[f"ArpClip{i}"].get("plainParams") for i in range(12)]
    p.parameters["arp_enable"].raw_value = 1.0
    rows = []
    for k in [None] + list(range(12)):
        trig = [] if k is None else [(bytes([0x90, k, 100]), 0.0),
                                     (bytes([0x80, k, 0]), 0.05)]
        msgs = trig + [(bytes([0x90, n, 100]), 0.02) for n in CHORD] \
            + [(bytes([0x80, n, 0]), 2.7) for n in CHORD]
        x = p(msgs, duration=3.0, sample_rate=SR, num_channels=2,
              reset=True).mean(axis=0)
        seq = [n for n, d in runs(track(x)) if d >= 0.05]
        rows.append({"trigger_note": k, "sequence": seq[:9]})
    return {"shapes_written": SLOT_SHAPES, "shapes_read_back": shapes,
            "rows": rows}


def sec_presets(_p=None):
    files = sorted((PRESETS / "Presets").rglob("*.SerumPreset"))
    shapes, slot_keys, arp0_keys, distinct = Counter(), Counter(), \
        Counter(), Counter()
    enabled = bad = 0
    for f in files:
        try:
            _, _, o = xfer_read(f.read_bytes())
        except Exception:
            bad += 1
            continue
        a0 = (o.get("Arp0") or {}).get("plainParams")
        if not (isinstance(a0, dict) and a0.get("kParamEnabled") == 1.0):
            continue
        enabled += 1
        arp0_keys.update(a0.keys())
        slots = [o.get(f"ArpClip{i}") or {} for i in range(12)]
        distinct[len({repr(x) for x in slots})] += 1
        for x in slots:
            pp = x.get("plainParams")
            if isinstance(pp, dict):
                slot_keys.update(pp.keys())
                shapes[pp.get("kParamShape", "(default)")] += 1
            else:
                shapes["(default)"] += 1
    return {"presets": len(files), "unreadable": bad,
            "arp_enabled": enabled,
            "distinct_slots_per_arp_preset": sorted(distinct.items()),
            "slot_shapes": shapes.most_common(),
            "slot_param_keys": slot_keys.most_common(),
            "arp0_param_keys": arp0_keys.most_common(),
            "factory_arp_banks": len(list(
                (PRESETS / "Arp Banks" / "Factory").glob("*.XferArpBank"))),
            "factory_arp_patterns": len(list(
                (PRESETS / "Arp Patterns" / "Factory").glob("*.XferArp")))}


SECTIONS = {"text": sec_text, "state": sec_state, "rate": sec_rate,
            "range": sec_range, "transpose": sec_transpose,
            "bank": sec_bank, "slots": sec_slots, "presets": sec_presets}


def main() -> None:
    want = sys.argv[1:] or list(SECTIONS)
    for name in want:
        p = None if name == "presets" else load()  # fresh per section
        print(f"== {name}")
        print(json.dumps(SECTIONS[name](p), default=str, indent=1))


if __name__ == "__main__":
    main()
