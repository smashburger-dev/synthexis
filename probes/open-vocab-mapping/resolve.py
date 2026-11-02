# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy"]
# ///
"""Open-vocabulary resolver prototype: bind the parser's target span to a
real parameter of the named plugin by name similarity, with the parser's
taxonomy class only as a tie-breaker.

  uv run probes/open-vocab-mapping/resolve.py "fist closes the Feedback on Timeless"
  uv run probes/open-vocab-mapping/resolve.py --eval [--n 20] [--jsonl out]

--eval builds template sentences from real parameter names of six plugins
(ShaperBox 3, Portal, Thermal, ValhallaDelay, Timeless 3, Galaxy Tape Echo),
none of which name appears in the parser's training data as a parameter,
and compares three resolvers on the same parses:
  class: params of the plugin labelled with the parser's class (current design)
  span:  name similarity between the BIO target span and the param names
  both:  span + 0.15 x class agreement
"""

import argparse
import json
import math
import random
import re
import sys
from collections import defaultdict
from pathlib import Path

import sentencepiece as spm
import torch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import frame_parser as fp  # noqa: E402

MAP = ROOT / "data" / "mapping"
LABELS = ["label-prio-v1", "label-vst3-prio-v1", "label-bulk-v1",
          "label-vst3-bulk-v1"]
FILLER = {"the", "a", "an", "my", "knob", "slider", "parameter", "param",
          "die", "der", "das", "den", "dem", "mehr", "more", "regler",
          "knopf", "wert", "of", "von"}
SKIP = re.compile(r"^(x \(reserved\)|reserved\d*|midi cc|cc ?\d+)", re.I)

PLUGINS = {  # spoken name -> plugin_id
    "ShaperBox": "Cableguys/ShaperBox 3",
    "Portal": "Output/Portal",
    "Thermal": "Output/Thermal",
    "Valhalla Delay": "Valhalla DSP, LLC/ValhallaDelay",
    "Timeless": "FabFilter/Timeless 3",
    "Galaxy Tape Echo": "Universal Audio (UADx)/UADx Galaxy Tape Echo",
}
SOURCES = {"en": ["I open my hand", "I make a fist", "I pinch",
                  "I raise my left hand", "I tilt my head", "I open my mouth"],
           "de": ["ich die Hand öffne", "ich eine Faust mache",
                  "ich pinche", "ich die linke Hand hebe",
                  "ich den Kopf neige", "ich den Mund öffne"]}
TEMPLATES = {"en": ["turn up {p} in {pl} when {s}",
                    "when {s}, {p} on {pl} should go up",
                    "map it so that {p} in {pl} follows when {s}"],
             "de": ["mehr {p} in {pl}, wenn {s}",
                    "wenn {s}, soll {p} in {pl} hochgehen",
                    "{p} auf {pl} soll aufgehen, wenn {s}"]}


def words(s: str) -> list:
    s = re.sub(r"([a-z])([A-Z])", r"\1 \2", s)
    return [w for w in re.split(r"[^0-9a-zäöüß]+", s.casefold()) if w]


def grams(ws: list) -> set:
    t = " " + " ".join(ws) + " "
    return {t[i:i + 3] for i in range(len(t) - 2)}


def sim(span: str, name: str) -> float:
    """Dice over char 3-grams, averaged with the share of span words that
    start a word of the name (so 'feedback' finds 'SLOT 1 DELAY FEEDBACK').
    Digits are identifiers: each digit the span says must be in the name."""
    a = [w for w in words(span) if w not in FILLER]
    b = words(name)
    if not a or not b:
        return 0.0
    ga, gb = grams(a), grams(b)
    dice = 2 * len(ga & gb) / (len(ga) + len(gb))
    cover = sum(any(x.startswith(w) or w.startswith(x) for x in b
                    if min(len(x), len(w)) >= 3) for w in a) / len(a)
    nums = {w for w in a if w.isdigit()}
    digit = 0.2 if nums and nums <= set(b) else -0.2 if nums else 0.0
    return 0.5 * dice + 0.5 * cover + digit


def find_plugin(parse: dict) -> tuple:
    """Parser spans first; if they match no plugin of the project, look for
    a project plugin name in the sentence itself (subword splits such as
    'The' + 'rmal' break the span, not the sentence)."""
    cands = [parse["plugin_ptr"]] + parse["plugin"]
    best = max(((sim(c, sp), pid) for c in cands if c
                for sp, pid in PLUGINS.items()), default=(0.0, None))
    if best[0] >= 0.6:
        return best
    text = " " + " ".join(words(parse["text"])) + " "
    for sp, pid in PLUGINS.items():
        if " " + " ".join(words(sp)) + " " in text:
            return 1.0, pid
    return best


def inventory() -> tuple:
    """-> ({plugin_id: [(index, name)]}, {(plugin_id, name): class})."""
    params, cls = defaultdict(dict), {}
    for fmt in ("au", "vst3"):
        for line in open(MAP / "harvest" / fmt / "params.jsonl"):
            p = json.loads(line)
            if p["plugin_id"] in PLUGINS.values() and \
                    not SKIP.match(p["name"]):
                params[p["plugin_id"]].setdefault(p["name"], p["index"])
    for run in LABELS:
        f = MAP / "labels" / run / "labels.jsonl"
        if not f.exists():
            continue
        for line in open(f):
            r = json.loads(line)
            if r["plugin_id"] in PLUGINS.values():
                cls.setdefault((r["plugin_id"], r["name"]), r["class"])
    return ({k: sorted((i, n) for n, i in v.items())
             for k, v in params.items()}, cls)


class Parser:
    def __init__(self, run: str):
        d = MAP / "train" / "runs" / run
        meta = json.loads((d / "meta.json").read_text())
        self.vocab, c = meta["frame_vocab"], meta["config"]
        self.sp = spm.SentencePieceProcessor(
            model_file=str(ROOT / meta["tokenizer"]))
        self.m = fp.Parser(c["vocab"], c["fv"], c["d"], c["heads"], c["ff"],
                           c["enc_layers"], c["dec_layers"], 0.0)
        self.m.load_state_dict(torch.load(d / "model.pt", map_location="cpu"))
        self.m.eval()
        self.g = fp.Grammar(self.vocab)

    def __call__(self, texts: list) -> list:
        ss = []
        for t in texts:
            ids, offs = fp.tokenize(self.sp, t)
            ss.append({"text": t, "ids": ids[:fp.MAX_SRC - 1],
                       "offs": offs[:fp.MAX_SRC - 1]})
        out = []
        with torch.no_grad():
            for s, (seq, lp) in zip(ss, fp.greedy(self.m, self.g, ss,
                                                  torch.device("cpu"))):
                frame = fp.items(seq, s, self.vocab)
                mem, _ = self.m.encode(torch.tensor([s["ids"]]))
                spans = self.bio_spans(s, self.m.bio(mem)[0].argmax(-1))
                cl = next((t.split(":", 2)[2] for t in frame
                           if t.startswith("T:param:")), None)
                ptr = next((frame[i + 1][1:] for i, t in enumerate(frame[:-1])
                            if t == "PLUGIN" and frame[i + 1][0] == "="), "")
                out.append({"text": s["text"], "conf": math.exp(lp),
                            "frame": frame, "cls": cl, "plugin_ptr": ptr,
                            "tgt": spans.get("TGT_COVER", []),
                            "plugin": spans.get("PLUGIN", [])})
        return out

    @staticmethod
    def bio_spans(s: dict, tags) -> dict:
        """-> {tag: [text]}, plus 'TGT_COVER': the text from the first to the
        last TGT piece, because long names come back in fragments
        ('pitch' ... 'size') and lose the words or digits between them."""
        spans, cur = defaultdict(list), None
        for i, tg in enumerate(tags.tolist()):
            name = fp.BIO[tg]
            if i == 0 or name == "O" or s["offs"][i] is None:
                cur = None
                continue
            a, b = s["offs"][i]
            if name.startswith("B-") or cur is None or cur[0] != name[2:]:
                cur = [name[2:], a, b]
                spans[cur[0]].append(cur)
            else:
                cur[2] = b
        out = {k: [s["text"][a:b].strip() for _, a, b in v]
               for k, v in spans.items()}
        if spans["TGT"]:
            a = min(x[1] for x in spans["TGT"])
            b = max(x[2] for x in spans["TGT"])
            cover = s["text"][a:b]
            for _, pa, pb in spans["PLUGIN"]:
                if a <= pa and pb <= b:
                    cover = cover.replace(s["text"][pa:pb], " ")
            out["TGT_COVER"] = [cover.strip()]
        return out


def resolve(parse: dict, params: dict, cls: dict, mode: str) -> tuple:
    """-> (plugin_id or None, [(score, name)] best first)."""
    best = find_plugin(parse)
    if best[0] < 0.3:
        return None, []
    pid = best[1]
    span = " ".join(parse["tgt"])
    ranked = []
    for idx, name in params.get(pid, []):
        agree = float(cls.get((pid, name)) == parse["cls"])
        if mode == "class":
            score = agree - idx * 1e-6        # first labelled param wins
        elif mode == "span":
            score = sim(span, name)
        else:
            score = sim(span, name) + 0.15 * agree
        ranked.append((score, name))
    ranked.sort(key=lambda r: -r[0])
    return pid, ranked


def spoken(name: str, rng: random.Random) -> str:
    """How a user would say a parameter name: lower case, slot prefix often
    dropped ('SLOT 1 DELAY FEEDBACK' -> 'delay feedback')."""
    n = " ".join(words(name))
    if rng.random() < 0.5:
        n = re.sub(r"^(slot|stage) \d ", "", n)
    return n


def build_eval(params: dict, n: int, seed: int) -> list:
    rng, rows = random.Random(seed), []
    for pl, pid in PLUGINS.items():
        names = [nm for _, nm in params.get(pid, [])]
        for nm in rng.sample(names, min(n, len(names))):
            lang = rng.choice(["en", "de"])
            p = spoken(nm, rng)
            text = rng.choice(TEMPLATES[lang]).format(
                p=p, pl=pl, s=rng.choice(SOURCES[lang]))
            gold = {x for x in names if " ".join(words(x)).endswith(p)}
            rows.append({"text": text, "plugin_id": pid, "said": p,
                         "gold": sorted(gold), "lang": lang})
    return rows


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v3-c50-s0")
    ap.add_argument("--eval", action="store_true")
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--jsonl", default=None)
    ap.add_argument("text", nargs="*")
    a = ap.parse_args()
    params, cls = inventory()
    parser = Parser(a.run)

    if not a.eval:
        for p in parser(a.text):
            pid, ranked = resolve(p, params, cls, "both")
            print(f"{p['conf']:.2f} {p['text']}\n     frame {' '.join(p['frame'])}"
                  f"\n     span {p['tgt']} plugin {pid}")
            for s, nm in ranked[:3]:
                print(f"       {s:.2f} {nm} [{cls.get((pid, nm))}]")
        return

    rows = build_eval(params, a.n, a.seed)
    parses = parser([r["text"] for r in rows])
    hits = defaultdict(lambda: [0, 0, 0])      # mode -> [plugin, top1, top3]
    span_found = 0
    for r, p in zip(rows, parses):
        span_found += any(sim(r["said"], t) > 0.5 for t in p["tgt"])
        r["parse"] = {k: p[k] for k in ("conf", "frame", "tgt", "plugin")}
        for mode in ("class", "span", "both"):
            pid, ranked = resolve(p, params, cls, mode)
            top = [nm for _, nm in ranked[:3]]
            h = hits[mode]
            h[0] += pid == r["plugin_id"]
            h[1] += bool(top[:1]) and top[0] in r["gold"]
            h[2] += bool(set(top) & set(r["gold"]))
            r[mode] = top
    n = len(rows)
    print(f"{n} sentences, {len(PLUGINS)} plugins, target span found "
          f"{span_found}/{n}")
    for mode, (pl, t1, t3) in hits.items():
        print(f"  {mode:5s} plugin {pl / n:.3f}  top-1 {t1 / n:.3f}  "
              f"top-3 {t3 / n:.3f}")
    if a.jsonl:
        with open(a.jsonl, "w") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
