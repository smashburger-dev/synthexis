# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx", "pyyaml"]
# ///
"""Param-sense labels for harvested AU parameters via Muse Spark Open.

  uv run tools/mapping/label.py --run <name> --max-usd <cap>
      [--only "<substr>,<substr>"] [--workers 8]
      [--reasoning-effort none|low] [--temperature 0.2] [--dry-run]

Reads data/mapping/harvest/au/{plugins,params}.jsonl (status ok),
applies rule labels and two dedup stages, packs parameter lines into
jobs, sends them through spark.run, repairs missing indices once, and
writes data/mapping/labels/<run>/{labels,summary}.json*.
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spark  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
HARVEST = ROOT / "data" / "mapping" / "harvest" / "au"
JOB_MAX_LINES = 80
BULK_MAX_LINES = 120
BIG_PLUGIN_REPS = 100

RE_MACRO_NUM = re.compile(r"^#?\s*\d+$", re.I)
RE_MACRO_WORD = re.compile(r"^(param(eter)?|p)\s*#?\d+$", re.I)
RE_MACRO_SLOT = re.compile(
    r"^(macro|knob|quick\s*control|qc|ctrl|control|m"
    r"|mod\s*wheel\s*macro)\s*#?\s*\d+$", re.I)
RE_SKIP = re.compile(
    r"^((unused|reserved|spare|dummy)\s*\d*|n/?a|none|-+|\s*)$", re.I)
RE_CC = re.compile(r"(midi\s*cc|^cc\s*\d+)", re.I)

DB_CLASSES = {"amp.gain", "osc.level", "fx.eq.gain", "unsafe", "fx.mix"}
ADJ_INSTRUCTION = ("Two labellers disagree on the parameters below; "
    "their candidate labels follow each parameter line. Decide the "
    "final label for every parameter by the rules above. Output only "
    "the arrays [i,\"class\",d,lo,hi], one per parameter, in order.")
RE_DB = re.compile(
    r"(-inf(?:inity)?|-∞|[-+]?\d+(?:\.\d+)?)\s*db", re.I)
RE_PCT = re.compile(r"([-+]?\d+(?:\.\d+)?)\s*%")

JOB_META: dict[str, dict] = {}  # job_id -> {"n": int, "params": [...]}


def load_ontology() -> dict:
    return yaml.safe_load((HERE / "ontology.yaml").read_text())


def system_prompt(onto: dict) -> str:
    rows = []
    for t in onto["taxonomy"]:
        rows.append(
            f"{t['id']} | {t['quantity']} | mappable "
            f"{'true' if t['mappable'] else 'false'} | {t['desc']} | "
            f"{', '.join(t['examples'])}")
    return (HERE / "prompts" / "label.md").read_text() \
        + "\n\n" + "\n".join(rows)


def load_harvest() -> tuple[dict, dict]:
    """plugin_id -> latest plugin rec (status ok), plugin_id -> params."""
    plugins, params = {}, {}
    for line in (HARVEST / "plugins.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        if rec.get("status") == "ok":
            plugins[rec["plugin_id"]] = rec
    for line in (HARVEST / "params.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        params.setdefault(rec["plugin_id"], []).append(rec)
    for lst in params.values():
        lst.sort(key=lambda p: p["index"])
    return plugins, params


def rule_label(p: dict):
    """None -> goes to LLM; 'skip' -> dropped; (class,d,lo,hi) -> rule."""
    name = (p.get("name") or "").strip()
    if RE_SKIP.match(name):
        return "skip"
    if (RE_MACRO_NUM.match(name) or RE_MACRO_WORD.match(name)
            or RE_MACRO_SLOT.match(name)):
        return ("macro", 1, 0, 1)
    if RE_CC.search(name):
        return ("system", 1, 0, 1)
    flags = p.get("flags") or []
    if "writable" not in flags or "meter_read_only" in flags:
        return ("system", 1, 0, 1)
    return None


def template_key(p: dict) -> tuple:
    dig = lambda s: re.sub(r"\d+", "#", s or "")
    return (dig(p.get("name")),
            tuple(dig(g) for g in (p.get("group") or [])),
            p.get("unit"))


def unit_disp(p: dict) -> str:
    u = p.get("unit") or "generic"
    if u in ("generic", "custom") and p.get("unit_name"):
        return str(p["unit_name"])
    return u


def render_values(p: dict) -> str:
    vs = p.get("value_strings")
    if vs:
        if len(vs) == 2:
            return f"switch: {vs[0]} / {vs[1]}"
        s = "list: " + " / ".join(vs[:12])
        if len(vs) > 12:
            s += f" (+{len(vs) - 12} more)"
        return s
    samples = p.get("samples") or []
    if p.get("unit") == "boolean":
        lo = samples[0][1] if samples else None
        hi = samples[-1][1] if samples else None
        return f"switch: {lo or '0'} / {hi or '1'}"
    if samples:
        def at(i):
            v = samples[i][1] if i < len(samples) else None
            return "" if v is None else str(v)
        return " .. ".join(at(i) for i in (0, 2, 5, 8, 10))
    return f"{p.get('min')} .. {p.get('max')}"


def render_line(i: int, p: dict) -> str:
    # v2-bridged AUs prefix the path with '' and 'global' — strip them
    gs = list(p.get("group") or [])
    while gs and gs[0] in ("", "global"):
        gs.pop(0)
    group = " > ".join(gs)
    head = f"{group} > {p['name']}" if group else str(p["name"])
    default = p.get("default_string") or p.get("default")
    return (f"{i} | {head} | {unit_disp(p)} | {render_values(p)} "
            f"| default {default}")


def plugin_header(rec: dict) -> str:
    tags = ", ".join(rec.get("tags") or []) or "none"
    return (f"## Plugin: {rec['name']} by {rec['manufacturer_name']}; "
            f"type: {rec['type_name']}; tags: {tags}")


def _norm(s: str) -> str:
    s = (s or "").lower().strip()
    for suf in (" vst3", "au64", " (au)"):
        if s.endswith(suf):
            s = s[: -len(suf)]
    return re.sub(r"[\s\-]+", "", s)


def load_priority() -> dict:
    cfg = yaml.safe_load((HERE / "priority.yaml").read_text())
    names, vendors, prefixes = [], [], []
    for e in cfg.get("explicit") or []:
        if isinstance(e, dict):
            for k, v in e.items():
                (vendors if k == "vendor" else prefixes).append(v)
        elif isinstance(e, str) and e.startswith("vendor:"):
            vendors.append(e[len("vendor:"):])
        elif isinstance(e, str) and e.startswith("prefix:"):
            prefixes.append(e[len("prefix:"):])
        else:
            names.append(e)
    fav_path = ROOT / "data" / "mapping" / "fl_favorites.json"
    favs = []
    if cfg.get("fl_favorites") and fav_path.exists():
        d = json.loads(fav_path.read_text())
        favs = d.get("generators", []) + d.get("effects", [])
    return {"names": names, "favs": favs, "vendors": vendors,
            "prefixes": prefixes,
            "fl_stock": bool(cfg.get("fl_stock"))}


def is_priority(rec: dict, pr: dict) -> bool:
    n = _norm(rec.get("name"))
    m = (rec.get("manufacturer_name") or "").strip().lower()
    if pr["fl_stock"] and rec.get("type") == "fl":
        return True
    if any(m == v.lower() for v in pr["vendors"]):
        return True
    if any(n.startswith(_norm(p)) for p in pr["prefixes"]):
        return True
    return n in {_norm(x) for x in pr["names"] + pr["favs"]}


def build_plugin_work(plugins: dict, params: dict,
                      no_template: bool = False):
    """Per plugin: skip/rule/copy labels + representative lines for LLM.

    Returns (label_of, reps) where label_of maps
    (plugin_id, index) -> (class,d,lo,hi,source,job_id) for rule/copy
    labels and reps maps plugin_id -> list of rep params needing LLM.
    no_template disables the intra-plugin template dedup (priority).
    """
    label_of: dict = {}
    reps: dict[str, list] = {}

    # plugin dedup: same manufacturer + identical ordered param names
    first_of_nameset: dict[tuple, str] = {}
    for pid in plugins:
        plist = params.get(pid) or []
        # name minus manufacturer prefix, so AU "FabFilter Pro-Q 3"
        # matches VST3 "Pro-Q 3"
        nm = _norm(plugins[pid].get("name"))
        mnm = _norm(plugins[pid].get("manufacturer_name"))
        if mnm and nm.startswith(mnm):
            nm = nm[len(mnm):]
        names = tuple(p.get("name") for p in plist)
        # vendor key: identical plugins of one vendor (Mono/Stereo, Serum 2 /
        # Serum 2 FX); name key: same plugin across AU and VST3 (VST3 often
        # reports no vendor)
        keys = [("n", nm, names)]
        if mnm and mnm != "?":
            keys.insert(0, ("v", mnm, names))
        first = next((first_of_nameset[k] for k in keys
                      if k in first_of_nameset), None)
        if first is not None:
            cross = (plugins[first].get("type") != plugins[pid]
                     .get("type"))
            for p in plist:
                label_of[(pid, p["index"])] = \
                    ("copy", {"copied_from": first,
                              "copy_au": cross})
            continue
        for k in keys:
            first_of_nameset[k] = pid

        seen_templates: dict[tuple, int] = {}
        plugin_reps = []
        for p in plist:
            rl = rule_label(p)
            if rl == "skip":
                continue
            if rl is not None:
                label_of[(pid, p["index"])] = (*rl, "rule", None)
                continue
            tk = template_key(p)
            if not no_template and tk in seen_templates:
                label_of[(pid, p["index"])] = \
                    ("copy", {"rep_index": seen_templates[tk]})
            else:
                seen_templates[tk] = p["index"]
                plugin_reps.append(p)
        reps[pid] = plugin_reps
    return label_of, reps


def build_jobs(plugins: dict, reps: dict, sys_prompt: str,
               max_lines: int = JOB_MAX_LINES):
    """Pack rep lines into jobs <= max_lines. Returns [Job]."""
    jobs: list[spark.Job] = []
    cur: list[tuple] = []  # ("h", header) | ("p", param)
    cur_n = 0

    def flush():
        nonlocal cur, cur_n
        if cur:
            _emit_job(cur, sys_prompt, jobs)
        cur, cur_n = [], 0

    for pid, prec in plugins.items():
        plist = reps.get(pid) or []
        if not plist:
            continue
        header = plugin_header(prec)
        if len(plist) > BIG_PLUGIN_REPS:
            flush()
            for start in range(0, len(plist), max_lines):
                _emit_job([("h", header)]
                          + [("p", p) for p in
                             plist[start:start + max_lines]],
                          sys_prompt, jobs)
        else:
            need = 1 + len(plist)
            if cur_n + need > max_lines:
                flush()
            cur.append(("h", header))
            cur += [("p", p) for p in plist]
            cur_n += need
    flush()
    return jobs


def _emit_job(block: list, sys_prompt: str, jobs: list) -> None:
    """block = [('h', header)|('p', param), ...]; assign indices, build."""
    jid = f"j{len(jobs):04d}"
    user_lines = []
    meta = {"n": 0, "params": {}}
    for kind, val in block:
        if kind == "h":
            user_lines.append(val)
        else:
            i = meta["n"]
            user_lines.append(render_line(i, val))
            meta["params"][i] = val
            meta["n"] += 1
    JOB_META[jid] = meta
    jobs.append(spark.Job(
        id=jid,
        messages=[
            {"role": "system", "content": sys_prompt},
            {"role": "user", "content": "\n".join(user_lines)},
        ],
        est_out_tokens=14 * meta["n"] + 1800))


def make_parse(taxonomy_ids: set):
    def parse(job: spark.Job, text: str):
        meta = JOB_META[job.id]
        valid = meta.get("valid_idx") or list(range(meta["n"]))
        n = len(valid)
        labels: dict[int, list] = {}
        for line in text.splitlines():
            line = line.strip()
            if not line.startswith("["):
                continue
            try:
                arr = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not (isinstance(arr, list) and len(arr) == 5):
                continue
            i, cls, d, lo, hi = arr
            if not (isinstance(i, int) and not isinstance(i, bool)
                    and i in valid):
                continue
            if cls not in taxonomy_ids or d not in (1, -1):
                continue
            try:
                lo, hi = float(lo), float(hi)
            except (TypeError, ValueError):
                continue
            if not (0 <= lo < hi <= 1):
                continue
            if i not in labels:
                labels[i] = [cls, d, lo, hi]
        missing = [i for i in valid if i not in labels]
        if len(labels) < n / 2:
            raise spark.ParseError(f"{len(labels)}/{n} valid labels")
        return {"labels": labels, "missing": missing}
    return parse


def collect_labels(parsed: dict, jobs_by_id: dict, src: str,
                   label_of: dict, unresolved: list) -> None:
    """Write spark labels into label_of; append (job, index) leftovers."""
    for jid, res in parsed.items():
        meta = JOB_META[jid]
        labels = {int(k): v for k, v in res["labels"].items()}
        for i, lab in labels.items():
            p = meta["params"][i]
            label_of[(p["plugin_id"], p["index"])] = \
                (*lab, src, jid)
        for i in res.get("missing", []):
            unresolved.append((jid, i))


REPAIR_CHUNK = 20
MISSING_CHUNK = 10


def build_missing_jobs(need: dict, plugins: dict,
                       sys_prompt: str) -> list:
    """Missing-rep jobs of <=MISSING_CHUNK lines."""
    out = []
    for pid, plist in need.items():
        header = plugin_header(plugins[pid])
        for start in range(0, len(plist), MISSING_CHUNK):
            part = plist[start:start + MISSING_CHUNK]
            jid = f"m{len(out):04d}"
            user = [header] + [render_line(i, p)
                               for i, p in enumerate(part)]
            JOB_META[jid] = {
                "n": len(part),
                "params": {i: p for i, p in enumerate(part)}}
            out.append(spark.Job(
                id=jid,
                messages=[
                    {"role": "system", "content": sys_prompt},
                    {"role": "user", "content": "\n".join(user)},
                ],
                est_out_tokens=14 * len(part) + 1800))
    return out


def rebuild_repair_jobs(jobs_by_id: dict, unresolved: list,
                        sys_prompt: str, prefix: str = "r") -> list:
    """Re-render jobs containing only missing lines, <=20 per job."""
    by_job: dict[str, list[int]] = {}
    for jid, i in unresolved:
        by_job.setdefault(jid, []).append(i)
    out = []
    for jid, idxs in sorted(by_job.items()):
        old = jobs_by_id[jid]
        meta = JOB_META[jid]
        headers = [
            l for l in old.messages[1]["content"].splitlines()
            if l.startswith("## Plugin:")]
        for start in range(0, len(idxs), REPAIR_CHUNK):
            part = sorted(idxs)[start:start + REPAIR_CHUNK]
            user_lines = list(headers)
            for i in part:
                user_lines.append(render_line(i, meta["params"][i]))
            rid = f"{prefix}{len(out):04d}"
            JOB_META[rid] = {
                "n": len(part),
                "params": {i: meta["params"][i] for i in part},
                "valid_idx": part}
            out.append(spark.Job(
                id=rid,
                messages=[
                    {"role": "system", "content": sys_prompt},
                    {"role": "user", "content": "\n".join(user_lines)},
                ],
                est_out_tokens=14 * len(part) + 1800))
    return out


def sample_dbs(p: dict):
    """All 11 sample strings as dB floats (-inf -> -200), else None."""
    samples = p.get("samples") or []
    if len(samples) != 11:
        return None
    vals = []
    for _t, s in samples:
        if s is None:
            return None
        m = RE_DB.search(str(s))
        if not m:
            return None
        g = m.group(1).lower()
        vals.append(-200.0 if g.startswith("-") and "inf" in g
                    or g.startswith("-∞") else float(g))
    return vals


def sample_pcts(p: dict):
    samples = p.get("samples") or []
    if len(samples) != 11:
        return None
    vals = []
    for _t, s in samples:
        if s is None:
            return None
        m = RE_PCT.search(str(s))
        if not m:
            return None
        vals.append(float(m.group(1)))
    return vals


def _crossing(vals, threshold):
    """t in 0..1 where vals (at t=i/10) crosses threshold upwards."""
    for i, v in enumerate(vals):
        if v >= threshold:
            if i == 0:
                return 0.0
            v0 = vals[i - 1]
            return (i - 1) / 10 + ((threshold - v0) / (v - v0)) / 10 \
                if v != v0 else i / 10
    return None


def safe_override(cls: str, p: dict, lo: float, hi: float):
    """Deterministic safe-range override; returns (hi, applied)."""
    if cls in DB_CLASSES:
        vals = sample_dbs(p)
        if vals:
            if vals[-1] <= 0.5:
                return 1.0, True
            t = _crossing(vals, 0.0)
            if t is not None and t > lo:
                return min(t, 1.0), True
    elif cls == "fx.delay.feedback":
        vals = sample_pcts(p)
        if vals:
            if vals[-1] <= 90.0:
                return 1.0, True
            t = _crossing(vals, 90.0)
            if t is not None and t > lo:
                return min(t, 1.0), True
    return hi, False


def _copy_chain(label_of, pid, idx, max_hops: int = 6):
    """Keys visited when following copy records. A plugin copy keeps the
    index and moves to the source plugin; a template copy keeps the
    current plugin and moves to the representative index."""
    rec = label_of.get((pid, idx))
    hops = 0
    while rec is not None and len(rec) == 2 and hops < max_hops:
        hops += 1
        if "copied_from" in rec[1]:
            pid = rec[1]["copied_from"]
        else:
            idx = rec[1]["rep_index"]
        yield (pid, idx)
        rec = label_of.get((pid, idx))


def _copy_resolved(label_of, labelled, pid, idx) -> bool:
    return any(k in labelled for k in _copy_chain(label_of, pid, idx))


def spend_of(run_names: tuple) -> float:
    p = spark._ledger_path()
    total = 0.0
    if p.exists():
        for line in p.read_text().splitlines():
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("run") in run_names:
                total += float(r.get("cost_usd") or 0)
    return total


class BudgetDone(Exception):
    """Cumulative invocation budget exhausted; keep partial labels."""


def make_budget(args):
    used: list = []

    def budget(name: str) -> float:
        left = args.max_usd - spend_of(tuple(used))
        if left <= 0:
            raise BudgetDone(
                f"cumulative spend ${args.max_usd - left:.2f} reached "
                f"--max-usd before {name}; stopping")
        used.append(name)
        return left
    return budget


def run_pass(run_name: str, jobs: list, parse, args, extra_of: dict,
             unresolved_out: list, sys_prompt: str, budget,
             temperature: float | None = None,
             reasoning_effort=None) -> dict:
    """One labelling pass: run + up to 2 repair rounds (<=20 lines)."""
    jobs_by_id = {j.id: j for j in jobs}
    parsed = spark.run(
        run_name, jobs, parse, max_usd=budget(run_name),
        workers=args.workers,
        temperature=temperature or args.temperature,
        reasoning_effort=reasoning_effort or args.reasoning_effort)
    unresolved: list = []
    collect_labels(parsed, jobs_by_id, "spark", extra_of, unresolved)
    for rnd in (1, 2):
        if not unresolved:
            break
        repair = rebuild_repair_jobs(
            jobs_by_id, unresolved, sys_prompt, prefix=f"r{rnd}_")
        repair_by_id = {j.id: j for j in repair}
        rp = spark.run(
            f"{run_name}-repair{rnd}", repair, parse,
            max_usd=budget(f"{run_name}-repair{rnd}"), workers=args.workers,
            temperature=temperature or args.temperature,
            reasoning_effort=reasoning_effort or args.reasoning_effort)
        left = []
        collect_labels(rp, repair_by_id, "spark", extra_of, left)
        unresolved = left
    unresolved_out.extend(unresolved)
    return parsed


def build_priority_jobs(reps: dict, plugins: dict, sys_prompt: str,
                        reverse: bool, prefix: str) -> list:
    """Same packing as build_jobs but each plugin's order can be
    reversed inside the job."""
    rev_reps = {pid: (lst[::-1] if reverse else lst)
                for pid, lst in reps.items()}
    jobs0 = build_jobs(plugins, rev_reps, sys_prompt)
    for j in jobs0:
        old = j.id
        j.id = f"{prefix}{old}"
        JOB_META[j.id] = JOB_META.pop(old)
    return jobs0


def build_adj_jobs(disputed: list, plugins: dict, sys_prompt: str,
                   la: dict, lb: dict) -> list:
    """Adjudication jobs: line + candidate A/B, <=80 lines."""
    by_pid: dict[str, list] = {}
    for p in disputed:
        by_pid.setdefault(p["plugin_id"], []).append(p)
    out = []
    for pid, plist in by_pid.items():
        header = plugin_header(plugins[pid])
        for start in range(0, len(plist), JOB_MAX_LINES):
            part = plist[start:start + JOB_MAX_LINES]
            jid = f"c{len(out):04d}"
            user_lines = [ADJ_INSTRUCTION, header]
            meta = {"n": 0, "params": {}}
            for i, p in enumerate(part):
                user_lines.append(render_line(i, p))
                for nm, lab in (("A", la), ("B", lb)):
                    rec = lab.get((pid, p["index"]))
                    cand = (json.dumps([i] + list(rec[:4]))
                            if rec else "missing")
                    user_lines.append(f"candidate {nm}: {cand}")
                meta["params"][i] = p
                meta["n"] += 1
            JOB_META[jid] = meta
            out.append(spark.Job(
                id=jid,
                messages=[
                    {"role": "system", "content": sys_prompt},
                    {"role": "user", "content": "\n".join(user_lines)},
                ],
                est_out_tokens=14 * meta["n"] + 1800))
    return out


def agreed(a, b) -> bool:
    return (a and b and a[0] == b[0] and a[1] == b[1]
            and abs(a[2] - b[2]) <= 0.1 and abs(a[3] - b[3]) <= 0.1)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--only", default="")
    ap.add_argument("--harvest", default="au")
    ap.add_argument("--priority", action="store_true")
    ap.add_argument("--repair-missing", action="store_true")
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--reasoning-effort", default=None)
    ap.add_argument("--temperature", type=float, default=0.2)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    onto = load_ontology()
    taxo_ids = {t["id"] for t in onto["taxonomy"]}
    sys_prompt = system_prompt(onto)
    plugins, params = {}, {}
    for h in args.harvest.split(","):
        sub = h.strip()
        if not sub:
            continue
        hp = ROOT / "data" / "mapping" / "harvest" / sub
        pj = hp / "plugins.jsonl"
        pa = hp / "params.jsonl"
        if pj.exists():
            for line in pj.read_text().splitlines():
                if line.strip():
                    rec = json.loads(line)
                    if rec.get("status") == "ok":
                        plugins[rec["plugin_id"]] = rec
        if pa.exists():
            for line in pa.read_text().splitlines():
                if line.strip():
                    rec = json.loads(line)
                    if sub == "vst3":
                        group = [g for g in
                                 (rec.get("unit_path") or "")
                                 .split(" > ")]
                        if group and group[0].lower() == "root":
                            group = group[1:]
                        rec["group"] = group
                        rec["unit"] = rec.get("units") or "generic"
                        fl = list(rec.get("flags") or [])
                        if ("can_automate" in fl
                                and "read_only" not in fl):
                            fl += ["readable", "writable"]
                        else:
                            fl += ["readable"]
                        rec["flags"] = fl
                    params.setdefault(rec["plugin_id"],
                                      []).append(rec)
    for lst in params.values():
        lst.sort(key=lambda p: p["index"])
    pr = None
    try:
        pr = load_priority()
    except FileNotFoundError:
        pass
    if args.priority and pr:
        plugins = {pid: r for pid, r in plugins.items()
                   if is_priority(r, pr)}
        matched = {_norm(r["name"]) for r in plugins.values()}
        missing = [x for x in pr["names"]
                   if _norm(x) not in matched]
        fav_miss = [x for x in pr["favs"]
                    if _norm(x) not in matched]
        print(f"priority: {len(plugins)} plugins matched; "
              f"unmatched explicit names: {missing}")
        print(f"unmatched favorites: {len(fav_miss)} "
              f"(FL stock/not harvested, expected)")
    elif pr:
        plugins = {pid: r for pid, r in plugins.items()
                   if not is_priority(r, pr)}
    if args.only:
        pats = [s.lower() for s in args.only.split(",") if s.strip()]
        plugins = {pid: r for pid, r in plugins.items()
                   if any(s in pid.lower() for s in pats)}
    params = {pid: params.get(pid, []) for pid in plugins}
    label_of, reps = build_plugin_work(plugins, params)

    n_llm = sum(len(v) for v in reps.values())
    n_rule = sum(1 for v in label_of.values()
                 if len(v) == 6 and v[4] == "rule")
    n_copy = sum(1 for v in label_of.values() if v[0] == "copy")
    parse = make_parse(taxo_ids)
    out_dir = ROOT / "data" / "mapping" / "labels" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)

    if args.repair_missing:
        lpath = out_dir / "labels.jsonl"
        labelled = {}
        if lpath.exists():
            for line in lpath.read_text().splitlines():
                if line.strip():
                    r = json.loads(line)
                    labelled[(r["plugin_id"], r["index"])] = r
        need = {pid: [p for p in plist
                      if (pid, p["index"]) not in labelled]
                for pid, plist in reps.items()}
        need = {k: v for k, v in need.items() if v}
        jobs = build_missing_jobs(need, plugins, sys_prompt)
        est = sum(spark._estimate_usd(j) for j in jobs)
        print(f"repair-missing: {sum(len(v) for v in need.values())} "
              f"unlabelled reps in {len(jobs)} jobs, "
              f"estimate ${est:.3f}")
        if args.dry_run:
            return
        budget = make_budget(args)
        repaired: dict = {}
        left: list = []
        run_pass(f"{args.run}-repairfinal", jobs, parse, args,
                 repaired, left, sys_prompt, budget,
                 reasoning_effort="medium")
        # merge: old rows + repaired rep labels + newly resolved copies
        for key, lab in repaired.items():
            labelled[key] = {
                "plugin_id": key[0], "index": key[1], "class": lab[0],
                "d": lab[1], "lo": lab[2], "hi": lab[3],
                "source": "spark-repair", "job_id": lab[5]}
        n_new_copies = 0
        with lpath.open("w") as f:
            by_source, by_class = Counter(), Counter()
            n_written = 0
            for pid, prec in plugins.items():
                for p in params.get(pid, []):
                    key = (pid, p["index"])
                    row = labelled.get(key)
                    if row is None:
                        for rk in _copy_chain(label_of, pid, p["index"]):
                            nxt = labelled.get(rk)
                            if nxt:
                                row = dict(nxt)
                                row["plugin_id"] = pid
                                row["index"] = p["index"]
                                row["name"] = p.get("name")
                                row["source"] = "copy"
                                if rk[0] != pid:
                                    row["copied_from"] = rk[0]
                                n_new_copies += 1
                                break
                        if row is None:
                            continue
                    hi, applied = safe_override(
                        row["class"], p, row["lo"], row["hi"])
                    out = {
                        "plugin_id": pid, "index": p["index"],
                        "name": p.get("name"),
                        "group": p.get("group") or [],
                        "unit": p.get("unit"),
                        "class": row["class"], "d": row["d"],
                        "lo": row["lo"], "hi": hi,
                        "source": row["source"],
                        "job_id": row.get("job_id"),
                        "safe_source":
                            "rule_db" if applied else row["source"]}
                    if row.get("copied_from"):
                        out["copied_from"] = row["copied_from"]
                    f.write(json.dumps(out) + "\n")
                    n_written += 1
                    by_source[row["source"]] += 1
                    by_class[row["class"]] += 1
        still = sum(1 for pid in plugins
                    for p in params.get(pid, [])
                    if (pid, p["index"]) not in labelled
                    and not _copy_resolved(label_of, labelled,
                                           pid, p["index"]))
        summary = {
            "plugins_labelled": len(plugins),
            "params_labelled": n_written,
            "unlabelled": still,
            "repaired_reps": len(repaired),
            "resolved_copies": n_new_copies,
            "by_source": dict(by_source),
            "by_class": dict(by_class.most_common()),
            "spend_usd": round(spend_of((
                f"{args.run}-a", f"{args.run}-b",
                f"{args.run}-adj", f"{args.run}-repairfinal",
                *[f"{args.run}-{s}-repair{i}"
                  for s in ("a", "b") for i in (1, 2)])), 4),
        }
        (out_dir / "summary.json").write_text(
            json.dumps(summary, indent=1))
        print(json.dumps(summary))
        return

    if args.priority:
        jobs_a = build_priority_jobs(reps, plugins, sys_prompt,
                                     False, "a")
        jobs_b = build_priority_jobs(reps, plugins, sys_prompt,
                                     True, "b")
        est = (sum(spark._estimate_usd(j) for j in jobs_a)
               + sum(spark._estimate_usd(j) for j in jobs_b))
        print(f"PRIORITY {len(plugins)} plugins: {n_llm} LLM lines x2 "
              f"passes in {len(jobs_a)}+{len(jobs_b)} jobs, "
              f"{n_copy} copies, {n_rule} rule, estimate ${est:.3f} "
              f"+ adjudication")
        if args.dry_run:
            with (out_dir / "sample_jobs.txt").open("w") as f:
                f.write("SYSTEM PROMPT:\n" + sys_prompt + "\n\n")
                for j in jobs_a[:2] + jobs_b[:2]:
                    f.write(f"=== JOB {j.id} ===\n"
                            + j.messages[1]["content"] + "\n\n")
            print(f"dry-run: wrote {out_dir / 'sample_jobs.txt'}")
            return

        budget = make_budget(args)
        try:
            la: dict = {}
            leftover: list = []
            run_pass(f"{args.run}-a", jobs_a, parse, args, la,
                     leftover, sys_prompt, budget, temperature=0.2)
            lb: dict = {}
            run_pass(f"{args.run}-b", jobs_b, parse, args, lb,
                     leftover, sys_prompt, budget, temperature=0.7)
        except BudgetDone as e:
            print(e)
            la = locals().get("la", {})
            lb = locals().get("lb", {})

        # merge into label_of; disagreements -> adjudication
        disputed = []
        for pid, plist in reps.items():
            for p in plist:
                key = (pid, p["index"])
                a, b = la.get(key), lb.get(key)
                if agreed(a, b):
                    label_of[key] = (*a[:4], "spark", a[5])
                elif a or b:
                    disputed.append(p)
                # both missing -> unlabelled
        adj_of: dict = {}
        if disputed:
            adj_jobs = build_adj_jobs(disputed, plugins, sys_prompt,
                                      la, lb)
            jobs_by_id = {j.id: j for j in adj_jobs}
            args_eff = args.reasoning_effort or "low"
            try:
                parsed = spark.run(
                    f"{args.run}-adj", adj_jobs, parse,
                    max_usd=budget(f"{args.run}-adj"),
                    workers=args.workers,
                    temperature=0.2, reasoning_effort=args_eff)
                collect_labels(parsed, jobs_by_id, "spark", adj_of, [])
            except BudgetDone as e:
                print(e)
        disagreements = []
        for p in disputed:
            key = (p["plugin_id"], p["index"])
            a, b = la.get(key), lb.get(key)
            fin = adj_of.get(key)
            if fin:
                label_of[key] = (*fin[:4], "spark", fin[5])
                final = list(fin[:4])
                fsrc = "adjudicated"
            else:
                final = list(a[:4]) if a else list(b[:4])
                fsrc = "fallback_a" if a else "fallback_b"
                if a or b:
                    label_of[key] = (*(a or b)[:4], "spark",
                                     (a or b)[5])
            disagreements.append({
                "plugin_id": p["plugin_id"], "index": p["index"],
                "name": p.get("name"),
                "a": list(a[:4]) if a else None,
                "b": list(b[:4]) if b else None,
                "final": final, "final_source": fsrc})
        with (out_dir / "disagreements.jsonl").open("w") as f:
            for d in disagreements:
                f.write(json.dumps(d) + "\n")
        agree_of = {}
        for pid, plist in reps.items():
            for p in plist:
                key = (pid, p["index"])
                a, b = la.get(key), lb.get(key)
                if agreed(a, b):
                    agree_of[key] = (True, False)
                elif a or b:
                    agree_of[key] = (False, key in adj_of)
    else:
        jobs = build_jobs(plugins, reps, sys_prompt,
                          max_lines=BULK_MAX_LINES)
        est = sum(spark._estimate_usd(j) for j in jobs)
        print(f"{len(plugins)} plugins: {n_llm} LLM lines in "
              f"{len(jobs)} jobs, {n_copy} copies, {n_rule} rule "
              f"labels, estimate ${est:.3f}")
        if args.dry_run:
            with (out_dir / "sample_jobs.txt").open("w") as f:
                f.write("SYSTEM PROMPT:\n" + sys_prompt + "\n\n")
                for j in jobs[:3]:
                    f.write(f"=== JOB {j.id} ===\n"
                            + j.messages[1]["content"] + "\n\n")
            print(f"dry-run: wrote {out_dir / 'sample_jobs.txt'}")
            return
        leftover = []
        jobs_by_id = {j.id: j for j in jobs}
        budget = make_budget(args)
        try:
            parsed = spark.run(
                args.run, jobs, parse, max_usd=budget(args.run),
                workers=args.workers, temperature=args.temperature,
                reasoning_effort=args.reasoning_effort)
            unresolved = []
            collect_labels(parsed, jobs_by_id, "spark", label_of,
                           unresolved)
            for rnd in (1, 2):
                if not unresolved:
                    break
                repair = rebuild_repair_jobs(
                    jobs_by_id, unresolved, sys_prompt,
                    prefix=f"r{rnd}_")
                repair_by_id = {j.id: j for j in repair}
                rp = spark.run(
                    f"{args.run}-repair{rnd}", repair, parse,
                    max_usd=budget(f"{args.run}-repair{rnd}"),
                    workers=args.workers,
                    temperature=args.temperature,
                    reasoning_effort=args.reasoning_effort)
                left = []
                collect_labels(rp, repair_by_id, "spark", label_of,
                               left)
                unresolved = left
        except BudgetDone as e:
            print(e)
        agree_of = None

    # write labels.jsonl
    by_source, by_class = Counter(), Counter()
    n_written, unlabelled = 0, []
    with (out_dir / "labels.jsonl").open("w") as f:
        for pid, prec in plugins.items():
            for p in params.get(pid, []):
                rec = label_of.get((pid, p["index"]))
                if rec is None:
                    unlabelled.append((pid, p["index"]))
                    continue
                rec0 = rec[1] if len(rec) == 2 else {}
                copied_from = rec0.get("copied_from")
                hops = 0
                for rk in _copy_chain(label_of, pid, p["index"]):
                    hops += 1
                    nxt = label_of.get(rk)
                    if nxt is None:
                        break
                    rec = nxt
                    if len(rec) == 6:
                        break
                if len(rec) != 6:
                    unlabelled.append((pid, p["index"]))
                    continue
                cls, d, lo, hi, source, job_id = rec
                if hops:
                    source = ("copy-au" if rec0.get("copy_au")
                              else "copy")
                hi, applied = safe_override(cls, p, lo, hi)
                line = {
                    "plugin_id": pid, "index": p["index"],
                    "name": p.get("name"), "group": p.get("group") or [],
                    "unit": p.get("unit"), "class": cls, "d": d,
                    "lo": lo, "hi": hi, "source": source,
                    "job_id": job_id,
                    "safe_source": "rule_db" if applied else source}
                if copied_from:
                    line["copied_from"] = copied_from
                if agree_of is not None:
                    ag, adj = agree_of.get(
                        (pid, p["index"]), (None, False))
                    line["agree"] = ag
                    line["adjudicated"] = adj
                f.write(json.dumps(line) + "\n")
                n_written += 1
                by_source[source] += 1
                by_class[cls] += 1

    run_names = [args.run] if not args.priority else [
        f"{args.run}-{s}" for s in ("a", "b", "adj")]
    run_names += [f"{r}-repair{i}" for r in list(run_names)
                  for i in (1, 2)]
    summary = {
        "plugins_labelled": len(plugins),
        "params_labelled": n_written,
        "unlabelled": len(unlabelled),
        "unlabelled_list": unlabelled[:50],
        "by_source": dict(by_source),
        "by_class": dict(by_class.most_common()),
        "spend_usd": round(spend_of(tuple(run_names)), 4),
    }
    (out_dir / "summary.json").write_text(
        json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
