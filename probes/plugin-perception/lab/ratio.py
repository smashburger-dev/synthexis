import json, sys
from paths import REC
S = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}
def q(x, p): x = sorted(x); return round(x[int(p * (len(x) - 1))], 3) if x else None
for s, path in [a.split("=") for a in sys.argv[1:]]:
    segs = [x for x in json.load(open(REC + S[s] + "/segments.json")) if x["block"] == "A" and not x["bad"]]
    two, one = [], []
    for l in open(path):
        f = json.loads(l)
        if "raw" not in f: continue
        sg = next((x for x in segs if x["start_s_cam0"] + x.get("margin_s", [0, 0])[0] <= f["t"] <= x["end_s_cam0"] - x.get("margin_s", [0, 0])[1]), None)
        if not sg: continue
        c = sorted([c for c in f["raw"]["cands"] if c["a"] >= 60], key=lambda c: -c["a"])
        if len(c) < 2: continue
        n = sum(1 for k in "LR" if sg["labels"].get(f"hand_{k}") not in (None, "absent"))
        (two if n == 2 else one).append(c[1]["a"] / c[0]["a"])
    print(s, path.split("/")[-1], "two-hand 2nd/1st p1/p5/p10/med", q(two, .01), q(two, .05), q(two, .1), q(two, .5), "n", len(two),
          "| one-hand 2nd/1st p50/p90/p95/p99", q(one, .5), q(one, .9), q(one, .95), q(one, .99), "n", len(one))
