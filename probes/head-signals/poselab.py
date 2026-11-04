"""Head pose lab: weak-perspective pose (POS, least squares on a generic
rigid face model, the one in geom.rs `HEAD_MODEL`) from a bench dump, and
its yaw / pitch / roll per block C sweep, with the camera's own tilt taken
out by the neutral pose. `simulate` replays the engine's `HeadPose` with
other gates and ranges (the sweeps in docs/session-2026-11-04-head.md).

  python3 probes/head-signals/poselab.py <frames.jsonl> <segments.json> [model]

<frames.jsonl>: the bench's `head_pts` (7 points, geom.rs `head_points`).
Dumps of the lab build of 2026-11-04 carry the raw regions instead (`lm`:
crest, nose, median line, 12 slots each, plus eye centres in `head_pts`);
`points` reads both. model: rigid (eyes, nose tip, bridge, wings,
subnasale; default), chin (adds the median line's lowest point, lab dumps
only) or small (eyes, tip, bridge). Angles in degrees: yaw > 0 turned to
the person's left, pitch > 0 looking up, roll > 0 tilted to the left
shoulder.
"""

import json
import math
import sys

# Model in eye distances: x to the person's left (image right when facing
# the camera), y down, z into the head (the nose tip is in front: z < 0).
MODEL = {
    "eye_il": (-0.5, 0.0, 0.0),
    "eye_ir": (0.5, 0.0, 0.0),
    "tip": (0.0, 0.62, -0.45),
    "bridge": (0.0, 0.0, -0.20),
    "wing_il": (-0.28, 0.70, -0.12),
    "wing_ir": (0.28, 0.70, -0.12),
    "sub": (0.0, 0.74, -0.25),
    "chin": (0.0, 1.75, -0.10),
}
ORDER = ["eye_il", "eye_ir", "tip", "bridge", "wing_il", "wing_ir", "sub", "chin"]


def points(f):
    """The model's points from the bench's landmark dump (`lm`: crest,
    nose, median line, 12 slots each) and eye centres (`head_pts`)."""
    lm, hp = f.get("lm"), f.get("head_pts")
    if not hp or not any(hp):
        return None
    if not lm and len(hp) == 14:
        return [hp[2 * i: 2 * i + 2] for i in range(7)] + [None]
    if not lm:
        return None
    reg = [[(lm[2 * (12 * k + i)], lm[2 * (12 * k + i) + 1]) for i in range(12) if lm[2 * (12 * k + i)] is not None] for k in range(3)]
    crest, nose, median = reg
    if len(crest) < 2 or len(nose) < 4 or len(median) < 3:
        return None
    # pick in the eye-line frame: x along the eyes, y across (down the face)
    a, b = hp[0:2], hp[2:4]
    d = math.dist(a, b) or 1.0
    ex = ((b[0] - a[0]) / d, (b[1] - a[1]) / d)
    X = lambda p: p[0] * ex[0] + p[1] * ex[1]
    Y = lambda p: -p[0] * ex[1] + p[1] * ex[0]
    on_median = [p for p in crest if any(abs(p[0] - m[0]) + abs(p[1] - m[1]) < 0.5 for m in median)] or crest
    tip = max(on_median, key=Y)
    bridge = min(crest, key=Y)
    ys = [Y(p) for p in nose]
    # the wings' lower corners: the outline's bottom quarter
    low = [p for p in nose if Y(p) >= min(ys) + 0.75 * (max(ys) - min(ys))]
    wl, wr = min(low, key=X), max(low, key=X)
    after = [m for m in median if Y(m) > Y(tip) + 0.5]
    sub = min(after, key=Y) if after else tip
    chin = max(median, key=Y)
    return [hp[0:2], hp[2:4], tip, bridge, wl, wr, sub, chin]


def inv3(m):
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]


def pinv(points):
    """(A^T A)^-1 A^T for the model points (rows): 3 x N."""
    ata = [[sum(p[r] * p[c] for p in points) for c in range(3)] for r in range(3)]
    k = inv3(ata)
    return [[sum(k[r][j] * p[j] for j in range(3)) for p in points] for r in range(3)]


def norm(v):
    n = math.sqrt(sum(x * x for x in v))
    return [x / n for x in v]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def rotation(pts, names, P):
    """Rows r1, r2, r3 of the rotation (model -> camera, y down)."""
    p0 = [(pts[0][0] + pts[1][0]) / 2, (pts[0][1] + pts[1][1]) / 2]
    bx = [p[0] - p0[0] for p in pts]
    by = [p[1] - p0[1] for p in pts]
    I = [sum(P[r][k] * bx[k] for k in range(len(bx))) for r in range(3)]
    J = [sum(P[r][k] * by[k] for k in range(len(by))) for r in range(3)]
    r1, r2 = norm(I), norm(J)
    r3 = norm(cross(r1, r2))
    r2 = cross(r3, r1)
    return r1, r2, r3


def apply(R, v):
    return [dot(R[0], v), dot(R[1], v), dot(R[2], v)]


def rot_x(deg):
    t = math.radians(deg)
    c, s = math.cos(t), math.sin(t)
    return ([1, 0, 0], [0, c, -s], [0, s, c])


def matmul(A, B):
    return tuple([sum(A[i][k] * B[k][j] for k in range(3)) for j in range(3)] for i in range(3))


def angles(R):
    """yaw_left, pitch_up, roll_left in degrees from the facing direction
    and the head's x axis, roll against the un-rolled axis around it."""
    f = apply(R, [0, 0, -1])
    u = apply(R, [1, 0, 0])
    yaw = math.degrees(math.atan2(f[0], -f[2]))
    pitch = math.degrees(math.atan2(-f[1], math.hypot(f[0], f[2])))
    h0 = norm(cross([0, -1, 0], f))
    roll = -math.degrees(math.atan2(dot(cross(h0, u), f), dot(h0, u)))
    return yaw, pitch, roll


def window(frames, seg):
    m0, m1 = seg.get("margin_s", [0, 0])
    a, b = seg["start_s_cam0"] + m0, seg["end_s_cam0"] - m1
    return [f for f in frames if a <= f["t"] <= b]


def q(v, p):
    v = sorted(v)
    return v[int(p * (len(v) - 1))] if v else float("nan")


def main():
    frames = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
    segs = json.load(open(sys.argv[2]))
    variant = sys.argv[3] if len(sys.argv) > 3 else "rigid"
    names = {"rigid": ORDER[:7], "chin": ORDER, "small": ORDER[:4]}[variant]
    P = pinv([MODEL[n] for n in names])
    for f in frames:
        pts = points(f)
        if pts:
            f["R"] = rotation(pts[: len(names)], names, P)
    by = {s["id"]: [f for f in window(frames, s) if "R" in f] for s in segs if s["block"] == "C" and not s["bad"]}
    raw_neutral = [angles(f["R"]) for f in by["C_neutral"]]
    cam_pitch = q([a[1] for a in raw_neutral], 0.5)
    cam_yaw = q([a[0] for a in raw_neutral], 0.5)
    print(f"camera from the neutral pose: pitch {cam_pitch:+.1f}, yaw {cam_yaw:+.1f} deg")
    # head in the world frame: undo the camera's tilt (a rotation about x)
    C = rot_x(cam_pitch)
    for label, fix in (("camera frame", False), ("world frame", True)):
        print(label)
        for seg in ("C_neutral", "C_yaw_sweep", "C_pitch_sweep", "C_roll_sweep", "C_turned_mouth", "C_mouth_sweep", "C_smile"):
            a = [angles(matmul(C, f["R"]) if fix else f["R"]) for f in by[seg]]
            cols = []
            for k, n in enumerate(("yaw", "pitch", "roll")):
                v = [x[k] for x in a]
                cols.append(f"{n} {q(v, .05):+6.1f}..{q(v, .95):+6.1f} (med {q(v, .5):+5.1f})")
            print(f"  {seg:15s} n={len(a):3d}  " + "  ".join(cols))


if __name__ == "__main__":
    main()


def residual(pts, names, P):
    """RMS distance of the model fit per eye distance."""
    p0 = [(pts[0][0] + pts[1][0]) / 2, (pts[0][1] + pts[1][1]) / 2]
    bx = [p[0] - p0[0] for p in pts]
    by = [p[1] - p0[1] for p in pts]
    I = [sum(P[r][k] * bx[k] for k in range(len(bx))) for r in range(3)]
    J = [sum(P[r][k] * by[k] for k in range(len(by))) for r in range(3)]
    d = math.dist(pts[0], pts[1])
    M = [MODEL[n] for n in names]
    e = sum((dot(I, m) - x) ** 2 + (dot(J, m) - y) ** 2 for m, x, y in zip(M, bx, by))
    return math.sqrt(e / len(M)) / d


def transpose(A):
    return tuple([A[j][i] for j in range(3)] for i in range(3))


def orthonormal(M):
    r1 = norm(M[0])
    r3 = norm(cross(r1, M[1]))
    return (r1, cross(r3, r1), r3)


def median_rotation(Rs):
    """Elementwise median of rotations, made orthonormal again."""
    return orthonormal([[sorted(R[i][j] for R in Rs)[len(Rs) // 2] for j in range(3)] for i in range(3)])


def simulate(frames, gate, full=(45.0, 30.0, 35.0), names=ORDER[:7], centre_s=30.0, roll_gate=None, rel=True, yaw_per_roll=0.0):
    """Values per frame as the engine would emit them: gated by the fit
    residual (yaw and pitch at `gate`, roll at `roll_gate`; a gated axis
    holds its last value). rel: the head against the running median pose
    of the last `centre_s` seconds of ungated frames (R0^T R), so the
    camera's yaw, pitch and roll fall out; else yaw and pitch against their
    running medians and roll absolute."""
    roll_gate = gate if roll_gate is None else roll_gate
    P = pinv([MODEL[n] for n in names])
    hist = []
    last = [0.5, 0.5, 0.5]
    seen = False
    clamp = lambda v: max(-1.0, min(1.0, v))
    for f in frames:
        pts = points(f)
        if pts:
            pts = pts[: len(names)]
            R = rotation(pts, names, P)
            res = residual(pts, names, P)
            if res <= gate:
                hist.append((f["t"], R, angles(R)))
            hist = [h for h in hist if f["t"] - h[0] <= centre_s]
            if hist and res <= max(gate, roll_gate):
                if rel:
                    R0 = median_rotation([h[1] for h in hist[::3]])
                    yaw, pitch, roll = angles(matmul(transpose(R0), R))
                else:
                    cy = sorted(h[2][0] for h in hist)[len(hist) // 2]
                    cp = sorted(h[2][1] for h in hist)[len(hist) // 2]
                    yaw, pitch, roll = angles(matmul(rot_x(cp), R))
                    yaw -= cy
                # a tilt turns the head the same way (yaw per roll in the
                # roll sweeps 0.43 / 0.90 / 0.87, S1-S3); take it out
                yaw -= yaw_per_roll * roll
                if res <= gate:
                    last[0] = 0.5 - 0.5 * clamp(yaw / full[0])
                    last[1] = 0.5 + 0.5 * clamp(pitch / full[1])
                if res <= roll_gate:
                    last[2] = 0.5 - 0.5 * clamp(roll / full[2])
                seen = True
        if seen and "face" in f:
            f["face"] = f["face"][:6] + list(last)
