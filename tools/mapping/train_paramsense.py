# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy"]
# ///
"""param-sense: parameter class and direction for unknown plugins.

  uv run tools/mapping/train_paramsense.py --cv 5 --name base        # CV only
  uv run tools/mapping/train_paramsense.py --cv 5 --name x --against base
  uv run tools/mapping/train_paramsense.py --name ps-v2 --members 5 \
      --buckets-log 16 --epochs 18 --export --cv-from m5b16

Labels: data/mapping/labels/{label-prio-v1,label-vst3-prio-v1,
label-bulk-v1,label-vst3-bulk-v1}/labels.jsonl (priority first; one row
per plugin and parameter name). Rows are ranked per (plugin, class) after
a fixed shuffle; training keeps rank < --cap so rule-labelled MIDI CC
slots do not swamp the rest, scoring always uses rank < 60 so runs with
different caps see the same rows. Value strings come from the harvest
(samples nearest 0/0.2/0.5/0.8/1).

Features per DESIGN.md: one hashed table [buckets, dim] shared by three
mean bags. name: char 3-5 grams of "<name>" plus words; context:
plugin-name words plus the 64 most frequent words of the plugin's
parameter names; values: char 3-grams (dropped with p=--value-drop in
training). Hash = FNV-1a 32-bit over UTF-8 of "field|gram" mod buckets,
deduplicated per bag after the mod.

Selection (probes/paramsense-v2/RESULT.md): 5 vendor-grouped folds over
all vendors, scored every epoch on the held-out fold, runtime-only
branch. A config's epoch count is the epoch with the best mean top-1.
verdict() accepts a candidate against the current best only if the
paired per-fold deltas clear the bar on 4 of 5 folds.

--members K trains K independent nets and averages their logits; export
folds them into one net of the same format (emb dims, hidden units and
classifier columns concatenated block-diagonally, classifier / K).

--export trains on all vendors for --epochs, scores the old 19-vendor
test split with a second net trained on the other vendors, and writes
data/mapping/train/runs/<name>/{metrics.json, weights.f16.bin,
meta.json, known_plugins.json}.
"""

import argparse
import hashlib
import json
import random
import re
import statistics
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
CV_DIR = DATA / "train" / "runs" / "ps-cv"
LABEL_RUNS = ["label-prio-v1", "label-vst3-prio-v1", "label-bulk-v1",
              "label-vst3-bulk-v1"]
PRIORITY = LABEL_RUNS[:2]
EVAL_CAP = 60
VALUE_AT = (0.0, 0.2, 0.5, 0.8, 1.0)
REPORT = ["unsafe", "ctl.switch", "ctl.select", "system"]
# scored branches: no value strings / 5 sampled ones / the default only
BRANCHES = [("runtime", None), ("values", "vf"), ("default", "df")]
RE_WORD = re.compile(r"[^\W_]+")
VENDOR_GROUP = {"Universal Audio (UADx)": "Universal Audio",
                "Universal Audio Sphere": "Universal Audio"}


def fnv1a(s: str) -> int:
    h = 0x811C9DC5
    for b in s.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


_cache: dict = {}


def hashed(field: str, gram: str) -> int:
    """Full 32-bit hash; the bucket is this mod buckets."""
    k = field + "|" + gram
    v = _cache.get(k)
    if v is None:
        v = _cache[k] = fnv1a(k)
    return v


def words(s: str) -> list:
    return RE_WORD.findall(s.casefold())


def name_feats(name: str) -> list:
    s = "<" + name.casefold() + ">"
    grams = {s[i:i + n] for n in (3, 4, 5) for i in range(len(s) - n + 1)}
    return sorted({hashed("n", g) for g in grams}
                  | {hashed("n", "w:" + w) for w in words(name)})


def ctx_feats(plugin: str, names: list) -> list:
    c = Counter(w for n in names for w in set(words(n)))
    top = [w for w, _ in c.most_common(64)]
    return sorted({hashed("c", "p:" + w) for w in words(plugin)}
                  | {hashed("c", w) for w in top})


def value_feats(vals: list) -> list:
    out = set()
    for v in vals:
        s = "<" + v.casefold() + ">"
        out |= {hashed("v", s[i:i + 3]) for i in range(len(s) - 2)}
    return sorted(out)


def vendor_of(plugin_id: str) -> str:
    v = plugin_id.split("/", 1)[0]
    return VENDOR_GROUP.get(v, v)


def vendor_split(vendor: str) -> str:
    """ps-v1 split, kept for the old-test comparison."""
    h = int(hashlib.sha1(f"ps:{vendor}".encode()).hexdigest()[:8], 16) % 100
    return "test" if h < 20 else "val" if h < 35 else "train"


def vendor_folds(rows: list, k: int) -> dict:
    """Greedy balance: biggest vendor group to the lightest fold."""
    size = Counter(r["vendor"] for r in rows if r["eval"])
    load, fold = [0] * k, {}
    for v, n in sorted(size.items(), key=lambda x: (-x[1], x[0])):
        f = load.index(min(load))
        fold[v] = f
        load[f] += n
    return fold


def load_values(keys: set) -> tuple:
    """(plugin_id, index) -> value strings nearest VALUE_AT, and -> the
    default value string (what FL shows for a freshly loaded plugin)."""
    out, default = {}, {}
    for h in ("au", "vst3"):
        with (DATA / "harvest" / h / "params.jsonl").open() as f:
            for line in f:
                r = json.loads(line)
                k = (r["plugin_id"], r["index"])
                if k not in keys:
                    continue
                if r.get("default_string"):
                    default[k] = [r["default_string"]]
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
    return out, default


def load_rows(max_cap: int):
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
    random.Random(0).shuffle(rows)
    per = Counter()
    kept = []
    for r in rows:
        k = (r["plugin_id"], r["class"])
        r["rank"] = per[k]
        per[k] += 1
        if r["rank"] < max(max_cap, EVAL_CAP):
            kept.append(r)
    rows = kept
    values, default = load_values({(r["plugin_id"], r["index"])
                                   for r in rows})
    ctx = {p: ctx_feats(p.split("/", 1)[-1], ns)
           for p, ns in names_by_plugin.items()}
    for r in rows:
        k = (r["plugin_id"], r["index"])
        r["nh"] = name_feats(r["name"])
        r["ch"] = ctx[r["plugin_id"]]
        r["vh"] = value_feats(values.get(k, []))
        r["dh"] = value_feats(default.get(k, []))
        r["vendor"] = vendor_of(r["plugin_id"])
        r["split"] = vendor_split(r["plugin_id"].split("/", 1)[0])
        r["eval"] = r["rank"] < EVAL_CAP
    return rows, n_all, known


def bucketize(rows: list, buckets: int) -> None:
    for r in rows:
        for src, dst in (("nh", "nf"), ("ch", "cf"), ("vh", "vf"),
                         ("dh", "df")):
            r[dst] = np.unique(np.asarray(r[src], dtype=np.int64) % buckets)


class ParamSense(nn.Module):
    def __init__(self, n_cls: int, buckets: int, dim: int, hidden: int,
                 drop=0.2):
        super().__init__()
        self.emb = nn.EmbeddingBag(buckets, dim, mode="mean")
        self.hid = nn.Linear(3 * dim, hidden)
        self.cls = nn.Linear(hidden, n_cls)
        self.dir = nn.Linear(hidden, 2)
        self.drop = nn.Dropout(drop)

    def forward(self, bags):
        z = torch.cat([self.emb(ids, offs) for ids, offs in bags], -1)
        h = self.drop(F.gelu(self.hid(z)))
        return self.cls(h), self.dir(h)


def merge(states: list) -> dict:
    """K member state dicts -> one state dict whose logits are the mean."""
    k = len(states)
    dim = states[0]["emb.weight"].shape[1]
    hid = states[0]["hid.bias"].shape[0]
    emb = torch.cat([s["emb.weight"] for s in states], 1)
    hw = torch.zeros(k * hid, 3 * k * dim)
    for m, s in enumerate(states):
        for b in range(3):          # bag b of member m sits at b*K*dim+m*dim
            hw[m * hid:(m + 1) * hid,
               b * k * dim + m * dim:b * k * dim + (m + 1) * dim] = \
                s["hid.weight"][:, b * dim:(b + 1) * dim]
    out = {"emb.weight": emb, "hid.weight": hw,
           "hid.bias": torch.cat([s["hid.bias"] for s in states])}
    for head in ("cls", "dir"):
        out[f"{head}.weight"] = torch.cat(
            [s[f"{head}.weight"] for s in states], 1) / k
        out[f"{head}.bias"] = sum(s[f"{head}.bias"] for s in states) / k
    return out


def to_bag(lists: list):
    offs = np.cumsum([0] + [len(l) for l in lists[:-1]])
    ids = np.concatenate(lists) if lists else np.zeros(0, dtype=np.int64)
    return torch.from_numpy(ids.astype(np.int64)), \
        torch.from_numpy(offs.astype(np.int64))


def batches(rows, idx, vkey, value_drop: float, rng):
    """vkey: "vf" (5 sampled value strings), "df" (default string) or None."""
    none = np.zeros(0, dtype=np.int64)
    nf = [rows[i]["nf"] for i in idx]
    cf = [rows[i]["cf"] for i in idx]
    vf = [rows[i][vkey] if vkey and rng.random() >= value_drop
          else none for i in idx]
    return [to_bag(nf), to_bag(cf), to_bag(vf)]


def f1s(y: list, p: list, classes: list) -> dict:
    out = {}
    for c in classes:
        tp = sum(1 for a, b in zip(y, p) if a == c and b == c)
        fp_ = sum(1 for a, b in zip(y, p) if a != c and b == c)
        fn = sum(1 for a, b in zip(y, p) if a == c and b != c)
        out[c] = 2 * tp / (2 * tp + fp_ + fn) if tp else 0.0
    return out


class Trainer:
    """K members trained in lockstep on one row set; scores mean logits."""

    def __init__(self, a, rows, classes, train_idx):
        self.a, self.rows, self.classes = a, rows, classes
        self.cid = {c: i for i, c in enumerate(classes)}
        self.train_idx = train_idx
        self.rng = random.Random(a.seed)
        torch.manual_seed(a.seed)
        self.models = [ParamSense(len(classes), 1 << a.buckets_log, a.dim,
                                  a.hidden) for _ in range(a.members)]
        self.opts = [torch.optim.AdamW(m.parameters(), lr=a.lr,
                                       weight_decay=0.01)
                     for m in self.models]
        self.ycls = torch.tensor([self.cid[r["class"]] for r in rows])
        self.ydir = torch.tensor([0 if r["d"] == -1 else 1 if r["d"] == 1
                                  else -100 for r in rows])
        freq = Counter(rows[i]["class"] for i in train_idx)
        w = torch.tensor([1 / max(1, freq[c]) ** a.cw for c in classes])
        if "unsafe" in self.cid:
            w[self.cid["unsafe"]] *= a.unsafe_w
        self.w = w / w.mean()

    def epoch(self) -> float:
        a, tot = self.a, 0.0
        for model, opt in zip(self.models, self.opts):
            model.train()
            idx = self.train_idx[:]
            self.rng.shuffle(idx)
            for k in range(0, len(idx), a.bs):
                part = idx[k:k + a.bs]
                lc, ld = model(batches(self.rows, part, "vf", a.value_drop,
                                       self.rng))
                loss = F.cross_entropy(lc, self.ycls[part], weight=self.w,
                                       label_smoothing=a.smooth)
                if (self.ydir[part] != -100).any():  # all-ignored CE is NaN
                    loss = loss + 0.3 * F.cross_entropy(
                        ld, self.ydir[part], ignore_index=-100)
                opt.zero_grad()
                loss.backward()
                opt.step()
                tot += loss.item() * len(part)
        return tot / len(self.train_idx) / len(self.models)

    def predict(self, idx, vkey):
        for m in self.models:
            m.eval()
        lc_all, ld_all = [], []
        with torch.no_grad():
            for k in range(0, len(idx), 4096):
                part = idx[k:k + 4096]
                bags = batches(self.rows, part, vkey, 0.0, self.rng)
                outs = [m(bags) for m in self.models]
                lc_all.append(sum(o[0] for o in outs) / len(outs))
                ld_all.append(sum(o[1] for o in outs) / len(outs))
        return torch.cat(lc_all).argmax(-1).tolist(), \
            torch.cat(ld_all).argmax(-1).tolist()

    def score(self, idx, vkey):
        ps, pd = self.predict(idx, vkey)
        y = [self.cid[self.rows[i]["class"]] for i in idx]
        f = f1s(y, ps, sorted(set(y)))
        dm = [(self.ydir[i].item(), p) for i, p in zip(idx, pd)
              if self.ydir[i].item() != -100]
        return {"n": len(idx),
                "top1": round(sum(a == b for a, b in zip(y, ps)) / len(y), 4),
                "macro_f1": round(sum(f.values()) / len(f), 4),
                "f1": {c: round(f.get(self.cid[c], 0.0), 4) for c in REPORT
                       if c in self.cid},
                "dir_acc": round(sum(a == b for a, b in dm) / max(1, len(dm)),
                                 4)}

    def state(self) -> dict:
        sts = [{k: t.detach().clone() for k, t in m.state_dict().items()}
               for m in self.models]
        return sts[0] if len(sts) == 1 else merge(sts)


def train_idx_of(rows, keep, cap):
    return [i for i, r in enumerate(rows) if r["rank"] < cap and keep(r)]


def eval_idx_of(rows, keep):
    return [i for i, r in enumerate(rows) if r["eval"] and keep(r)]


def seed_mean(scores: list) -> dict:
    """Mean of score() dicts over seeds."""
    m = lambda xs: round(statistics.mean(xs), 4)
    return {"n": scores[0]["n"],
            "top1": m([s["top1"] for s in scores]),
            "macro_f1": m([s["macro_f1"] for s in scores]),
            "f1": {c: m([s["f1"][c] for s in scores])
                   for c in scores[0]["f1"]},
            "dir_acc": m([s["dir_acc"] for s in scores])}


def run_cv(a, rows, classes) -> dict:
    """Per fold and epoch, scores are the mean over --seeds."""
    fold = vendor_folds(rows, a.cv)
    seeds = [int(s) for s in a.seeds.split(",")]
    per_fold = []
    for f in range(a.cv):
        tr = train_idx_of(rows, lambda r: fold[r["vendor"]] != f, a.cap)
        te = eval_idx_of(rows, lambda r: fold[r["vendor"]] == f)
        runs = []
        for seed in seeds:
            a.seed = seed
            t = Trainer(a, rows, classes, tr)
            curve = []
            for ep in range(a.epochs):
                loss = t.epoch()
                curve.append({b: t.score(te, k) for b, k in BRANCHES})
                print(f"fold {f} seed {seed} ep {ep + 1} loss {loss:.4f} "
                      f"top1 {curve[-1]['runtime']['top1']} macro "
                      f"{curve[-1]['runtime']['macro_f1']}", flush=True)
            runs.append(curve)
        curve = [{"epoch": e + 1} | {b: seed_mean([c[e][b] for c in runs])
                                      for b, _ in BRANCHES}
                 for e in range(a.epochs)]
        per_fold.append({"fold": f, "train_rows": len(tr), "test_rows": len(te),
                         "vendors": sorted(v for v, x in fold.items()
                                           if x == f), "curve": curve})
    mean_top1 = [statistics.mean(pf["curve"][e]["runtime"]["top1"]
                                 for pf in per_fold) for e in range(a.epochs)]
    best = max(range(a.epochs), key=lambda e: mean_top1[e])
    summary = {"best_epoch": best + 1,
               "curve_mean_top1": [round(x, 4) for x in mean_top1]}
    for branch, _ in BRANCHES:
        at = [pf["curve"][best][branch] for pf in per_fold]
        summary[branch] = {
            "top1": [x["top1"] for x in at],
            "macro_f1": [x["macro_f1"] for x in at],
            "unsafe_f1": [x["f1"].get("unsafe", 0.0) for x in at],
            "mean_top1": round(statistics.mean(x["top1"] for x in at), 4),
            "mean_macro_f1": round(statistics.mean(x["macro_f1"]
                                                   for x in at), 4),
            "mean_unsafe_f1": round(statistics.mean(
                x["f1"].get("unsafe", 0.0) for x in at), 4)}
    return {"name": a.name, "args": vars(a), "summary": summary,
            "folds": per_fold}


def verdict(cand: dict, base: dict) -> dict:
    """The selection rule. Paired per-fold deltas on the runtime branch."""
    c, b = cand["summary"]["runtime"], base["summary"]["runtime"]
    dt = [x - y for x, y in zip(c["top1"], b["top1"])]
    dm = [x - y for x, y in zip(c["macro_f1"], b["macro_f1"])]
    mt, mm = statistics.mean(dt), statistics.mean(dm)
    win_top1 = mt >= 0.005 and sum(d > 0 for d in dt) >= 4 and mm >= -0.01
    win_macro = mm >= 0.01 and sum(d > 0 for d in dm) >= 4 and mt >= -0.005
    return {"against": base["name"], "d_top1": [round(d, 4) for d in dt],
            "d_macro_f1": [round(d, 4) for d in dm],
            "mean_d_top1": round(mt, 4), "mean_d_macro_f1": round(mm, 4),
            "accept": win_top1 or win_macro,
            "why": "top1" if win_top1 else "macro_f1" if win_macro else "-"}


def export(a, rows, classes, known, t0) -> None:
    out = DATA / "train" / "runs" / a.name
    out.mkdir(parents=True, exist_ok=True)
    # old-split comparison: train on the ps-v1 train+val vendors
    old = Trainer(a, rows, classes,
                  train_idx_of(rows, lambda r: r["split"] != "test", a.cap))
    for _ in range(a.epochs):
        old.epoch()
    te = eval_idx_of(rows, lambda r: r["split"] == "test")
    t = Trainer(a, rows, classes, train_idx_of(rows, lambda r: True, a.cap))
    for ep in range(a.epochs):
        print(f"full ep {ep + 1} loss {t.epoch():.4f}", flush=True)
    state = t.state()
    n_params = sum(v.numel() for v in state.values())
    metrics = {"name": a.name, "args": vars(a), "params": n_params,
               "rows_train": len(t.train_idx),
               "old_test_runtime_only": old.score(te, None),
               "old_test_with_values": old.score(te, "vf"),
               "old_test_default_value": old.score(te, "df"),
               "old_test_note": "net trained on the 44 non-test vendors of "
                                "the ps-v1 split for --epochs, scored on its "
                                "19 test vendors (ps-v1: 0.7276 / 0.5111)"}
    if a.cv_from:
        cv = json.loads((CV_DIR / f"{a.cv_from}.json").read_text())
        metrics["cv"] = {"run": a.cv_from, "summary": cv["summary"],
                         "folds": [{k: pf[k] for k in ("fold", "vendors",
                                                       "test_rows")}
                                   | {"at_best": pf["curve"][
                                       cv["summary"]["best_epoch"] - 1]}
                                   for pf in cv["folds"]]}
    blob, tensors, off = bytearray(), [], 0
    for k, v in state.items():
        b = v.to(torch.float16).numpy().tobytes()
        tensors.append({"name": k, "shape": list(v.shape), "dtype": "f16",
                        "offset": off, "nbytes": len(b)})
        blob += b
        off += len(b)
    buckets, dim = state["emb.weight"].shape
    hidden = state["hid.bias"].shape[0]
    (out / "weights.f16.bin").write_bytes(bytes(blob))
    (out / "meta.json").write_text(json.dumps({
        "classes": classes, "buckets": buckets, "dim": dim,
        "hash": "fnv1a32(utf8(field + '|' + gram)) % buckets",
        "fields": {"n": "char 3-5 grams of '<' + casefold(name) + '>' and "
                        "'w:' + word", "c": "'p:' + plugin-name word and "
                        "top-64 words of the plugin's parameter names",
                   "v": "char 3-grams of '<' + casefold(value) + '>'"},
        "words": "regex [^\\W_]+ over casefold", "bag": "mean",
        "layers": f"concat(n,c,v) -> hid({3 * dim}->{hidden}) gelu -> "
                  f"cls({len(classes)}), dir(2: -1,+1)",
        "tensors": tensors}, indent=1))
    (out / "known_plugins.json").write_text(json.dumps(
        {"key": "casefold(plugin name)\\tcasefold(param name)",
         "value": ["class", "d", "lo", "hi"], "rows": known},
        ensure_ascii=False))
    metrics["known_plugin_rows"] = len(known)
    metrics["weights_bytes"] = len(blob)
    metrics["minutes"] = round((time.time() - t0) / 60, 1)
    (out / "metrics.json").write_text(json.dumps(metrics, indent=1))
    print(json.dumps({k: metrics[k] for k in
                      ("old_test_runtime_only", "old_test_with_values",
                       "params", "minutes")}))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="ps-v2")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--cap", type=int, default=60)
    ap.add_argument("--bs", type=int, default=256)
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--buckets-log", type=int, default=14)
    ap.add_argument("--dim", type=int, default=48)
    ap.add_argument("--hidden", type=int, default=96)
    ap.add_argument("--cw", type=float, default=0.5,
                    help="class weight = 1 / freq ** cw")
    ap.add_argument("--unsafe-w", type=float, default=1.0)
    ap.add_argument("--value-drop", type=float, default=0.5)
    ap.add_argument("--smooth", type=float, default=0.0)
    ap.add_argument("--members", type=int, default=1)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--cv", type=int, default=0)
    ap.add_argument("--seeds", default="0,1", help="CV seeds, averaged")
    ap.add_argument("--against", help="CV run to apply verdict() against")
    ap.add_argument("--verdict", nargs=2, metavar=("CAND", "BASE"),
                    help="print verdict() for two finished CV runs and exit")
    ap.add_argument("--export", action="store_true")
    ap.add_argument("--cv-from", help="CV run whose numbers go in metrics")
    a = ap.parse_args()
    if a.verdict:
        c, b = (json.loads((CV_DIR / f"{n}.json").read_text())
                for n in a.verdict)
        print(json.dumps(verdict(c, b)))
        return
    torch.set_num_threads(a.threads)
    t0 = time.time()
    rows, n_all, known = load_rows(a.cap)
    bucketize(rows, 1 << a.buckets_log)
    classes = sorted({r["class"] for r in rows})
    print(f"rows {n_all} -> kept {len(rows)}; eval {sum(r['eval'] for r in rows)}"
          f"; prep {time.time() - t0:.0f}s", flush=True)
    if a.cv:
        res = run_cv(a, rows, classes)
        res["minutes"] = round((time.time() - t0) / 60, 1)
        if a.against:
            res["verdict"] = verdict(res, json.loads(
                (CV_DIR / f"{a.against}.json").read_text()))
        CV_DIR.mkdir(parents=True, exist_ok=True)
        (CV_DIR / f"{a.name}.json").write_text(json.dumps(res, indent=1))
        print(json.dumps({"summary": res["summary"],
                          "verdict": res.get("verdict")}))
    if a.export:
        export(a, rows, classes, known, t0)


if __name__ == "__main__":
    main()
