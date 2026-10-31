# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "sentencepiece"]
# ///
"""Error analysis of a parser run on the dev sets (no decoding, reads the
stored *_preds.jsonl).

  uv run probes/parser-errors/analyze.py [--run p-v3-c50-s0]

1. fields:      first failing field per language, accuracy per register
2. fragments:   sentences with a letter word cut into >= 3 tokenizer pieces,
                difference adjusted for length, language and digits
3. unsupported: labels in the generated commands that the sentence text does
                not express (object never named, invert neither marked nor
                cued, momentary mode without a cue)
4. supported:   route_edit when both sides drop what the text does not say
                and merge duplicate encodings, per issue and all together
"""

import argparse
import collections
import json
import re
import sys
from pathlib import Path

import sentencepiece as spm

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import frame_parser as fp  # noqa: E402

DATA = ROOT / "data" / "mapping"
SPLITS = ["dev", "dev_s2", "dev_s3", "challenge"]
INV = re.compile(
    r"invert|inverse|revers|backward|opposite|flip|upside|the less|less |"
    r"lower|down|decreas|reduc|clos|shut|quieter|darker|drier|fewer|smaller|"
    r"shorter|softer|umgekehrt|invers|rückwärts|gegen|weniger|leiser|kleiner|"
    r"schließ|runter|dunkler|kürzer|tiefer|niedriger|zu ", re.I)
MOM = re.compile(r"hold|while|as long as|momentar|only when|press|keep|"
                 r"solange|halt|während|gedrückt|nur wenn", re.I)
PITCH = {"T:param:osc.pitch": "T:param:pitch",
         "T:param:fx.pitch": "T:param:pitch"}


def load(run: str, splits: list) -> list:
    rows = []
    for sp in splits:
        gold = {}
        for line in open(DATA / "train" / "ds-v3" / f"{sp}.jsonl"):
            g = json.loads(line)
            gold[g["id"]] = g
        for line in open(DATA / "train" / "runs" / run / f"{sp}_preds.jsonl"):
            r = json.loads(line)
            g = gold[r["id"]]
            r.update(split=sp, lang=g["lang"], register=g["register"])
            rows.append(r)
    return rows


def objects() -> dict:
    gl = collections.defaultdict(set)
    for line in open(DATA / "glossary" / "gloss-v1" / "glossary.jsonl"):
        g = json.loads(line)
        if g["c"].startswith("object:"):
            gl[g["c"][7:]].add(g["p"].casefold())
    return gl


def fields(rows: list) -> None:
    print("\n1. first failing field")
    for lang in ("en", "de"):
        rs = [r for r in rows if r["lang"] == lang]
        c = collections.Counter(
            "ok" if r["route_edit"] else next(
                (f for f in ("intent", "sources", "targets", "invert", "scope")
                 if not r[f]), "modifiers/grouping") for r in rs)
        reg = collections.defaultdict(lambda: [0, 0])
        for r in rs:
            reg[r["register"]][0] += 1
            reg[r["register"]][1] += r["route_edit"]
        print(f"  {lang} n {len(rs)} route_edit {c['ok'] / len(rs):.3f}; "
              + ", ".join(f"{k} {v / len(rs):.3f}" for k, v in c.most_common()
                          if k != "ok"))
        print("     by register: " + ", ".join(
            f"{k} {v[1] / v[0]:.2f} ({v[0]})" for k, v in
            sorted(reg.items(), key=lambda x: x[1][1] / x[1][0])))


def fragments(rows: list) -> None:
    sp = spm.SentencePieceProcessor(
        model_file=str(DATA / "train" / "tok-v1" / "sp.model"))
    st = collections.defaultdict(lambda: [0, 0, 0, 0])
    words = collections.Counter()
    for r in rows:
        ws = re.findall(r"[A-Za-zäöüÄÖÜß]+", r["text"])
        fw = [w for w in ws if len(sp.encode(w, out_type=str)) >= 3]
        words.update(w.casefold() for w in fw)
        n = len(ws)
        a = st[("short" if n <= 9 else "mid" if n <= 15 else "long",
                r["lang"], bool(re.search(r"\d", r["text"])))]
        k = 0 if fw else 2
        a[k] += 1
        a[k + 1] += r["route_edit"]
    tot = [sum(a[i] for a in st.values()) for i in range(4)]
    num = den = 0.0
    for a in st.values():
        if a[0] and a[2]:
            w = a[0] * a[2] / (a[0] + a[2])
            num += w * (a[1] / a[0] - a[3] / a[2])
            den += w
    print(f"\n2. fragmented letter words: n {tot[0]} route_edit "
          f"{tot[1] / tot[0]:.3f}, others n {tot[2]} {tot[3] / tot[2]:.3f}; "
          f"adjusted difference {num / den:+.3f}")
    print("   most frequent:", ", ".join(w for w, _ in words.most_common(20)))
    print("   ß pieces:", sp.encode("schließe", out_type=str),
          "vs", sp.encode("schliesse", out_type=str))


def unsupported(obj: dict) -> None:
    c = collections.Counter()
    for run in ("cmd-s1", "cmd-s2", "cmd-s3"):
        for line in open(DATA / "commands" / run / "sentences.jsonl"):
            r = json.loads(line)
            marked = {s["slot"] for s in r["spans"]}
            low, lang = r["text"].casefold(), r["lang"]
            for sid, s in r["edit"]["slots"].items():
                if s["kind"] == "invert":
                    c[lang, "invert"] += 1
                    if sid not in marked and not INV.search(r["text"]):
                        c[lang, "invert: no mark, no cue"] += 1
                if s["kind"] == "source" and s.get("object"):
                    c[lang, "object"] += 1
                    names = obj.get(s["object"], set()) | {s["object"]}
                    if not any(p in low for p in names):
                        c[lang, "object: never named"] += 1
                if s.get("target") == "toggle" and s.get("mode") == "momentary":
                    c[lang, "momentary"] += 1
                    if not MOM.search(r["text"]):
                        c[lang, "momentary: no cue"] += 1
    print("\n3. labels the generated sentence does not express")
    for lang in ("en", "de"):
        for a, b in (("invert", "invert: no mark, no cue"),
                     ("object", "object: never named"),
                     ("momentary", "momentary: no cue")):
            print(f"  {lang} {b}: {c[lang, b]} of {c[lang, a]} "
                  f"({c[lang, b] / max(1, c[lang, a]):.0%})")


def canon(toks: list, text: str, parts: set, obj: dict) -> list:
    out, low = [], text.casefold()
    for t in toks:
        if "dup" in parts and t == "side:either":
            continue
        if "dup" in parts and t == "G:hand.fist":
            out += ["G:hand.shape", "shape:fist"]
            continue
        if "obj" in parts and t.startswith("object:") and not any(
                p in low for p in obj.get(t[7:], set()) | {t[7:]}):
            t = "object:any"
        if "mode" in parts and t.startswith("mode:") and not MOM.search(text):
            continue
        if "inv" in parts and t == "INV" and not INV.search(text):
            continue
        if "pitch" in parts:
            t = PITCH.get(t, t)
        out.append(t)
    return out


def supported(rows: list, obj: dict) -> None:
    print("\n4. route_edit on text-supported labels (dev sets, change vs base)")
    for lang in ("en", "de"):
        rs = [r for r in rows if r["lang"] == lang]
        base = sum(r["route_edit"] for r in rs) / len(rs)
        out = []
        for parts in ({"dup"}, {"obj"}, {"mode"}, {"inv"}, {"pitch"},
                      {"dup", "obj", "mode", "inv", "pitch"}):
            k = sum(fp.compare(canon(r["pred"], r["text"], parts, obj),
                               canon(r["gold"], r["text"], parts, obj))
                    ["route_edit"] for r in rs) / len(rs)
            out.append(f"{'all' if len(parts) > 1 else next(iter(parts))} "
                       f"{k - base:+.3f}")
        print(f"  {lang} n {len(rs)} base {base:.3f}: " + ", ".join(out))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v3-c50-s0")
    a = ap.parse_args()
    dev = load(a.run, SPLITS[:3])
    obj = objects()
    fields(dev)
    fragments(load(a.run, SPLITS))
    unsupported(obj)
    supported(dev, obj)


if __name__ == "__main__":
    main()
