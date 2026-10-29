# /// script
# requires-python = ">=3.11"
# ///
"""Run the AU parameter harvest over every installed component.

  uv run tools/mapping/harvest/run_harvest.py [--retry-failed]
      [--match <substr>]... [--limit N] [--timeout 60] [--workers 3]

Builds data/mapping/bin/au_harvest if needed, runs --list, then one
subprocess per component (3 in parallel, hard timeout, kill process
group). Appends data/mapping/harvest/au/plugins.jsonl (one line per
component) and params.jsonl (one line per parameter). Resumable:
components with a recorded status are skipped; --retry-failed reruns
timeout/error entries once with --oop.
"""

import argparse
import json
import os
import signal
import statistics
import subprocess
import sys
import threading
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BIN = ROOT / "data" / "mapping" / "bin" / "au_harvest"
BUILD = ROOT / "tools" / "mapping" / "harvest" / "build.sh"
SRC = ROOT / "tools" / "mapping" / "harvest" / "au_harvest.swift"
OUT_DIR = ROOT / "data" / "mapping" / "harvest" / "au"
PLUGINS_JSONL = OUT_DIR / "plugins.jsonl"
PARAMS_JSONL = OUT_DIR / "params.jsonl"
SUMMARY_JSON = OUT_DIR / "summary.json"


def key_of(comp: dict) -> str:
    return f"{comp['type']}/{comp['subtype']}/{comp['manufacturer']}"


def build_if_needed() -> None:
    if BIN.exists() and BIN.stat().st_mtime >= SRC.stat().st_mtime:
        return
    r = subprocess.run(["bash", str(BUILD)])
    if r.returncode != 0 or not BIN.exists():
        sys.exit(f"build failed (exit {r.returncode})")


def list_components() -> list[dict]:
    r = subprocess.run([str(BIN), "--list"], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"--list failed (exit {r.returncode}): {r.stderr[-500:]}")
    comps = []
    for line in r.stdout.splitlines():
        line = line.strip()
        if line:
            comps.append(json.loads(line))
    return comps


def load_done() -> dict[str, dict]:
    """key -> latest recorded line (later lines win on retry)."""
    done: dict[str, dict] = {}
    if PLUGINS_JSONL.exists():
        for line in PLUGINS_JSONL.read_text().splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            done[rec["key"]] = rec
    return done


def run_one(comp: dict, timeout: int, oop: bool, tmpdir: Path) -> dict:
    """Run one --dump subprocess; never raises.

    The JSON document comes back via a temp file (--out): plugins that
    print to stdout can no longer corrupt it. Child stdout/stderr are
    only logged, truncated.
    """
    key = key_of(comp).replace("/", "_")
    out_path = tmpdir / f"{key}-{os.getpid()}-{threading.get_ident()}.json"
    cmd = [str(BIN), "--dump", comp["type"], comp["subtype"],
           comp["manufacturer"], "--out", str(out_path)]
    if oop:
        cmd.append("--oop")
    t0 = time.monotonic()
    rec = {k: comp.get(k) for k in (
        "type", "subtype", "manufacturer", "name",
        "manufacturer_name", "type_name", "version", "tags")}
    rec["key"] = key_of(comp)
    rec["plugin_id"] = f"{comp.get('manufacturer_name','?')}/{comp.get('name','?')}"
    rec["oop"] = oop
    try:
        p = subprocess.Popen(
            cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, start_new_session=True)
        try:
            out, err = p.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(p.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()
            rec.update(status="timeout", n_params=0, params=[],
                       error=f"killed after {timeout}s")
            return rec
        rc = p.returncode
        # child noise is diagnostic only, keep a short tail
        noise = ((err or "")[-160:] + " | " + (out or "")[-160:]).strip(" |")
        rec["log_tail"] = noise[-300:]
        doc = None
        if out_path.exists():
            try:
                doc = json.loads(out_path.read_text())
            except (json.JSONDecodeError, OSError):
                doc = None
        if doc is None:
            sig = f"signal {-rc}" if rc < 0 else f"exit {rc}"
            rec.update(status="error", n_params=0, params=[],
                       error=f"{sig}: {noise}" if noise else sig)
            return rec
        rec.update(doc)
        rec["key"] = key_of(comp)
        rec["plugin_id"] = (
            f"{doc.get('manufacturer_name') or comp.get('manufacturer_name','?')}"
            f"/{doc.get('name') or comp.get('name','?')}")
        params = doc.get("params") or []
        rec["params"] = params
        rec["n_params"] = len(params)
        if doc.get("error"):
            rec["status"] = "error"
            rec["error"] = str(doc["error"])[:300]
        else:
            rec["status"] = "ok" if params else "no_params"
        return rec
    except Exception as e:  # spawn failure etc.
        rec.update(status="error", n_params=0, params=[], error=str(e)[:300])
        return rec
    finally:
        out_path.unlink(missing_ok=True)
        rec["load_ms"] = round((time.monotonic() - t0) * 1000)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--retry-failed", action="store_true")
    ap.add_argument("--match", action="append", default=[],
                    help="substring filter on name/manufacturer (repeatable)")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--timeout", type=int, default=60)
    ap.add_argument("--workers", type=int, default=3)
    args = ap.parse_args()

    build_if_needed()
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    comps = list_components()
    done = load_done()

    todo: list[tuple[dict, bool]] = []  # (comp, oop)
    for c in comps:
        k = key_of(c)
        prev = done.get(k)
        if prev is None:
            todo.append((c, False))
        elif (args.retry_failed and prev.get("status") in ("timeout", "error")
              and prev.get("attempt", 1) < 2):
            todo.append((c, True))
    if args.match:
        pats = [m.lower() for m in args.match]
        todo = [(c, o) for c, o in todo
                if any(p in (c.get("name", "") + " "
                             + c.get("manufacturer_name", "")).lower()
                       for p in pats)]
    if args.limit:
        todo = todo[: args.limit]

    print(f"{len(comps)} components, {len(done)} recorded, "
          f"{len(todo)} to harvest", flush=True)
    if not todo:
        write_summary(done)
        return

    t0 = time.monotonic()
    n_done = 0
    tmpdir = OUT_DIR / "tmp"
    tmpdir.mkdir(exist_ok=True)
    pf = PLUGINS_JSONL.open("a")
    af = PARAMS_JSONL.open("a")
    try:
        with ThreadPoolExecutor(max_workers=args.workers) as ex:
            futs = {
                ex.submit(run_one, c, args.timeout, oop, tmpdir): c
                for c, oop in todo
            }
            for fut in as_completed(futs):
                c = futs[fut]
                rec = fut.result()
                rec["attempt"] = 2 if rec.pop("oop") else 1
                rec["ts"] = round(time.time(), 3)
                n_done += 1
                params = rec.pop("params", [])
                pf.write(json.dumps(rec) + "\n")
                pf.flush()
                pid = rec["plugin_id"]
                for p in params:
                    af.write(json.dumps({"plugin_id": pid, **p}) + "\n")
                af.flush()
                done[rec["key"]] = rec
                print(
                    f"[{n_done}/{len(todo)}] {rec['status']:<9} "
                    f"{pid} ({rec['n_params']} params, "
                    f"{rec['load_ms']} ms)", flush=True)
    finally:
        pf.close()
        af.close()

    write_summary(done)
    print(f"wall {time.monotonic() - t0:.0f}s -> {OUT_DIR}", flush=True)


def write_summary(done: dict[str, dict]) -> None:
    recs = list(done.values())
    by_status = Counter(r["status"] for r in recs)
    n_par = [r["n_params"] for r in recs if r["status"] == "ok"]
    by_mfg = Counter(r.get("manufacturer_name", "?") for r in recs)
    summary = {
        "components": len(recs),
        "by_status": dict(by_status),
        "total_params": sum(n_par),
        "params_per_plugin": {
            "median": statistics.median(n_par) if n_par else 0,
            "max": max(n_par) if n_par else 0,
        },
        "top_manufacturers": by_mfg.most_common(20),
        "failed": [
            {"plugin_id": r["plugin_id"], "status": r["status"],
             "error": r.get("error", "")[:160]}
            for r in recs if r["status"] in ("timeout", "error")
        ],
    }
    SUMMARY_JSON.write_text(json.dumps(summary, indent=1))
    print(f"summary: {dict(by_status)} total_params={summary['total_params']}")


if __name__ == "__main__":
    main()
