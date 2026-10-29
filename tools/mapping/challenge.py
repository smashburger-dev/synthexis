# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx", "pyyaml"]
# ///
"""Challenge set: verbalise held-out edits with the challenge prompt.

  uv run tools/mapping/edits.py --run edits-ch99 --n 300 --seed 99
  uv run tools/mapping/challenge.py --run ch-s1 --edits-run edits-ch99
      --max-usd 1.5 [--reasoning-effort medium] [--dry-run]

Same jobs, validator and negative check as commands.py; only the system
prompt differs (prompts/challenge.md). Edit ids get a "c" prefix so they
never collide with training edit ids. Never used for training.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import commands  # noqa: E402
import spark  # noqa: E402

ROOT = commands.ROOT
HERE = commands.HERE


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--edits-run", required=True)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--workers", type=int, default=12)
    ap.add_argument("--reasoning-effort", default="medium")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    commands.ONTO = yaml.safe_load((HERE / "ontology.yaml").read_text())
    cfg = yaml.safe_load((HERE / "edits.yaml").read_text())
    edits_path = (ROOT / "data" / "mapping" / "commands"
                  / args.edits_run / "edits.jsonl")
    edits = [json.loads(l) for l in edits_path.read_text().splitlines()
             if l.strip()]
    for e in edits:
        e["id"] = "c" + e["id"]
    sys_prompt = (HERE / "prompts" / "challenge.md").read_text()
    jobs = commands.build_jobs(edits, cfg["edits_per_job"], sys_prompt)
    est = sum(spark._estimate_usd(j) for j in jobs)
    print(f"{len(edits)} edits -> {len(jobs)} jobs, estimate ${est:.3f}")

    out_dir = ROOT / "data" / "mapping" / "commands" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    commands.load_negative(ROOT / "data" / "mapping" / "glossary" / "gloss-v1")
    if args.dry_run:
        (out_dir / "sample_jobs.txt").write_text(
            "SYSTEM PROMPT:\n" + sys_prompt + "\n\n"
            + jobs[0].messages[1]["content"] + "\n")
        print(f"dry-run: wrote {out_dir / 'sample_jobs.txt'}")
        return

    parsed = spark.run(
        args.run, jobs, commands.make_parse(), max_usd=args.max_usd,
        workers=args.workers, temperature=0.9,
        reasoning_effort=args.reasoning_effort)

    n_sent, drops = 0, Counter()
    with (out_dir / "sentences.jsonl").open("w") as sf:
        for res in parsed.values():
            for s in res["sentences"]:
                sf.write(json.dumps(s, ensure_ascii=False) + "\n")
                n_sent += 1
            for d in res["drops"]:
                drops[d["reason"]] += 1
    requested = sum(len(e["variants"]) for e in edits)
    summary = {"variants_requested": requested, "sentences": n_sent,
               "valid_rate": round(n_sent / requested, 3),
               "drops_by_reason": dict(drops.most_common())}
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
