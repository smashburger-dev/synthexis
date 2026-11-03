# /// script
# requires-python = ">=3.11"
# ///
"""Generated training sentences that come close to the real-sentence set.

  uv run probes/real-eval/leak.py --runs cmd-s4b [--ratio 0.7]

Compares every sentence of the command runs with every row of
data/mapping/eval/expanded.jsonl: shared word 6-grams and the word
sequence ratio. Sentences at or above --ratio, or sharing a 6-gram, are
written to data/mapping/commands/<run>/leak_drop.json for
dataset.py --exclude, so no near-copy of Noa's sentences is trained on.
"""

import argparse
import difflib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"


def words(t: str) -> list:
    return re.findall(r"\w+", t.casefold())


def grams(w: list, n: int = 6) -> set:
    return {" ".join(w[i:i + n]) for i in range(len(w) - n + 1)}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", required=True)
    ap.add_argument("--ratio", type=float, default=0.7)
    a = ap.parse_args()
    ev = [words(json.loads(line)["text"])
          for line in open(DATA / "eval" / "expanded.jsonl")]
    evg = set().union(*(grams(w) for w in ev))
    for run in a.runs.split(","):
        drop, n, best = [], 0, []
        for line in open(DATA / "commands" / run / "sentences.jsonl"):
            r = json.loads(line)
            w = words(r["text"])
            n += 1
            sm = difflib.SequenceMatcher(None, b=w)
            top = 0.0
            for e in ev:
                sm.set_seq1(e)
                if sm.real_quick_ratio() > top and sm.quick_ratio() > top:
                    top = max(top, sm.ratio())
            best.append(top)
            if top >= a.ratio or grams(w) & evg:
                drop.append(r["id"])
        (DATA / "commands" / run / "leak_drop.json").write_text(
            json.dumps(drop))
        best.sort()
        print(f"{run}: {n} sentences, dropped {len(drop)}; closest ratio "
              f"median {best[len(best) // 2]:.2f}, max {best[-1]:.2f}")


if __name__ == "__main__":
    main()
