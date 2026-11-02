# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml", "httpx>=0.27"]
# ///
"""Ceiling check: Muse Spark Open parses the real-sentence set directly.

  SYNTHEXIS_BASETEN_CAP=70 uv run probes/real-eval/spark_parse.py \
      --run sp-real --max-usd 1 [--split calibration] [--effort high]
  uv run probes/real-eval/score.py --preds data/mapping/eval/preds-sp-real.jsonl

Spark gets the frame language the parser is trained on: the token rules,
the ontology sources and families, and few-shot examples drawn from the
synthetic training set (ds-v6p train_cat, never from the real set). It
writes one frame per sentence; tokens outside the frame language are
dropped. The replies are a measurement only and never go into training.
"""

import argparse
import json
import random
import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import spark  # noqa: E402
from project_labels import unswitched  # noqa: E402

EVAL = ROOT / "data" / "mapping" / "eval"
TRAIN = ROOT / "data" / "mapping" / "train" / "ds-v6p" / "train_cat.jsonl"
JOB_SIZE = 25
RE_LINE = re.compile(r"^(\S+)\s+(.+?)\s*$")
TOKEN = re.compile(r"^(<route>|</route>|INV|VALUE|BIP|REF|TO|FROM|NOTE|CHORD|"
                   r"TIME|TRACK(:ref)?|PLUGIN|PNAME|=\S+|"
                   r"[A-Za-z_]+:\S+)$")

STRICT = re.compile(r"toggl|switch|bypass|schalt|on and off|an und aus|"
                    r"turn\w* (on|off)|mute", re.I)

RULES = """You parse commands for synthexis, an audio plugin for FL Studio that turns camera-tracked hands, face, head, objects and movements into automation and MIDI. A producer types one sentence in English or German (often casual, with typos and Denglisch). Write the edit it asks for as a frame: a flat list of tokens.

## Frame language

- First token: the intent, one of I:map, I:unmap, I:clear, I:modify, I:replace, I:role, I:preset, I:scale, I:undo, I:query, I:none.
- Then one `<route> ... </route>` per target the sentence controls. Inside a route:
  - sources `S:<source id>` (list below), each optionally followed by `side:left`, `side:right` or `side:both` when the sentence names a hand or side.
  - a held hand shape as source: `S:hand.shape` followed by `shape:<name>` (fist, open, point, peace, three, four, rock, shaka, thumbs_up, l_shape, pinky_up, thumb_index_middle) or `shape:m<5 bits>` for any other set of extended fingers, bits in the order thumb index middle ring pinky (index and ring up = m01010).
  - one single finger shown, bent or extended: `S:hand.finger.<thumb|index|middle|ring|pinky>`; tilting a finger sideways: `S:hand.finger.<f>.tilt`.
  - gates `G:<id>`: a condition that must hold while another source moves. A pinch held while the hand drags, raises or turns is `G:hand.pinch` plus the moving source (`S:hand.y`, `S:hand.x`, `S:hand.roll`). `S:hand.pinch` is only right when the pinch amount itself drives the target. A shape held as condition is `G:hand.shape shape:<name>`.
  - `INV` when the target moves against the named source state ("opening my hand closes the filter": S:hand.openness INV; "the more I close my fist, the less reverb": S:hand.fist INV).
  - targets: `T:param:<meaning>` for a plugin parameter (cutoff, reverb mix, wavetable position, any knob), `T:toggle:<family>` for switching an effect or plugin on/off or bypassing it, `T:midi.note:`, `T:midi.chord:`, `T:midi.pattern:<name>`, `T:midi.prop:<prop>`, `T:midi.cc:<n>`, `T:macro:<n>`, `T:xy:` for an XY pad.
  - `mode:step` when the target jumps through values in steps, `mode:ramp` when it glides over time after a trigger, `mode:momentary` when it holds only while the source holds, `mode:toggle` when each trigger flips it.
  - `VALUE` once for every explicit value the sentence assigns (each finger or position mapped to its own value adds one VALUE).
  - names may follow as `TRACK =<word>`, `PLUGIN =<word>`, `PNAME =<word>`; they are optional.
- A movement described in both directions ("when I raise it, more; when I lower it, less again") is one route, not two and not an invert.
- Several clauses that drive different targets are several routes; a later clause that only says "it" or "es" keeps the target it points back to.
- Polarity: every source id names the state that reads 1. hand.openness is an open hand, hand.fist a closed fist.

## Output

One line per sentence, nothing else: `<id> <tokens separated by spaces>`. Write your best reading for every sentence; do not explain.

## Sources
"""


def render(frame: list, text: str) -> str:
    out = []
    for t in frame:
        if isinstance(t, list):
            out.append("=" + re.sub(r"\s+", "_", text[t[0]:t[1]]))
        else:
            out.append(t)
    return " ".join(out)


def examples(seed: int = 0) -> list:
    """One English and one German training sentence per feature."""
    rows = [json.loads(line) for line in open(TRAIN)]
    rng = random.Random(seed)
    rng.shuffle(rows)
    feats = {
        "pinch gate": lambda f: "G:hand.pinch" in f,
        "mask": lambda f: any(t.startswith("shape:m") for t in f),
        "values": lambda f: "VALUE" in f,
        "step": lambda f: "mode:step" in f,
        "ramp": lambda f: "mode:ramp" in f,
        "toggle": lambda f: any(t.startswith("T:toggle") for t in f),
        "invert": lambda f: "INV" in f,
        "chord": lambda f: any(t.startswith("T:midi.chord") for t in f),
        "pattern": lambda f: any(t.startswith("T:midi.pattern") for t in f),
        "finger": lambda f: any(re.fullmatch(r"S:hand\.finger\.\w+", t)
                                for t in f),
        "stage 5, several routes": lambda f: f.count("<route>") >= 2,
    }
    picked, seen = [], set()
    for name, test in feats.items():
        long = name.startswith("stage 5")
        for lang in ("en", "de"):
            for r in rows:
                f = [t for t in r["frame"] if isinstance(t, str)]
                if r["lang"] == lang and r["text"] not in seen and \
                        len(r["text"]) <= (400 if long else 200) and \
                        (not long or r["id"].startswith("j0")) and \
                        not unswitched(f, r["text"]) and test(f) and \
                        (STRICT.search(r["text"]) or not any(
                            t.startswith("T:toggle") for t in f)):
                    picked.append(r)
                    seen.add(r["text"])
                    break
    return picked


def system_prompt() -> str:
    onto = yaml.safe_load((ROOT / "tools" / "mapping" / "ontology.yaml")
                          .read_text())
    srcs = "\n".join(f"{s['id']} | {s['desc']}" for s in onto["sources"])
    fams = ", ".join(onto.get("families", []))
    ex = "\n".join(f"{r['text']}\n-> {render(r['frame'], r['text'])}\n"
                   for r in examples())
    return (f"{RULES}{srcs}\n\nEffect families for T:toggle: {fams}\n\n"
            f"## Examples\n\n{ex}")


def clean(tokens: list) -> list:
    return [t for t in tokens if TOKEN.match(t)]


def make_parse(ids_by_job: dict):
    def parse(job: spark.Job, text: str):
        out = {}
        for line in text.splitlines():
            m = RE_LINE.match(line.strip().strip("`"))
            if m and m.group(1) in ids_by_job[job.id]:
                out[m.group(1)] = clean(m.group(2).split())
        missing = [i for i in ids_by_job[job.id] if i not in out]
        if len(missing) > len(ids_by_job[job.id]) // 5:
            raise spark.ParseError(f"{len(missing)} sentences missing")
        return out
    return parse


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="sp-real")
    ap.add_argument("--max-usd", type=float, default=1.0)
    ap.add_argument("--split", default=None)
    ap.add_argument("--effort", default="high")
    a = ap.parse_args()
    rows = [json.loads(line) for line in open(EVAL / "expanded.jsonl")]
    todo = [r for r in rows if a.split in (None, r["split"])]
    sysmsg = system_prompt()
    jobs, ids_by_job = [], {}
    for k in range(0, len(todo), JOB_SIZE):
        chunk = todo[k:k + JOB_SIZE]
        jid = f"{a.run}-{k // JOB_SIZE:02d}"
        ids_by_job[jid] = {r["id"] for r in chunk}
        user = "\n".join(f"{r['id']} {r['text']}" for r in chunk)
        jobs.append(spark.Job(id=jid, messages=[
            {"role": "system", "content": sysmsg},
            {"role": "user", "content": user}],
            est_out_tokens=60 * len(chunk) + 4000))
    parsed = spark.run(a.run, jobs, make_parse(ids_by_job),
                       max_usd=a.max_usd, workers=4, pilot=1,
                       reasoning_effort=a.effort)
    frames = {i: f for p in parsed.values() for i, f in p.items()}
    with open(EVAL / f"preds-{a.run}.jsonl", "w") as w:
        for r in rows:
            w.write(json.dumps({"text": r["text"],
                                "frame": frames.get(r["id"], [])},
                               ensure_ascii=False) + "\n")
    print(f"{len(frames)}/{len(todo)} sentences parsed")


if __name__ == "__main__":
    main()
