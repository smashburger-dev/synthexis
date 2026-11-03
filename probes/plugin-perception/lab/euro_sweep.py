from ramps import *
import statistics as st, sys, pickle, os
CACHE = os.path.join(SP, "euro_series.pkl")
if os.path.exists(CACHE):
    static, moving = pickle.load(open(CACHE, "rb"))
else:
    static, moving = [], []
    for s in ("S1", "S2", "S3"):
        F = frames(f"ba_{s}.jsonl"); G = segments(s)
        for sg in G.values():
            if sg["block"] == "A" and "both" not in sg["id"]:
                m0, m1 = sg.get("margin_s", [0, 0]); static.append(openness_series(F, sg["start_s_cam0"] + m0 + 0.5, sg["end_s_cam0"] - m1))
        F = frames(f"bc_{s}.jsonl")
        for side in "RL":
            sg = G[f"B_{side}_open"]; moving.append(openness_series(F, sg["start_s_cam0"], sg["end_s_cam0"]))
    pickle.dump((static, moving), open(CACHE, "wb"))
def run(series, mc, beta, dc):
    outs = []
    for ser in series:
        if len(ser) < 10: continue
        f = OneEuro(mc, beta, dc); prev = None; o = []
        for t, v in ser:
            dt = 1 / 30 if prev is None else max(t - prev, 1e-3); prev = t
            o.append(f.step(v, dt))
        outs.append((ser, o))
    return outs
def jitter(outs): return st.mean(st.mean(abs(o[i + 1] - o[i]) for i in range(len(o) - 1)) for _, o in outs)
def track_err(outs): return st.mean(st.mean(abs(a - b) for (_, a), b in zip(ser, o)) for ser, o in outs)
def half_rise_ms(mc, beta, dc):
    # step 0 -> 1 at 30 fps: time until the output crosses 0.5 and 0.9
    f = OneEuro(mc, beta, dc); f.step(0, 1 / 30); t50 = t90 = None
    for i in range(1, 120):
        o = f.step(1.0, 1 / 30)
        if t50 is None and o >= .5: t50 = i * 33.3
        if t90 is None and o >= .9: t90 = i * 33.3; break
    return t50, t90
if __name__ == "__main__":
  for mc, beta, dc in [tuple(map(float, a.split(","))) for a in sys.argv[1:]]:
      t50, t90 = half_rise_ms(mc, beta, dc)
      print(f"mc {mc} beta {beta} dc {dc}: static jitter {jitter(run(static, mc, beta, dc)):.4f}  ramp |err| {track_err(run(moving, mc, beta, dc)):.3f}  step 50% {t50:.0f} ms 90% {t90:.0f} ms")
