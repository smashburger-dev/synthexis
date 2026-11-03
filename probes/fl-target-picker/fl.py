"""Shared helpers for the target-picker probes: bridge calls with timing,
paged listing, run files. Needs flbridged (this folder) and the protocol-2
device script installed in FL (see README.md)."""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "fl-camera-plugin" / "bridge"))
from flb import call  # noqa: E402

RUNS = Path(__file__).resolve().parent / "runs"


def must(r):
    if "error" in r:
        raise RuntimeError(r)
    return r


def timed(cmd, **args):
    """One bridge call; returns (reply, wall ms, reply bytes)."""
    t0 = time.perf_counter()
    r = call(cmd, **args)
    ms = (time.perf_counter() - t0) * 1000
    return must(r), ms, len(json.dumps(r, separators=(",", ":")))


def paged(cmd, key, **args):
    """Fetch every page of a paged command. Returns (items, stats)."""
    items, start = [], args.pop("start", 0)
    st = {"pages": 0, "wall_ms": 0.0, "fl_ms": 0.0, "bytes": 0}
    while start is not None:
        r, ms, n = timed(cmd, start=start, **args)
        items += r[key]
        st["pages"] += 1
        st["wall_ms"] += ms
        st["fl_ms"] += r.get("ms", 0.0)
        st["bytes"] += n
        start = r["next"]
    return items, st


def ev(code):
    return must(call("eval", code=code))["r"]


def save(name, obj):
    RUNS.mkdir(exist_ok=True)
    p = RUNS / f"{time.strftime('%Y%m%d-%H%M%S')}-{name}.json"
    p.write_text(json.dumps(obj, indent=1))
    return p


def plugins_in_project():
    """Every plugin instance: generators (slot -1) and mixer effects."""
    out = []
    gens, _ = paged("gens", "gens", budget=16000)
    for g in gens:
        if g["plugin"]:
            out.append({"kind": "gen", "index": g["i"], "slot": -1,
                        "plugin": g["plugin"], "where": g["name"]})
    tracks, _ = paged("mixer", "tracks", budget=16000)
    for t in tracks:
        for s in t["slots"]:
            out.append({"kind": "fx", "index": t["t"], "slot": s["s"],
                        "plugin": s["plugin"], "where": t["name"]})
    return out
