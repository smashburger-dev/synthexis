# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Independent design reviews of the mapping assistant by Muse Spark Open.

  uv run probes/mapping-pipeline/review.py --round 1 --max-usd 0.5

Sends DESIGN.md, the ontology, the edit sampler config and the two
generation prompts to Spark Open three times with different review
briefs (data and labels, architecture and evaluation, coverage gaps).
Writes the answers to probes/mapping-pipeline/review-r<round>-<brief>.md.
"""

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import spark  # noqa: E402

FILES = [
    "probes/mapping-pipeline/DESIGN.md",
    "tools/mapping/ontology.yaml",
    "tools/mapping/edits.yaml",
    "tools/mapping/prompts/label.md",
    "tools/mapping/prompts/commands.md",
]

SYSTEM = """You are a senior reviewer for on-device language understanding and ML data pipelines. You review the design of a mapping assistant inside a music plugin. You did not write it and you owe its authors nothing; find what will break. Be concrete: name the file and the line or block, say what goes wrong in practice, and give the smallest change that fixes it. Do not praise. Do not restate the design.

Context: synthexis is an audio plugin for FL Studio on Apple Silicon. Camera-tracked hands, face and objects drive plugin parameters. A producer types one sentence (English or German) and the plugin turns it into route edits fully offline. Muse Spark Open (a large open-weight LLM on Baseten) is used only at build time: it labels harvested plugin parameters and writes synthetic training sentences from structured edits. The shipped models must be small (parser up to about 10M parameters, low RAM) and precise. Deadline: 6 days, one developer plus agents, about 90 USD of LLM credit left.

The design documents follow, each starting with a line `=== <path> ===`."""

BRIEFS = {
    "data": """Review the synthetic data and labelling methodology (edits.yaml, commands.md, label.md, the build-time section of DESIGN.md). Answer:
1. Which properties of the generated sentences will make a parser trained on them fail on real producers' sentences? Rank by expected damage.
2. Where can labels be wrong by construction (edit says X, sentence says Y) and the validator will not notice? How to catch it cheaply.
3. Is the sampling distribution sensible for a first dataset of about 40k sentences? What would you change, with numbers.
4. For the parameter labels: what will the labeller get systematically wrong given the prompt and the taxonomy, and how to fix the prompt or taxonomy.
Give at most 12 findings, each: severity (high/medium/low), where, problem, fix.""",
    "arch": """Review the runtime architecture and evaluation (DESIGN.md runtime, model sizes, evaluation sections; ontology.yaml as the output space). Answer:
1. Is an encoder plus constrained span-pointer frame decoder the right choice for this output space under about 10M parameters and low RAM, compared with a BIO tagger plus linker, a tagger plus rules, or a small pretrained encoder? Argue with the concrete sentence types the ontology implies.
2. Which frame grammar decisions (route brackets, scope, modifiers, polarity pairs like hand.fist / hand.openness) will make learning or decoding harder than needed?
3. What evaluation would most likely mislead the developers into shipping something that fails in the demo?
4. Resolver and param-sense: where will a correct parse still produce the wrong route in a real FL project?
Give at most 12 findings, each: severity, where, problem, fix.""",
    "gaps": """Find coverage gaps. Write 40 realistic sentences (20 English, 20 German) that producers would type into this plugin while making music, which the ontology and edit grammar as written CANNOT represent or would represent wrongly. Vary them: relative changes ("a bit more"), references to earlier routes ("same for the bass", "that one"), conditions ("only while I hold a fist"), timing ("over 4 bars"), multiple hands, MIDI, presets, corrections, negation, questions. After the list, group the gaps into at most 8 concrete ontology or grammar changes, each with which sentences it fixes. Do not propose changes that need an online model at runtime.""",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--round", type=int, default=1)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--briefs", default="data,arch,gaps")
    args = ap.parse_args()

    docs = "\n\n".join(
        f"=== {p} ===\n{(ROOT / p).read_text()}" for p in FILES)
    system = SYSTEM + "\n\n" + docs
    jobs = [spark.Job(id=b, messages=[
        {"role": "system", "content": system},
        {"role": "user", "content": BRIEFS[b]}], est_out_tokens=6000)
        for b in args.briefs.split(",")]
    res = spark.run(f"review-r{args.round}", jobs, lambda j, t: t,
                    max_usd=args.max_usd, workers=3, pilot=0,
                    reasoning_effort="high")
    for b, text in res.items():
        out = ROOT / "probes" / "mapping-pipeline" / f"review-r{args.round}-{b}.md"
        out.write_text(text)
        print("wrote", out.relative_to(ROOT))


if __name__ == "__main__":
    main()
