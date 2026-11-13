# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy"]
# ///
"""Export what the plugin needs to give every picked plugin parameter
classes (plugin/synthexis/src/sense.rs):

  uv run tools/mapping/export_paramsense.py [--run ps-v2] [--out DIR ...]

Writes into each --out (default data/mapping/completion and
plugin/assistant/completion):
  known.tsv             labelled classes of every mapped plugin, grouped:
                        a line "#<casefold plugin name>" starts a plugin,
                        then "<casefold parameter name>\\t<class>" lines.
                        Same label choice as export_completion.py (earlier
                        label runs win per plugin id, final class first);
                        placeholder names (MIDI CC slots) are left out.
                        Hand-labelled stock plugins come from stock.py:
                        FL's effects (tools/mapping/fl/stock_labels.tsv)
                        and Live's devices as "ableton:<class_name>"
                        (tools/mapping/ableton/stock_labels.tsv).
  paramsense/meta.json, paramsense/weights.f16.bin
                        the param-sense run, copied unchanged

and plugin/synthexis/tests/fixtures/paramsense_parity.json: the run's
runtime-only predictions (no value strings) for every parameter of a few
harvested plugins, for the Rust parity test. The context words are taken in
a fixed order (first appearance over the names, each name's words in
order), the order sense.rs uses; training took them from a set, so ties at
the 64-word cut fell in hash order.
"""

import argparse
import json
import re
import shutil
from collections import Counter
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

import stock
from fl_param_counts import placeholder  # is_placeholder of fl.rs

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
OUTS = [DATA / "completion", ROOT / "plugin" / "assistant" / "completion"]
FIXTURE = ROOT / "plugin" / "synthexis" / "tests" / "fixtures" / "paramsense_parity.json"
LABEL_RUNS = ["label-vst3-prio-v1", "label-vst3-bulk-v1", "label-prio-v1", "label-bulk-v1"]
PARITY_PLUGINS = ["Xfer Records/Serum 2", "Output/Portal", "FabFilter/Pro-Q 3",
                  "Valhalla DSP, LLC/ValhallaDelay", "Arturia/Pigments", "u-he/Diva",
                  "Native Instruments/Massive", "Surge Synth Team/Surge XT"]


# Features as train_paramsense.py builds them, with the bucket count of the
# exported run (ps-v1 2^14, ps-v2 2^16) instead of a constant.
RE_WORD = re.compile(r"[^\W_]+")
BUCKETS = [1 << 14]


def fnv1a(s: str) -> int:
    h = 0x811C9DC5
    for b in s.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def bucket(field: str, gram: str) -> int:
    return fnv1a(field + "|" + gram) % BUCKETS[0]


def words(s: str) -> list:
    return RE_WORD.findall(s.casefold())


def name_feats(name: str) -> list:
    s = "<" + name.casefold() + ">"
    grams = {s[i:i + n] for n in (3, 4, 5) for i in range(len(s) - n + 1)}
    return sorted({bucket("n", g) for g in grams} | {bucket("n", "w:" + w) for w in words(name)})


def ctx_feats(plugin: str, names: list) -> list:
    c: Counter = Counter()
    for n in names:
        c.update(list(dict.fromkeys(words(n))))  # each word once per name, in order
    top = [w for w, _ in c.most_common(64)]
    return sorted({bucket("c", "p:" + w) for w in words(plugin)} | {bucket("c", w) for w in top})


def known_table(data: Path) -> dict:
    owner, out = {}, {}
    owner_names = set()
    for run in LABEL_RUNS:
        for line in open(data / "labels" / run / "labels.jsonl", encoding="utf-8"):
            r = json.loads(line)
            pid = r["plugin_id"]
            if owner.setdefault(pid, run) != run or placeholder(r["name"]):
                continue
            cls = r["final"][0] if "final" in r else r["class"]
            plug = pid.split("/", 1)[-1].casefold().strip()
            owner_names.add(plug)
            out.setdefault(plug, {})[r["name"].casefold().strip()] = cls
    # stock plugins labelled by hand (stock.py): FL reports some names in
    # its own case ("Fruity delay 2"); Live devices are "ableton:<class_name>"
    for plug, name, cls in stock.rows():
        if plug.casefold() in owner_names:
            continue
        out.setdefault(plug.casefold().strip(), {})[name.casefold().strip()] = cls
    return out


def load_run(run: Path):
    meta = json.loads((run / "meta.json").read_text())
    blob = (run / "weights.f16.bin").read_bytes()
    t = {}
    for x in meta["tensors"]:
        a = np.frombuffer(blob, dtype=np.float16, count=int(np.prod(x["shape"])), offset=x["offset"])
        t[x["name"]] = torch.from_numpy(a.astype(np.float32).reshape(x["shape"]))
    return meta, t


def predict(t: dict, classes: list, plugin: str, names: list) -> list:
    cf = ctx_feats(plugin, names)
    emb = t["emb.weight"]
    mean = lambda ids: emb[ids].mean(0) if ids else torch.zeros(emb.shape[1])  # noqa: E731
    c = mean(cf)
    out = []
    for n in names:
        z = torch.cat([mean(name_feats(n)), c, torch.zeros(emb.shape[1])])
        h = F.gelu(t["hid.weight"] @ z + t["hid.bias"])
        out.append(classes[int((t["cls.weight"] @ h + t["cls.bias"]).argmax())])
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="ps-v2")
    ap.add_argument("--data", default=str(DATA))
    ap.add_argument("--out", action="append", help="completion dir; repeatable")
    a = ap.parse_args()
    data = Path(a.data)
    run = data / "train" / "runs" / a.run
    meta, t = load_run(run)
    BUCKETS[0] = meta["buckets"]
    assert t["emb.weight"].shape[0] == meta["buckets"], "meta.json buckets differ from emb.weight"

    known = known_table(data)
    lines = []
    for plug in sorted(known):
        lines.append("#" + plug)
        lines += [f"{n}\t{c}" for n, c in sorted(known[plug].items()) if "\t" not in n and "\n" not in n]
    for out in [Path(o) for o in a.out] if a.out else OUTS:
        (out / "paramsense").mkdir(parents=True, exist_ok=True)
        (out / "known.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")
        for f in ("meta.json", "weights.f16.bin"):
            shutil.copyfile(run / f, out / "paramsense" / f)
        print(f"{len(known)} plugins, {len(lines) - len(known)} labelled parameters, run {a.run} -> {out}")

    names_of: dict = {}
    for line in open(data / "harvest" / "au" / "params.jsonl", encoding="utf-8"):
        r = json.loads(line)
        if r["plugin_id"] in PARITY_PLUGINS and r["name"].strip() and not placeholder(r["name"]):
            names_of.setdefault(r["plugin_id"], []).append(r["name"])
    cases = []
    for pid in PARITY_PLUGINS:
        if pid not in names_of:
            continue
        plugin, names = pid.split("/", 1)[-1], names_of[pid][:600]
        cases.append({"plugin": plugin, "names": names, "classes": predict(t, meta["classes"], plugin, names)})
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps({"run": a.run, "cases": cases}, ensure_ascii=False))
    print(f"parity fixture: {len(cases)} plugins, {sum(len(c['names']) for c in cases)} names -> {FIXTURE}")


if __name__ == "__main__":
    main()
