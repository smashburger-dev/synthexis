# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml", "httpx>=0.27"]
# ///
"""Which teacher labels best: Spark or DeepSeek V4.1 Flash, same prompt.

  SYNTHEXIS_BASETEN_CAP=70 uv run probes/real-eval/teacher_check.py --n 100

Rows come from the synthetic dev_s5 split (Noa's style, gold from the
edits), never from the real set: DeepSeek processes in China, so it only
sees synthetic text. Prompt and parsing are spark_parse.py's. Arms:
spark (reasoning high), ds-think (thinking enabled, effort high) and
ds-chat (thinking disabled, temperature 0). DeepSeek spend goes to
data/mapping/deepseek/ledger.jsonl; the key comes from DEEPSEEK_API_KEY
or the keychain item DEEPSEEK_API_KEY and is never printed.
"""

import argparse
import datetime as dt
import json
import os
import random
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import httpx

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import score as S  # noqa: E402
import spark_parse as P  # noqa: E402

ROOT = HERE.parents[1]
DEV = ROOT / "data" / "mapping" / "train" / "ds-v6p" / "dev_s5.jsonl"
DS_DIR = ROOT / "data" / "mapping" / "deepseek"
API = "https://api.deepseek.com/chat/completions"
MODEL = "deepseek-flash"
# USD per 1M tokens (cache hit, miss, output), off-peak; peak is double
# (Mon-Fri 01-04 and 06-10 UTC), from kleinhirn-k24/docs/K24_DATA_PLAN.md.
PRICE = (0.003, 0.15, 0.60)
PEAK_HOURS = ((1, 4), (6, 10))


def key() -> str:
    k = os.environ.get("DEEPSEEK_API_KEY") or subprocess.run(
        ["security", "find-generic-password", "-a", os.environ["USER"],
         "-s", "DEEPSEEK_API_KEY", "-w"],
        capture_output=True, text=True).stdout.strip()
    if not k:
        raise SystemExit("DEEPSEEK_API_KEY missing (env or keychain)")
    return k


def cost(usage: dict) -> float:
    now = dt.datetime.now(dt.timezone.utc)
    peak = now.weekday() < 5 and any(a <= now.hour < b for a, b in PEAK_HOURS)
    hit = usage.get("prompt_cache_hit_tokens", 0)
    miss = usage.get("prompt_tokens", 0) - hit
    usd = (hit * PRICE[0] + miss * PRICE[1]
           + usage.get("completion_tokens", 0) * PRICE[2]) / 1e6
    return usd * (2 if peak else 1)


def deepseek(messages: list, think: bool, k: str) -> tuple[str, float]:
    body = {"model": MODEL, "messages": messages, "max_tokens": 32000}
    if think:
        body |= {"thinking": {"type": "enabled"}, "reasoning_effort": "high"}
    else:
        body |= {"thinking": {"type": "disabled"}, "temperature": 0}
    for attempt in range(4):
        try:
            r = httpx.post(API, json=body, timeout=600,
                           headers={"Authorization": f"Bearer {k}"})
            r.raise_for_status()
            js = r.json()
            usd = cost(js.get("usage", {}))
            DS_DIR.mkdir(parents=True, exist_ok=True)
            with open(DS_DIR / "ledger.jsonl", "a") as w:
                w.write(json.dumps({"ts": time.time(), "run": "teacher-check",
                                    "think": think, "usage": js.get("usage"),
                                    "cost_usd": usd}) + "\n")
            return js["choices"][0]["message"]["content"], usd
        except (httpx.HTTPError, KeyError) as e:
            err = type(e).__name__
            time.sleep(5 * (attempt + 1))
    raise SystemExit(f"DeepSeek failed: {err}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=100)
    ap.add_argument("--arms", default="spark,ds-think,ds-chat")
    a = ap.parse_args()
    rows = [json.loads(line) for line in open(DEV)]
    rows = [r for r in rows if r["frame"][0] == "I:map"]
    rows = random.Random(0).sample(rows, a.n)
    sysmsg = P.system_prompt()
    chunks = [rows[i:i + P.JOB_SIZE] for i in range(0, len(rows), P.JOB_SIZE)]
    msgs = [[{"role": "system", "content": sysmsg},
             {"role": "user", "content": "\n".join(
                 f"{r['id']} {r['text']}" for r in c)}] for c in chunks]
    ids = [{r["id"] for r in c} for c in chunks]
    preds, spend = {}, {}
    for arm in a.arms.split(","):
        if arm == "spark":
            jobs = [P.spark.Job(id=f"tc-{i:02d}", messages=m,
                                est_out_tokens=60 * len(c) + 4000)
                    for i, (m, c) in enumerate(zip(msgs, chunks))]
            parse = P.make_parse({j.id: s for j, s in zip(jobs, ids)})
            out = P.spark.run("teacher-check", jobs, parse, max_usd=0.5,
                              workers=4, pilot=1, reasoning_effort="high")
            preds[arm] = {i: f for p in out.values() for i, f in p.items()}
            spend[arm] = None
        else:
            k = key()
            with ThreadPoolExecutor(4) as ex:
                res = list(ex.map(lambda m: deepseek(m, arm == "ds-think", k),
                                  msgs))
            parse = P.make_parse({str(i): s for i, s in enumerate(ids)})
            preds[arm] = {}
            for i, (text, _) in enumerate(res):
                try:
                    preds[arm] |= parse(P.spark.Job(str(i), [], 0), text)
                except P.spark.ParseError as e:
                    print(arm, "chunk", i, e)
            spend[arm] = round(sum(u for _, u in res), 4)
    for arm, p in preds.items():
        ok = sum(S.canon(p.get(r["id"], [])) == S.canon(
            [t for t in r["frame"] if isinstance(t, str)]) for r in rows)
        print(f"{arm}: exact {ok}/{len(rows)}, parsed {len(p)}, "
              f"deepseek usd {spend[arm]}")
    gold = {r["id"]: S.canon([t for t in r["frame"] if isinstance(t, str)])
            for r in rows}
    arms = list(preds)
    for i, x in enumerate(arms):
        for y in arms[i + 1:]:
            agree = [r["id"] for r in rows if r["id"] in preds[x] and
                     S.canon(preds[x][r["id"]]) ==
                     S.canon(preds[y].get(r["id"], []))]
            ok = sum(S.canon(preds[x][i2]) == gold[i2] for i2 in agree)
            print(f"{x} and {y} agree on {len(agree)}/{len(rows)}; "
                  f"of those {ok} match the gold")

if __name__ == "__main__":
    main()
