# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy"]
# ///
"""Decode-time changes tested without retraining, scored with route_edit.

  uv run probes/parser-errors/decode_tests.py [--run p-v3-c50-s0]

ss:   replace ß by ss before tokenizing (sentences with ß in all dev sets)
snap: widen pointer spans to whole words ('500 m' -> '500 milliseconds')
"""

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "probes" / "open-vocab-mapping"))
import resolve as rv  # noqa: E402
import frame_parser as fp  # noqa: E402

SPLITS = ["dev", "dev_s2", "dev_s3", "challenge"]
ORIG = fp.span_text


def word(c: int) -> bool:          # ASCII letter/digit or any UTF-8 byte
    return c >= 0x80 or chr(c).isalnum()


def snapped(text: str, offs: list, i: int, j: int) -> str:
    b = text.encode()
    s, e = offs[i][0], offs[j][1]
    while s > 0 and word(b[s - 1]) and word(b[s]):
        s -= 1
    while e < len(b) and word(b[e]) and word(b[e - 1]):
        e += 1
    return b[s:e].decode("utf-8", "ignore")


def ok(frames: list, rows: list, fix=lambda t: t) -> list:
    return [fp.compare([fix(t) for t in f["frame"]],
                       [fix(t) for t in r["gold"]])["route_edit"]
            for f, r in zip(frames, rows)]


def report(name: str, a: list, b: list) -> None:
    n = len(a)
    print(f"  {name}: n {n} route_edit {sum(a) / n:.3f} -> {sum(b) / n:.3f} "
          f"(+{sum(y and not x for x, y in zip(a, b))} / "
          f"-{sum(x and not y for x, y in zip(a, b))})")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v3-c50-s0")
    a = ap.parse_args()
    parser = rv.Parser(a.run)
    rows = {sp: [json.loads(line) for line in open(
        ROOT / "data" / "mapping" / "train" / "runs" / a.run
        / f"{sp}_preds.jsonl")] for sp in SPLITS}

    print("ss (sentences with ß, all sets)")
    sz = [r for sp in SPLITS for r in rows[sp] if "ß" in r["text"]]
    ss = lambda t: t.replace("ß", "ss")          # noqa: E731
    report("ß as is vs ss", ok(parser([r["text"] for r in sz]), sz, ss),
           ok(parser([ss(r["text"]) for r in sz]), sz, ss))

    print("snap (dev sets only; challenge is not used to choose)")
    for sp in SPLITS[:3]:
        texts = [r["text"] for r in rows[sp]]
        fp.span_text = ORIG
        base = ok(parser(texts), rows[sp])
        fp.span_text = snapped
        report(sp, base, ok(parser(texts), rows[sp]))
    fp.span_text = ORIG


if __name__ == "__main__":
    main()
