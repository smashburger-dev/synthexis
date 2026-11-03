# /// script
# requires-python = ">=3.11"
# ///
"""Gold TGT spans of the synthetic ds-v11c dev sets with their target kind,
as byte ranges, for plugin/complete/examples/sim_targets.rs (see RESULT.md).
Never reads the real set.

  uv run probes/completion-targets/targets.py [--data DIR] > targets.jsonl
"""

import argparse
import json
from pathlib import Path

SPLITS = "dev,challenge,dev_s2,dev_s3,dev_s4,dev_s5,dev_s6,dev_s7,dev_s8"


def kind(t: str) -> str:
    """T:param:filter.cutoff -> param:filter.cutoff, T:toggle:reverb ->
    toggle, T:midi.cc:74 -> midi.cc, T:macro:4 -> macro,
    T:midi.prop:arp.rate -> midi.prop:arp.rate."""
    k = t[2:].split(":", 1)
    if k[0] in ("param", "midi.prop"):
        return f"{k[0]}:{k[1]}"
    return k[0]


def routes(frame):
    out, cur = [], None
    for t in frame:
        if t == "<route>":
            cur = []
        elif t == "</route>":
            out.append(cur or [])
            cur = None
        elif cur is not None:
            cur.append(t)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default=str(Path(__file__).resolve().parents[2] / "data" / "mapping"))
    ap.add_argument("--ds", default="ds-v11c")
    a = ap.parse_args()
    for split in SPLITS.split(","):
        for line in open(Path(a.data) / "train" / a.ds / f"{split}.jsonl", encoding="utf-8"):
            r = json.loads(line)
            text = r["text"]
            byte = lambda c: len(text[:c].encode())
            tgts = [(byte(b), byte(e)) for b, e, k in r["bio"] if k == "TGT"]
            rs = routes([t for t in r["frame"] if isinstance(t, str)])
            ts = [t for x in rs for t in x if t.startswith("T:")]
            if len(ts) != len(tgts):
                ts = [next((t for t in x if t.startswith("T:")), None) for x in rs]
            if not tgts or len(ts) != len(tgts):
                continue
            tgt = [[b, e, kind(t)] for (b, e), t in zip(tgts, ts) if t]
            print(json.dumps({"split": split, "text": text, "tgt": tgt}, ensure_ascii=False))


if __name__ == "__main__":
    main()
