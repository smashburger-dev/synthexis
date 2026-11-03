"""Hand openness candidates replayed on raw keypoints (Python copy of geom.rs,
equal to the bench within 0.0003; RESULT.md, "Fist curve and two hands on x").

  python3 openlab.py            # S1-S3: the decile map (C0, before 11-03) and the current line

Reads ba_<S>.jsonl (block A) and bc_<S>.jsonl (B ramps) with raw keypoints
from paths.WORK (lab/dump.sh, SYNTHEXIS_BENCH_RAW=1). A new candidate is a
Map(...) passed to report(); the gates it has to pass are in RESULT.md.
"""
import statistics as st
from paths import dev_ids
from ramps import frames, segments, window, finger_cues

q = lambda x, p: sorted(x)[int(p * (len(x) - 1))] if x else float("nan")
lin = lambda v, lo, hi: min(1.0, max(0.0, (v - lo) / (hi - lo)))
# geom.rs FINGER_BEND, FINGER_REACH, OPEN_LO, OPEN_HI
BEND = [(0.61, 3.68), (0.60, 3.53), (0.62, 3.45), (0.79, 3.56)]
REACH = [(0.96, 1.95), (0.77, 2.00), (0.67, 1.83), (0.60, 1.57)]
OPEN_LO, OPEN_HI = 0.079, 0.910
# FINGER_DECILES, dropped 11-03 (it pulled half-open hands to 0.5)
DEC = [0.0, 0.042, 0.079, 0.124, 0.190, 0.306, 0.583, 0.764, 0.879, 0.955, 1.0]


def dec(u):
    k = min(max(i for i, x in enumerate(DEC) if x <= u), 9)
    return (k + (u - DEC[k]) / max(DEC[k + 1] - DEC[k], 1e-6)) / 10


def thumb(c):
    return 0.5 * (1 - lin(c[0], 0.2, 1.4)) + 0.5 * lin(c[1], 0.35, 0.9)


def u_of(c, fi, bend, reach):
    b, r = bend[fi - 1], reach[fi - 1]
    return 0.5 * (1 - lin(c[0], b[0], b[1])) + 0.5 * lin(c[2], r[0], r[1])


def kp_of(f, side):
    """Keypoints of the candidate the bench used for this side: the candidate
    whose palm x is nearest to the emitted hand x."""
    h = f.get(f"hand_{side}")
    cs = [c for c in f.get("raw", {}).get("cands", []) if c.get("kp")]
    if not h or not cs:
        return None
    fw = 1280.0

    def px(c):
        k = c["kp"]
        return 1 - ((k[0] + k[10] + k[18] + k[34]) / 4) / fw

    k = min(cs, key=lambda c: abs(px(c) - h[8]))["kp"]
    return [(k[2 * i], k[2 * i + 1]) for i in range(21)]


def load(sessions, prefix):
    return {s: (frames(f"{prefix}_{s}.jsonl"), segments(s)) for s in sessions}


class Map:
    """curve 'dec' (decile map per finger) or 'lin'; then either a knee
    (a, b: above a, stretch so b reads 1) or lin(mean, lo, hi)."""

    def __init__(self, curve, bend=BEND, reach=REACH, lo=0.0, hi=1.0, knee=None):
        self.curve, self.bend, self.reach, self.lo, self.hi, self.knee = curve, bend, reach, lo, hi, knee

    def fingers(self, kp):
        c = finger_cues(kp)
        f = [thumb(c[0])]
        for fi in range(1, 5):
            u = u_of(c[fi], fi, self.bend, self.reach)
            f.append(dec(u) if self.curve == "dec" else u)
        return f

    def openness(self, kp):
        o = sum(self.fingers(kp)) / 5
        if self.knee is not None:
            a, b = self.knee
            return o if o <= a else min(1.0, a + (o - a) * (1 - a) / (b - a))
        return lin(o, self.lo, self.hi)


def block_a(D, m):
    """Openness on open-hand and fist frames, the median frame step on held
    mixed poses, dev finger accuracy (fingers 1-4 above 0.5 against labels)."""
    op, fi, mixed, ok, n = [], [], [], 0, 0
    for s, (F, G) in D.items():
        dv = dev_ids(s)
        for sg in G.values():
            if sg["block"] != "A":
                continue
            labs = {k: sg["labels"].get(f"hand_{k}") for k in "LR"}
            one = [k for k in "LR" if labs[k] not in (None, "absent")]
            if len(one) != 1:
                continue
            k = one[0]
            lab = labs[k]
            vals = []
            for f in window(F, sg):
                kp = kp_of(f, k)
                if kp is None:
                    continue
                vals.append(m.openness(kp))
                if sg["id"] in dv:
                    fs = m.fingers(kp)
                    for j in range(1, 5):
                        ok += (fs[j] > 0.5) == (lab[j] == "1")
                        n += 1
            if lab == "11111":
                op += vals
            elif lab == "00000":
                fi += vals
            elif len(vals) > 5:
                mixed.append(st.median(abs(a - b) for a, b in zip(vals, vals[1:])))
    return op, fi, mixed, ok / max(n, 1)


def ramps(D, m):
    """Share of B ramp frames in 0.4..0.6 and the median 20% to 80% time."""
    durs, mid = [], []
    for s, (F, G) in D.items():
        for side in "RL":
            sg = G.get(f"B_{side}_open")
            if not sg:
                continue
            ser = [(f["t"], m.openness(kp)) for f in window(F, sg) if (kp := kp_of(f, side))]
            ts = [t for t, _ in ser]
            v = [o for _, o in ser]
            v = [st.median(v[max(0, i - 2): i + 3]) for i in range(len(v))]
            mid += [0.4 <= x <= 0.6 for x in v]
            i = 0
            while i < len(v) - 1:
                if v[i] < 0.2 or v[i] > 0.8:
                    low = v[i] < 0.2
                    j = i
                    while j < len(v) and ((v[j] < 0.2) if low else (v[j] > 0.8)):
                        j += 1
                    k = j
                    while k < len(v) and 0.2 <= v[k] <= 0.8:
                        k += 1
                    if k < len(v) and ((v[k] > 0.8) if low else (v[k] < 0.2)):
                        durs.append(ts[k] - ts[max(j - 1, 0)])
                    i = k + 1
                else:
                    i += 1
    return sum(mid) / max(len(mid), 1), 1000 * st.median(durs) if durs else float("nan"), len(durs)


def report(name, A, B, m):
    op, fi, jit, acc = block_a(A, m)
    mid, d, n = ramps(B, m)
    print(f"{name:10s} open p10/p25/p50 {q(op, .1):.2f} {q(op, .25):.2f} {q(op, .5):.2f}"
          f" | fist p50/p75/p90 {q(fi, .5):.2f} {q(fi, .75):.2f} {q(fi, .9):.2f}"
          f" | dev fingers {acc:.3f} | mixed step {st.median(jit):.4f}"
          f" | ramps in 0.4..0.6 {mid:.2f}, 20-80% {d:.0f} ms (n={n})")


if __name__ == "__main__":
    S = ["S1", "S2", "S3"]
    A, B = load(S, "ba"), load(S, "bc")
    report("C0 decile", A, B, Map("dec", knee=(0.6, 0.88)))
    report("current", A, B, Map("lin", lo=OPEN_LO, hi=OPEN_HI))
