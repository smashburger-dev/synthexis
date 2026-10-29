# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy"]
# ///
"""param-sense v1: parameter class and direction for unknown plugins.

  uv run tools/mapping/train_paramsense.py [--name ps-v1] [--epochs 12]

Labels: data/mapping/labels/{label-prio-v1,label-vst3-prio-v1,
label-bulk-v1,label-vst3-bulk-v1}/labels.jsonl (priority first; one row
per plugin and parameter name). Rows per (plugin, class) are capped at
--cap so rule-labelled MIDI CC slots do not swamp the rest. Value
strings come from the harvest (samples nearest 0/0.2/0.5/0.8/1).

Features per DESIGN.md: one hashed table 2^14 x 48 shared by three mean
bags. name: char 3-5 grams of "<name>" plus words; context: plugin-name
words plus the 64 most frequent words of the plugin's parameter names;
values: char 3-grams (dropped with p=0.5 in training). Hash = FNV-1a
32-bit over UTF-8 of "field|gram" mod 2^14.

Split: 20 % of vendors held out (test); 15 % of vendors (val)
pick the epoch. Test is scored on the runtime-only branch (no values)
and with values. Writes data/mapping/train/runs/<name>/{metrics.json,
weights.f16.bin, meta.json, known_plugins.json}.
"""

import argparse
import hashlib
import json
import random
import re
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
LABEL_RUNS = ["label-prio-v1", "label-vst3-prio-v1", "label-bulk-v1",
              "label-vst3-bulk-v1"]
PRIORITY = LABEL_RUNS[:2]
BUCKETS, DIM = 1 << 14, 48
VALUE_AT = (0.0, 0.2, 0.5, 0.8, 1.0)
REPORT = ["unsafe", "ctl.switch", "ctl.select", "system"]
RE_WORD = re.compile(r"[^\W_]+")


def fnv1a(s: str) -> int:
    h = 0x811C9DC5
    for b in s.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


_cache: dict = {}


def bucket(field: str, gram: str) -> int:
    k = field + "|" + gram
    v = _cache.get(k)
    if v is None:
        v = _cache[k] = fnv1a(k) % BUCKETS
    return v


def words(s: str) -> list:
    return RE_WORD.findall(s.casefold())


def name_feats(name: str) -> list:
    s = "<" + name.casefold() + ">"
    grams = {s[i:i + n] for n in (3, 4, 5) for i in range(len(s) - n + 1)}
    return sorted({bucket("n", g) for g in grams}
                  | {bucket("n", "w:" + w) for w in words(name)})


def ctx_feats(plugin: str, names: list) -> list:
    c = Counter(w for n in names for w in set(words(n)))
    top = [w for w, _ in c.most_common(64)]
    return sorted({bucket("c", "p:" + w) for w in words(plugin)}
                  | {bucket("c", w) for w in top})


def value_feats(vals: list) -> list:
    out = set()
    for v in vals:
        s = "<" + v.casefold() + ">"
        out |= {bucket("v", s[i:i + 3]) for i in range(len(s) - 2)}
    return sorted(out)


def vendor_split(vendor: str) -> str:
    h = int(hashlib.sha1(f"ps:{vendor}".encode()).hexdigest()[:8], 16) % 100
    return "test" if h < 20 else "val" if h < 35 else "train"


def load_values(keys: set) -> dict:
    """(plugin_id, index) -> value strings nearest VALUE_AT."""
    out = {}
    for h in ("au", "vst3"):
        with (DATA / "harvest" / h / "params.jsonl").open() as f:
            for line in f:
                r = json.loads(line)
                k = (r["plugin_id"], r["index"])
                if k not in keys:
                    continue
                smp = [(x, s) for x, s in r.get("samples") or [] if s]
                if not smp:
                    continue
                lo, hi = min(x for x, _ in smp), max(x for x, _ in smp)
                span = (hi - lo) or 1.0
                vals = []
                for t in VALUE_AT:
                    x, s = min(smp, key=lambda p: abs((p[0] - lo) / span - t))
                    vals.append(s)
                out[k] = vals
    return out


class ParamSense(nn.Module):
    def __init__(self, n_cls: int, hidden=96, drop=0.2):
        super().__init__()
        self.emb = nn.EmbeddingBag(BUCKETS, DIM, mode="mean")
        self.hid = nn.Linear(3 * DIM, hidden)
        self.cls = nn.Linear(hidden, n_cls)
        self.dir = nn.Linear(hidden, 2)
        self.drop = nn.Dropout(drop)

    def forward(self, bags):
        z = torch.cat([self.emb(ids, offs) for ids, offs in bags], -1)
        h = self.drop(F.gelu(self.hid(z)))
        return self.cls(h), self.dir(h)


def to_bag(lists: list):
    offs = np.cumsum([0] + [len(l) for l in lists[:-1]])
    ids = np.concatenate([np.asarray(l, dtype=np.int64) for l in lists]) \
        if any(lists) else np.zeros(0, dtype=np.int64)
    return torch.from_numpy(ids), torch.from_numpy(offs.astype(np.int64))


def batches(rows, idx, use_values: bool, value_drop: float, rng):
    nf = [rows[i]["nf"] for i in idx]
    cf = [rows[i]["cf"] for i in idx]
    vf = [rows[i]["vf"] if use_values and rng.random() >= value_drop
          else [] for i in idx]
    return [to_bag(nf), to_bag(cf), to_bag(vf)]


def f1s(y: list, p: list, classes: list) -> dict:
    out = {}
    for c in classes:
        tp = sum(1 for a, b in zip(y, p) if a == c and b == c)
        fp_ = sum(1 for a, b in zip(y, p) if a != c and b == c)
        fn = sum(1 for a, b in zip(y, p) if a == c and b != c)
        out[c] = 2 * tp / (2 * tp + fp_ + fn) if tp else 0.0
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="ps-v1")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--cap", type=int, default=60)
    ap.add_argument("--bs", type=int, default=256)
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args()
    rng = random.Random(a.seed)
    torch.manual_seed(a.seed)
    torch.set_num_threads(8)
    out = DATA / "train" / "runs" / a.name
    out.mkdir(parents=True, exist_ok=True)
    t0 = time.time()

    seen, rows, names_by_plugin, known = set(), [], defaultdict(list), {}
    for run in LABEL_RUNS:
        for line in (DATA / "labels" / run / "labels.jsonl").open():
            r = json.loads(line)
            names_by_plugin[r["plugin_id"]].append(r["name"])
            k = (r["plugin_id"], r["name"])
            if k in seen:
                continue
            seen.add(k)
            rows.append({"plugin_id": r["plugin_id"], "index": r["index"],
                         "name": r["name"], "class": r["class"],
                         "d": r.get("d"), "src": r.get("source")})
            if run in PRIORITY and r.get("source") != "rule":
                plug = r["plugin_id"].split("/", 1)[-1].casefold().strip()
                known[f"{plug}\t{r['name'].casefold().strip()}"] = [
                    r["class"], r.get("d"), r.get("lo"), r.get("hi")]
    n_all = len(rows)
    rng.shuffle(rows)
    per = Counter()
    capped = []
    for r in rows:
        k = (r["plugin_id"], r["class"])
        if per[k] < a.cap:
            per[k] += 1
            capped.append(r)
    rows = capped
    values = load_values({(r["plugin_id"], r["index"]) for r in rows})
    ctx = {p: ctx_feats(p.split("/", 1)[-1], ns)
           for p, ns in names_by_plugin.items()}
    for r in rows:
        r["nf"] = name_feats(r["name"])
        r["cf"] = ctx[r["plugin_id"]]
        r["vf"] = value_feats(values.get((r["plugin_id"], r["index"]), []))
        r["split"] = vendor_split(r["plugin_id"].split("/", 1)[0])
    classes = sorted({r["class"] for r in rows})
    cid = {c: i for i, c in enumerate(classes)}
    split = {s: [i for i, r in enumerate(rows) if r["split"] == s]
             for s in ("train", "val", "test")}
    freq = Counter(rows[i]["class"] for i in split["train"])
    w = torch.tensor([1 / max(1, freq[c]) ** 0.5 for c in classes])
    w = w / w.mean()
    print(f"rows {n_all} -> capped {len(rows)}; split "
          f"{ {k: len(v) for k, v in split.items()} }; values for "
          f"{sum(1 for r in rows if r['vf'])}; prep {time.time() - t0:.0f}s",
          flush=True)

    model = ParamSense(len(classes))
    n_params = sum(p.numel() for p in model.parameters())
    opt = torch.optim.AdamW(model.parameters(), lr=a.lr, weight_decay=0.01)
    ycls = torch.tensor([cid[r["class"]] for r in rows])
    ydir = torch.tensor([0 if r["d"] == -1 else 1 if r["d"] == 1 else -100
                         for r in rows])

    def predict(idx, use_values):
        model.eval()
        ps, pd = [], []
        with torch.no_grad():
            for k in range(0, len(idx), 2048):
                part = idx[k:k + 2048]
                lc, ld = model(batches(rows, part, use_values, 0.0, rng))
                ps += lc.argmax(-1).tolist()
                pd += ld.argmax(-1).tolist()
        return ps, pd

    def score(idx, use_values):
        ps, pd = predict(idx, use_values)
        y = [cid[rows[i]["class"]] for i in idx]
        present = sorted(set(y))
        f = f1s(y, ps, present)
        dm = [(ydir[i].item(), p) for i, p in zip(idx, pd)
              if ydir[i].item() != -100]
        return {"n": len(idx),
                "top1": round(sum(a == b for a, b in zip(y, ps)) / len(y), 4),
                "macro_f1": round(sum(f.values()) / len(f), 4),
                "f1": {c: round(f.get(cid[c], 0.0), 4) for c in REPORT
                       if c in cid},
                "dir_acc": round(sum(a == b for a, b in dm) / max(1, len(dm)),
                                 4)}

    best, best_state, best_ep = -1, None, 0
    for ep in range(a.epochs):
        model.train()
        idx = split["train"][:]
        rng.shuffle(idx)
        tot = 0.0
        for k in range(0, len(idx), a.bs):
            part = idx[k:k + a.bs]
            lc, ld = model(batches(rows, part, True, 0.5, rng))
            loss = F.cross_entropy(lc, ycls[part], weight=w)
            if (ydir[part] != -100).any():      # all-ignored CE is NaN
                loss = loss + 0.3 * F.cross_entropy(ld, ydir[part],
                                                    ignore_index=-100)
            opt.zero_grad()
            loss.backward()
            opt.step()
            tot += loss.item() * len(part)
        v = score(split["val"], False)
        print(f"epoch {ep + 1} loss {tot / len(idx):.4f} val top1 "
              f"{v['top1']} macro_f1 {v['macro_f1']}", flush=True)
        if v["macro_f1"] > best:
            best, best_ep = v["macro_f1"], ep + 1
            best_state = {k: t.clone() for k, t in model.state_dict().items()}
    model.load_state_dict(best_state)

    metrics = {"name": a.name, "args": vars(a), "params": n_params,
               "rows_all": n_all, "rows_capped": len(rows),
               "split": {k: len(v) for k, v in split.items()},
               "vendors": {s: len({rows[i]["plugin_id"].split("/")[0]
                                   for i in v}) for s, v in split.items()},
               "best_epoch": best_ep,
               "test_runtime_only": score(split["test"], False),
               "test_with_values": score(split["test"], True),
               "val_runtime_only": score(split["val"], False),
               "minutes": round((time.time() - t0) / 60, 1)}
    blob, tensors, off = bytearray(), [], 0
    for k, t in best_state.items():
        b = t.to(torch.float16).numpy().tobytes()
        tensors.append({"name": k, "shape": list(t.shape), "dtype": "f16",
                        "offset": off, "nbytes": len(b)})
        blob += b
        off += len(b)
    (out / "weights.f16.bin").write_bytes(bytes(blob))
    (out / "meta.json").write_text(json.dumps({
        "classes": classes, "buckets": BUCKETS, "dim": DIM,
        "hash": "fnv1a32(utf8(field + '|' + gram)) % buckets",
        "fields": {"n": "char 3-5 grams of '<' + casefold(name) + '>' and "
                        "'w:' + word", "c": "'p:' + plugin-name word and "
                        "top-64 words of the plugin's parameter names",
                   "v": "char 3-grams of '<' + casefold(value) + '>'"},
        "words": "regex [^\\W_]+ over casefold", "bag": "mean",
        "layers": "concat(n,c,v) -> hid(144->96) gelu -> cls(44), dir(2: "
                  "-1,+1)", "tensors": tensors}, indent=1))
    (out / "known_plugins.json").write_text(json.dumps(
        {"key": "casefold(plugin name)\\tcasefold(param name)",
         "value": ["class", "d", "lo", "hi"], "rows": known},
        ensure_ascii=False))
    metrics["known_plugin_rows"] = len(known)
    metrics["weights_bytes"] = len(blob)
    (out / "metrics.json").write_text(json.dumps(metrics, indent=1))
    print(json.dumps({k: metrics[k] for k in
                      ("test_runtime_only", "test_with_values", "best_epoch",
                       "params", "minutes")}))


if __name__ == "__main__":
    main()
