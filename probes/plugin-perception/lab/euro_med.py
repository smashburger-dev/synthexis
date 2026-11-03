from euro_sweep import *
import pickle, os
q = lambda x, p: sorted(x)[int(p * (len(x) - 1))]
def chain(ser, med, mc, beta, dc, eps=0.0):
    f = OneEuro(mc, beta, dc); prev = None; buf = []; o = []; out = None
    for t, v in ser:
        dt = 1 / 30 if prev is None else max(t - prev, 1e-3); prev = t
        buf = (buf + [v])[-med:] if med > 1 else [v]
        x = f.step(sorted(buf)[len(buf) // 2], dt)
        if out is None: out = x
        d = x - out
        if abs(d) > eps: out += d - eps * (1 if d > 0 else -1)
        o.append(out)
    return o
def score(static, moving, med, mc, beta, dc, eps=0.0):
    sd, rng, step = [], [], []
    for ser in static:
        if len(ser) < 30: continue
        o = chain(ser, med, mc, beta, dc, eps)[10:]
        sd.append(st.pstdev(o)); s = sorted(o); rng.append(s[int(.95 * len(s))] - s[int(.05 * len(s))])
        step += [abs(o[i + 1] - o[i]) for i in range(len(o) - 1)]
    err = st.mean(st.mean(abs(a - b) for (_, a), b in zip(ser, chain(ser, med, mc, beta, dc, eps))) for ser in moving if len(ser) > 10)
    # step response 0 -> 1
    o = chain([(i / 30, 0.0 if i < 5 else 1.0) for i in range(60)], med, mc, beta, dc, eps)
    t90 = next(i for i, x in enumerate(o) if x >= .9) - 5
    return f"hold std {q(sd,.5):.3f} p5-p95 {q(rng,.5):.3f} step p99 {q(step,.99):.3f} mean {st.mean(step):.4f} | ramp |err| {err:.3f} | 0->1 step 90% after {t90*33:.0f} ms"
if __name__ == "__main__":
    for a in sys.argv[1:]:
        med, mc, beta, dc, eps = a.split(",")
        print(a, score(static, moving, int(med), float(mc), float(beta), float(dc), float(eps)))
