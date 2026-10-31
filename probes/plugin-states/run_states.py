# /// script
# requires-python = ">=3.11"
# ///
"""State-list harvest: the text of every state of every stepped parameter
(VST3 steps > 0, AU indexed/boolean/value_strings), plus a dense text scan
of continuous parameters whose 11 harvest samples show named or repeated
text (hidden steps).

  uv run probes/plugin-states/run_states.py [--format vst3|au|both]
      [--match STR] [--retry-failed] [--workers 3] [--timeout 90]

Same crash isolation as tools/mapping/harvest: one subprocess per plugin
(own process group, killed on timeout), 3 workers, resumable. Reads the
harvest (never writes it) and writes
data/mapping/harvest/states/{vst3,au}.jsonl (one line per parameter),
{vst3,au}_runs.jsonl (one line per plugin) and summary.json.
"""

import argparse
import json
import os
import re
import signal
import subprocess
import threading
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main_checkout() -> Path:
    """data/ is git-ignored and only exists in the main checkout."""
    common = subprocess.run(
        ["git", "-C", str(HERE), "rev-parse", "--path-format=absolute",
         "--git-common-dir"], capture_output=True, text=True, check=True)
    return Path(common.stdout.strip()).parent


DATA = main_checkout() / "data" / "mapping"
HARVEST = DATA / "harvest"
OUT = HARVEST / "states"
BIN = {"vst3": DATA / "bin" / "vst3_states",
       "au": DATA / "bin" / "au_states"}
DENSE_MAX_RUNS = 64  # more distinct text runs: continuous, not states
HAS_DIGIT = re.compile(r"\d")


def latest(path: Path, key: str) -> dict[str, dict]:
    out = {}
    if path.exists():
        for line in path.read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                out[r[key]] = r
    return out


def dense_candidate(texts: list) -> bool:
    """Text changes, but not at every sample, or names a state."""
    t = [x for x in texts if x]
    k = len(set(t))
    if k < 2 or len(t) < len(texts):
        return False
    return k < len(texts) or any(not HAS_DIGIT.search(x) for x in t)


def candidates(fmt: str) -> dict[str, set]:
    """plugin_id -> ids (VST3) or addresses (AU) to scan densely."""
    out: dict[str, set] = {}
    with (HARVEST / fmt / "params.jsonl").open() as f:
        for line in f:
            r = json.loads(line)
            if fmt == "vst3":
                if r["steps"] != 0:
                    continue
                pid = r["id"]
            else:
                if r.get("unit") in ("indexed", "boolean") \
                        or r.get("value_strings"):
                    continue
                pid = r["address"]
            texts = [s[1] or "" for s in r.get("samples") or []]
            if texts and dense_candidate(texts):
                out.setdefault(r["plugin_id"], set()).add(pid)
    return out


def jobs(fmt: str) -> list[dict]:
    recs = latest(HARVEST / fmt / "plugins.jsonl", "key")
    return [r for r in recs.values() if r["status"] == "ok"]


def command(fmt: str, rec: dict, out_path: Path, dense_path: Path,
            oop: bool) -> list[str]:
    if fmt == "vst3":
        cmd = [str(BIN["vst3"]), "--dump", rec["bundle"]]
    else:
        cmd = [str(BIN["au"]), "--dump", rec["type"], rec["subtype"],
               rec["manufacturer"]]
        if oop:
            cmd.append("--oop")
    return cmd + ["--out", str(out_path), "--dense", str(dense_path)]


def states_of(p: dict) -> dict:
    """Normalize one dumped parameter to [normalized value, text] states."""
    if p["source"] == "steps":
        st = [[s[0], s[1] or (s[2] if len(s) > 2 else "")]
              for s in p["states"]]
        return {"states": st}
    runs = p["runs"]
    if len(runs) > DENSE_MAX_RUNS or len(runs) < 2:
        return {"continuous": True, "n_runs": len(runs)}
    # write the middle of each run: the plugin reports that text there
    return {"states": [[round((a + b) / 2, 6), s] for a, b, s in runs],
            "ranges": [[a, b] for a, b, _ in runs]}


def run_one(fmt: str, rec: dict, dense: set, timeout: int, oop: bool,
            tmpdir: Path) -> tuple[dict, list[dict]]:
    tag = f"{rec['key'].replace('/', '_')}-{os.getpid()}-" \
          f"{threading.get_ident()}"
    out_path = tmpdir / f"{tag}.json"
    dense_path = tmpdir / f"{tag}.ids"
    dense_path.write_text("\n".join(str(i) for i in sorted(dense)))
    run = {"key": rec["key"], "plugin_id": rec["plugin_id"],
           "format": fmt, "status": "error", "oop": oop,
           "n_dense_candidates": len(dense)}
    rows: list[dict] = []
    t0 = time.monotonic()
    try:
        p = subprocess.Popen(command(fmt, rec, out_path, dense_path, oop),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             text=True, start_new_session=True)
        try:
            out, err = p.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(p.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()
            run.update(status="timeout", error=f"killed after {timeout}s")
            return run, rows
        noise = ((err or "")[-160:] + " | " + (out or "")[-160:]).strip(" |")
        doc = None
        if out_path.exists():
            try:
                doc = json.loads(out_path.read_text())
            except (json.JSONDecodeError, OSError):
                doc = None
        if doc is None:
            run["error"] = f"exit {p.returncode}: {noise}"[:300]
            return run, rows
        if fmt == "vst3":
            groups = [(c.get("name"), c.get("params") or [], c.get("error"))
                      for c in doc.get("classes", [])]
            presets = [{"class_name": c.get("name"), "list": pl["name"],
                        "programs": pl["programs"]}
                       for c in doc.get("classes", [])
                       for pl in c.get("program_lists") or []]
        else:
            groups = [(None, doc.get("params") or [], doc.get("error"))]
            presets = [{"class_name": None, "list": "factory",
                        "programs": doc.get("factory_presets") or []}]
        run["presets"] = [pl for pl in presets if pl["programs"]]
        errors = [e for _, _, e in groups if e]
        n_cont = 0
        for cls, params, _ in groups:
            for prm in params:
                pid = prm.get("id", prm.get("address"))
                if prm["source"] == "dense" and pid not in dense:
                    continue
                s = states_of(prm)
                if s.get("continuous"):
                    n_cont += 1
                    continue
                row = {"plugin_id": rec["plugin_id"], "key": rec["key"],
                       "format": fmt,
                       "class_name": cls, "index": prm["index"],
                       "id": pid, "name": prm["name"],
                       "steps": (prm["steps"] if prm["source"] == "steps"
                                 else len(s["states"]) - 1),
                       "source": prm["source"],
                       "default": prm.get("default")}
                for k in ("truncated", "program_change", "unit", "min",
                          "max", "n_value_strings"):
                    if prm.get(k) not in (None, False):
                        row[k] = prm[k]
                row.update(s)
                rows.append(row)
        run["n_params"] = len(rows)
        run["n_dense_continuous"] = n_cont
        run["status"] = "ok" if not errors or rows else "error"
        if errors:
            run["error"] = "; ".join(errors)[:300]
        return run, rows
    except Exception as e:  # never let one plugin stop the run
        run["error"] = str(e)[:300]
        return run, rows
    finally:
        out_path.unlink(missing_ok=True)
        dense_path.unlink(missing_ok=True)
        run["ms"] = round((time.monotonic() - t0) * 1000)


def harvest(fmt: str, args) -> float:
    runs_path = OUT / f"{fmt}_runs.jsonl"
    rows_path = OUT / f"{fmt}.jsonl"
    done = latest(runs_path, "key")
    todo = []
    for rec in jobs(fmt):
        prev = done.get(rec["key"])
        if prev is None or (args.retry_failed
                            and prev["status"] in ("error", "timeout")):
            todo.append(rec)
    if args.match:
        todo = [r for r in todo if args.match.lower() in r["key"].lower()]
    cand = candidates(fmt)
    print(f"{fmt}: {len(done)} recorded, {len(todo)} to run, "
          f"{sum(len(v) for v in cand.values())} dense candidates")
    tmpdir = OUT / "tmp"
    tmpdir.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    with runs_path.open("a") as rf, rows_path.open("a") as pf, \
            ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(run_one, fmt, r, cand.get(r["plugin_id"], set()),
                          args.timeout,
                          args.retry_failed and r["key"] in done, tmpdir): r
                for r in todo}
        for i, fut in enumerate(as_completed(futs), 1):
            run, rows = fut.result()
            for row in rows:
                pf.write(json.dumps(row) + "\n")
            pf.flush()
            rf.write(json.dumps(run) + "\n")
            rf.flush()
            if i % 25 == 0 or i == len(todo):
                print(f"[{fmt} {i}/{len(todo)}] {run['status']} "
                      f"{run['plugin_id']} ({run.get('n_params', 0)} "
                      f"params, {run['ms']} ms)", flush=True)
    return time.monotonic() - t0


def summarize(fmt: str, wall: float) -> dict:
    runs = latest(OUT / f"{fmt}_runs.jsonl", "key")
    keep = {r["key"] for r in runs.values() if r["status"] == "ok"}
    rows = []
    with (OUT / f"{fmt}.jsonl").open() as f:
        for line in f:
            r = json.loads(line)
            if r["key"] in keep:
                rows.append(r)
    # a retried plugin appears twice in the rows file: keep one copy
    seen, uniq = set(), []
    for r in rows:
        k = (r["key"], r["class_name"], r["index"])
        if k not in seen:
            seen.add(k)
            uniq.append(r)
    src = Counter(r["source"] for r in uniq)
    n_states = sum(len(r.get("states", [])) for r in uniq)
    empty = sum(1 for r in uniq for s in r.get("states", []) if not s[1])
    all_empty = sum(1 for r in uniq if r.get("states")
                    and not any(s[1] for s in r["states"]))
    return {
        "plugins_run": len(runs),
        "by_status": dict(Counter(r["status"] for r in runs.values())),
        "failed": [{"plugin_id": r["plugin_id"], "status": r["status"],
                    "error": r.get("error", "")[:160]}
                   for r in runs.values() if r["status"] != "ok"],
        "params": len(uniq),
        "params_by_source": dict(src),
        "plugins_covered": len({r["plugin_id"] for r in uniq}),
        "plugins_covered_by_source": {
            s: len({r["plugin_id"] for r in uniq if r["source"] == s})
            for s in src},
        "states": n_states,
        "states_empty_text": empty,
        "params_all_text_empty": all_empty,
        "truncated": sum(1 for r in uniq if r.get("truncated")),
        "dense_continuous": sum(r.get("n_dense_continuous", 0)
                                for r in runs.values()),
        "plugins_with_presets": sum(1 for r in runs.values()
                                    if r.get("presets")),
        "plugins_with_named_presets": sum(
            1 for r in runs.values()
            if any(len({x for x in pl["programs"] if x}) > 1
                   and not all(re.fullmatch(r"(Prog(ram)?|Preset) ?\d+", x)
                               for x in pl["programs"] if x)
                   for pl in r.get("presets") or [])),
        "wall_s": round(wall),
        "run_ms_sum": sum(r.get("ms", 0) for r in runs.values()),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--format", choices=["vst3", "au", "both"],
                    default="both")
    ap.add_argument("--match", default="")
    ap.add_argument("--retry-failed", action="store_true")
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--timeout", type=int, default=90)
    ap.add_argument("--fresh", action="store_true",
                    help="truncate this probe's own output files first")
    args = ap.parse_args()
    for fmt in BIN:
        if not BIN[fmt].exists():
            raise SystemExit(f"missing {BIN[fmt]}: run build.sh")
    OUT.mkdir(parents=True, exist_ok=True)
    fmts = ["vst3", "au"] if args.format == "both" else [args.format]
    if args.fresh:
        for fmt in fmts:
            for name in (f"{fmt}.jsonl", f"{fmt}_runs.jsonl"):
                (OUT / name).write_text("")
    summary_path = OUT / "summary.json"
    summary = json.loads(summary_path.read_text()) \
        if summary_path.exists() else {}
    for fmt in fmts:
        wall = harvest(fmt, args)
        s = summarize(fmt, wall)
        prev = {} if args.fresh else summary.get(fmt, {})
        s["wall_s_total"] = prev.get("wall_s_total", 0) + s["wall_s"]
        summary[fmt] = s
        summary_path.write_text(json.dumps(summary, indent=1))
        print(fmt, {k: v for k, v in s.items() if k != "failed"})


if __name__ == "__main__":
    main()
