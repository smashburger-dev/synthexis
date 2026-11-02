# /// script
# requires-python = ">=3.11"
# ///
"""Numbers for RESULT.md from data/mapping/harvest/states/.

  uv run probes/plugin-states/stats.py
"""

import json
import re
from collections import Counter

from run_states import HARVEST, OUT

LETTER = re.compile(r"[A-Za-z]")


def rows(fmt: str) -> list[dict]:
    seen, out = set(), []
    with (OUT / f"{fmt}.jsonl").open() as f:
        for line in f:
            r = json.loads(line)
            k = (r["key"], r["class_name"], r["index"])
            if k not in seen:
                seen.add(k)
                out.append(r)
    return out


def partial_in_harvest(fmt: str) -> set:
    """Stepped parameters whose harvest row lacks some state texts."""
    out = set()
    with (HARVEST / fmt / "params.jsonl").open() as f:
        for line in f:
            r = json.loads(line)
            if fmt == "vst3":
                if r["steps"] > 0 and len(r["samples"]) < r["steps"] + 1:
                    out.add((r["plugin_id"], r["class_name"], r["id"]))
            elif r.get("unit") == "indexed":
                n = int(round(r["max"] - r["min"])) + 1
                if len(r.get("value_strings") or []) < n:
                    out.add((r["plugin_id"], None, r["address"]))
    return out


def report(fmt: str) -> dict:
    rs = rows(fmt)
    steps = [r for r in rs if r["source"] == "steps"]
    dense = [r for r in rs if r["source"] == "dense"]
    partial = partial_in_harvest(fmt)
    gained = [r for r in steps
              if (r["plugin_id"], r["class_name"], r["id"]) in partial]
    def n_states(rr):
        return sum(len(r.get("states", [])) for r in rr)
    dup = [r for r in steps if r.get("states") and
           len({s[1] for s in r["states"]}) < len(r["states"])]
    named_dense = [r for r in dense
                   if any(LETTER.search(s[1]) for s in r["states"])]
    return {
        "stepped_params": len(steps),
        "stepped_plugins": len({r["key"] for r in steps}),
        "stepped_states": n_states(steps),
        "states_per_param_median": sorted(
            len(r["states"]) for r in steps)[len(steps) // 2],
        "params_more_than_11_states": sum(
            1 for r in steps if len(r.get("states", [])) > 11),
        "harvest_partial_params": len(partial),
        "harvest_partial_now_complete": sum(
            1 for r in gained if not r.get("truncated")),
        "harvest_partial_plugins": len({r["key"] for r in gained}),
        "states_gained": n_states(gained) - sum(
            min(11 if fmt == "vst3" else 64, len(r["states"]))
            for r in gained),
        "truncated": [(r["plugin_id"], r["name"], r["steps"])
                      for r in rs if r.get("truncated")],
        "states_empty_text": sum(1 for r in steps for s in r["states"]
                                 if not s[1]),
        "params_with_duplicate_texts": len(dup),
        "dense_params": len(dense),
        "dense_plugins": len({r["key"] for r in dense}),
        "dense_named_params": len(named_dense),
        "dense_named_plugins": len({r["key"] for r in named_dense}),
        "dense_states": n_states(dense),
        "program_change_params": [(r["plugin_id"], r["name"], r["steps"])
                                  for r in rs if r.get("program_change")],
    }


def main() -> None:
    out = {fmt: report(fmt) for fmt in ("vst3", "au")}
    pc = out["vst3"].pop("program_change_params")
    out["vst3"]["program_change_params_n"] = len(pc)
    out["vst3"]["program_change_plugins"] = len({p for p, _, _ in pc})
    out["vst3"]["program_change_examples"] = pc[:8]
    out["au"].pop("program_change_params")
    serum = [r for r in rows("vst3") if r["plugin_id"] == "Xfer Records/Serum 2"
             and r["class_name"] == "Serum 2" and r["name"] == "A Warp Mode"]
    if serum:
        out["serum2_warp_mode"] = {
            "states": len(serum[0]["states"]),
            "fm_states": [s for s in serum[0]["states"] if "FM" in s[1]]}
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
