# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy", "pedalboard==0.9.26"]
# ///
"""Sentence -> parser -> open-vocabulary resolver -> parameter write into
the real plugin -> rendered audio, measured.

  uv run probes/open-vocab-mapping/live_write.py

Each case loads the plugin in pedalboard (a generic VST3/AU host, standing in
for FL's setParamValue), renders a test signal with the resolved parameter at
raw 0.0 and at raw 1.0, and reports how much the output changed against a
control (the same setting rendered twice). A write counts as audible when the
change is at least 6 dB above the control. Optional 'pre' writes set other
parameters first (e.g. switch a ShaperBox module on).
"""

import sys
from pathlib import Path

import numpy as np
import pedalboard

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resolve as rv  # noqa: E402

SR = 48000
VST3 = "/Library/Audio/Plug-Ins/VST3/"
AU = "/Library/Audio/Plug-Ins/Components/"
PATHS = {
    "Cableguys/ShaperBox 3": VST3 + "ShaperBox 3.vst3",
    "Output/Portal": VST3 + "Portal.vst3",
    "Output/Thermal": AU + "Thermal.component",
    "Valhalla DSP, LLC/ValhallaDelay": VST3 + "ValhallaDelay.vst3",
    "FabFilter/Timeless 3": VST3 + "FabFilter Timeless 3.vst3",
    "Universal Audio (UADx)/UADx Galaxy Tape Echo":
        VST3 + "uaudio_galaxy_tape_echo.vst3",
}
CASES = [  # (sentence, {param: raw value} written before the sweep)
    ("mehr Noise Low Mix in ShaperBox, wenn ich die Hand öffne", {}),
    ("mehr Noise Low Mix in ShaperBox, wenn ich die Hand öffne",
     {"NoiseShaper On": 1.0}),
    ("my fist switches the volume shaper on in ShaperBox", {}),
    ("pinch controls the grain dry wet in Portal", {}),
    ("die Faust steuert den Drive in Thermal", {}),
    ("spread my fingers to open Diffusion in Valhalla Delay", {}),
    ("fist closes the Feedback on Timeless", {}),
    ("head tilt changes the loop age on the Galaxy Tape Echo", {}),
]


def signal() -> np.ndarray:
    """2 s of saw chord and noise in 8th-note bursts, then 1 s of silence
    so delay and reverb tails show up."""
    t = np.arange(2 * SR) / SR
    saw = sum(2 * ((t * f) % 1) - 1 for f in (110, 165, 220.5)) / 3
    noise = np.random.default_rng(0).standard_normal(t.size) * 0.3
    env = np.exp(-((t % 0.25) * 18))
    x = (saw * 0.5 + noise) * env * 0.5
    x = np.concatenate([x, np.zeros(SR)])
    return np.stack([x, np.roll(x, 37)]).astype(np.float32)


def key_for(plugin, name: str):
    """pedalboard keys are snake_case ('volumeshaper_on'); compare letters
    and digits only."""
    want = "".join(rv.words(name))
    for k in plugin.parameters:
        if "".join(rv.words(k)) == want:
            return k
    return None


def render(plugin, x, settings: dict) -> np.ndarray:
    for k, v in settings.items():
        plugin.parameters[k].raw_value = v
    return plugin.process(x, SR, reset=True)


def db(a: np.ndarray, ref: np.ndarray) -> float:
    return 20 * np.log10(np.sqrt(np.mean(a ** 2)) /
                         max(np.sqrt(np.mean(ref ** 2)), 1e-12) + 1e-12)


def centroid(y: np.ndarray) -> float:
    s = np.abs(np.fft.rfft(y.mean(0)))
    f = np.fft.rfftfreq(y.shape[1], 1 / SR)
    return float((s * f).sum() / max(s.sum(), 1e-12))


def main() -> None:
    params, cls = rv.inventory()
    parser = rv.Parser("p-v3-c50-s0")
    x = signal()
    loaded = {}
    for (text, pre), p in zip(CASES, parser([c[0] for c in CASES])):
        pid, ranked = rv.resolve(p, params, cls, "span")
        name = ranked[0][1] if ranked else None
        print(f"\n{text}\n  span {p['tgt']} -> {pid} / {name}"
              + (f" (pre {pre})" if pre else ""))
        if not name:
            continue
        try:
            if pid not in loaded:
                loaded[pid] = pedalboard.load_plugin(PATHS[pid])
            plug = loaded[pid]
        except Exception as e:                       # noqa: BLE001
            print(f"  load failed: {str(e).splitlines()[0][:120]}")
            continue
        k = key_for(plug, name)
        pre_k = {key_for(plug, n): v for n, v in pre.items()}
        if k is None or None in pre_k:
            print("  parameter not found in host")
            continue
        base = {pk: plug.parameters[pk].raw_value for pk in pre_k}
        base[k] = plug.parameters[k].raw_value
        lo = render(plug, x, {**pre_k, k: 0.0})
        lo2 = render(plug, x, {**pre_k, k: 0.0})
        hi = render(plug, x, {**pre_k, k: 1.0})
        shown = str(plug.parameters[k])[:80]
        ctrl, change = db(lo2 - lo, lo), db(hi - lo, lo)
        audible = change - ctrl >= 6
        print(f"  write raw 0 -> 1: display {shown}")
        print(f"  change {change:6.1f} dB vs control {ctrl:6.1f} dB -> "
              f"{'AUDIBLE' if audible else 'no change'}; level "
              f"{db(lo, x):+.1f} -> {db(hi, x):+.1f} dB, centroid "
              f"{centroid(lo):.0f} -> {centroid(hi):.0f} Hz")
        render(plug, x[:, :100], base)               # restore defaults


if __name__ == "__main__":
    main()
