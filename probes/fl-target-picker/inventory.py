#!/usr/bin/env python3
"""Inventory probe: what a target picker can list from FL, and how long it
takes. Lists mixer tracks with names and effect slots, channel rack
generators, and per plugin the parameter names, at three page budgets.

  python3 probes/fl-target-picker/inventory.py [--repeat 3]

Writes runs/<ts>-inventory.json and prints a summary.
"""

import argparse
import statistics

import fl

BUDGETS = (3500, 16000, 56000)  # reply bytes per page; FL max frame ~65.5 KB


def names_of(p, budget):
    items, st = fl.paged("plist", "p", index=p["index"], slot=p["slot"], budget=budget)
    return items, st


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repeat", type=int, default=3)
    args = ap.parse_args()
    out = {"version": fl.ev("ui.getVersion()"), "runs": []}

    # light inventory: tracks, slots, generators (what the picker shows first)
    light = []
    for _ in range(args.repeat):
        _, sm = fl.paged("mixer", "tracks", budget=16000)
        _, sg = fl.paged("gens", "gens", budget=16000)
        light.append({"mixer": sm, "gens": sg})
    out["light"] = light
    tracks, _ = fl.paged("mixer", "tracks", budget=16000, empty=True)
    out["tracks"] = tracks
    gens, _ = fl.paged("gens", "gens", budget=16000)
    out["gens"] = gens
    plugs = fl.plugins_in_project()
    out["plugins"] = plugs

    # parameter names per plugin, per budget, repeated (cold vs warm)
    for budget in BUDGETS:
        for rep in range(args.repeat):
            for p in plugs:
                items, st = names_of(p, budget)
                row = dict(p, budget=budget, rep=rep, named=len(items),
                           last=items[-1][0] if items else None, **st)
                out["runs"].append(row)
                if rep == 0 and budget == BUDGETS[0]:
                    p["names"] = items

    path = fl.save("inventory", out)
    print(f"FL {out['version']}; {len(tracks)} mixer tracks, {len(gens)} channels, "
          f"{len(plugs)} plugin instances -> {path.name}")
    lm = [r["mixer"]["wall_ms"] for r in light]
    lg = [r["gens"]["wall_ms"] for r in light]
    print(f"light inventory: mixer {statistics.median(lm):.1f} ms "
          f"({light[0]['mixer']['bytes']} B), gens {statistics.median(lg):.1f} ms")
    for p in plugs:
        rows = [r for r in out["runs"] if r["index"] == p["index"] and r["slot"] == p["slot"]]
        named = rows[0]["named"]
        per = []
        for budget in BUDGETS:
            b = [r for r in rows if r["budget"] == budget]
            per.append(f"{budget // 1000}k: {statistics.median(x['wall_ms'] for x in b):.0f} ms"
                       f"/{b[0]['pages']}p/{b[0]['bytes']} B")
        fl_ms = statistics.median(r["fl_ms"] for r in rows)
        print(f"  {p['where']}/{p['slot']} {p['plugin']}: {named} named, "
              f"FL scan {fl_ms:.0f} ms; " + "; ".join(per))
    for budget in BUDGETS:
        tot = [sum(r["wall_ms"] for r in out["runs"] if r["budget"] == budget and r["rep"] == k)
               for k in range(args.repeat)]
        byt = sum(r["bytes"] for r in out["runs"] if r["budget"] == budget and r["rep"] == 0)
        print(f"all names, budget {budget}: {statistics.median(tot):.0f} ms, {byt} B")


if __name__ == "__main__":
    main()
