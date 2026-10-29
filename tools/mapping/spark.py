# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Baseten Model API client (Muse Spark Open) with cost guards.

Used as a module by the tools/mapping drivers (sys.path.insert this
dir, `import spark`), and as a CLI:

  uv run tools/mapping/spark.py ledger
  uv run tools/mapping/spark.py probe

Endpoint is OpenAI Chat Completions compatible. Billing is per token
only. Identical prompt prefixes are cached automatically; a stable
x-session-affinity header per worker routes related requests to the
same replica. The API key lives in macOS Keychain item
`synthexis.baseten`; it is read into memory only, never printed.

Cost guards (same pattern as tools/labels/run.py):
  1. up-front estimate, refused above --max-usd
  2. ledger total + estimate refused above $SYNTHEXIS_BASETEN_CAP (60)
  3. pilot: first `pilot` jobs re-estimate the real cost; abort if the
     re-estimate exceeds --max-usd or 1.5x the up-front estimate
  4. rolling stop: no new job once spent + in-flight * mean >= max_usd;
     finished jobs stay cached, rerun to resume

Every HTTP attempt is appended to data/mapping/spark/ledger.jsonl;
finished job attempts go to data/mapping/spark/<run>/replies.jsonl and
make reruns resumable (same request hash -> returned from cache).
"""

import argparse
import hashlib
import json
import os
import random
import subprocess
import sys
import threading
import time
import uuid
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

ENDPOINT = "https://inference.baseten.co/v1/chat/completions"
MODEL = "meta-models/muse-spark-open-1026"
PRICE_IN = 1.25e-6          # $/token, uncached input
PRICE_IN_CACHED = 0.15e-6   # $/token, cached input
PRICE_OUT = 4.25e-6         # $/token, output incl. reasoning
CHARS_PER_TOKEN = 3.2       # for the up-front estimate
DEFAULT_CAP_ENV = "SYNTHEXIS_BASETEN_CAP"
DEFAULT_CAP_USD = 60.0
MIN_REQUEST_INTERVAL_S = 1.2   # <= 50 req/min per session-affinity id
REQUEST_TIMEOUT_S = 600.0
HTTP_MAX_TRIES = 6

ROOT = Path(__file__).resolve().parents[2]
DATA_DIR = Path(
    os.environ.get("SYNTHEXIS_SPARK_DIR", ROOT / "data" / "mapping" / "spark"))


class ParseError(Exception):
    """parse() raises this for an unusable reply -> job is retried."""


class AbortError(Exception):
    """Raised when a cost guard refuses or aborts a run (CLI: exit 2)."""


@dataclass
class Job:
    id: str                  # stable, unique within a run
    messages: list[dict]     # full chat messages
    est_out_tokens: int      # caller's output estimate for guard 1


def _sleep(s: float) -> None:
    time.sleep(s)


_api_key: str | None = None
_client = None
_client_lock = threading.Lock()


def _key() -> str:
    global _api_key
    if _api_key is None:
        r = subprocess.run(
            ["security", "find-generic-password", "-s",
             "synthexis.baseten", "-w"],
            capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(
                "keychain item 'synthexis.baseten' unavailable")
        _api_key = r.stdout.strip()
    return _api_key


def _http() -> Any:
    global _client
    with _client_lock:
        if _client is None:
            import httpx
            _client = httpx.Client(
                timeout=httpx.Timeout(REQUEST_TIMEOUT_S))
        return _client


def _http_transport(body: dict, session_id: str, timeout_s: float):
    """Default transport: real POST. Returns (status, headers, payload)."""
    resp = _http().post(
        ENDPOINT,
        json=body,
        headers={
            "Authorization": f"Bearer {_key()}",
            "Content-Type": "application/json",
            "x-session-affinity": session_id,
        },
        timeout=timeout_s,
    )
    try:
        payload = resp.json()
    except ValueError:
        payload = {"_raw": resp.text[:2000]}
    return resp.status_code, dict(resp.headers), payload


def _cost(usage: dict) -> float:
    pt = int(usage.get("prompt_tokens") or 0)
    ct = int(usage.get("completion_tokens") or 0)
    cached = int((usage.get("prompt_tokens_details") or {})
                 .get("cached_tokens") or 0)
    return (pt - cached) * PRICE_IN + cached * PRICE_IN_CACHED \
        + ct * PRICE_OUT


def _reasoning_tokens(usage: dict) -> int:
    return int((usage.get("completion_tokens_details") or {})
               .get("reasoning_tokens") or 0)


def _content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "".join(
            p.get("text", "") for p in content if isinstance(p, dict))
    return ""


def _estimate_usd(job: Job) -> float:
    in_tok = len(json.dumps(job.messages, ensure_ascii=False)) / CHARS_PER_TOKEN
    return in_tok * PRICE_IN + (job.est_out_tokens + 100) * PRICE_OUT


def _req_hash(model: str, messages: list, temperature, reasoning_effort,
              max_tokens) -> str:
    canon = json.dumps(
        [model, messages, temperature, reasoning_effort, max_tokens],
        sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canon.encode()).hexdigest()


def _run_dir(run_name: str) -> Path:
    return DATA_DIR / run_name


def _ledger_path() -> Path:
    return DATA_DIR / "ledger.jsonl"


_ledger_lock = threading.Lock()
_replies_lock = threading.Lock()


def _ledger_append(line: dict) -> None:
    with _ledger_lock:
        DATA_DIR.mkdir(parents=True, exist_ok=True)
        with _ledger_path().open("a") as f:
            f.write(json.dumps(line) + "\n")


def _ledger_total_usd() -> float:
    p = _ledger_path()
    if not p.exists():
        return 0.0
    total = 0.0
    for line in p.read_text().splitlines():
        try:
            total += float(json.loads(line).get("cost_usd") or 0)
        except json.JSONDecodeError:
            continue
    return total


class _Spend:
    """Shared spend/progress counters."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.spent = 0.0
        self.done = 0          # finished new jobs
        self.in_flight = 0
        self.req_ts: deque = deque()
        self.prompt_tokens = 0
        self.cached_tokens = 0
        self.stop = False

    def mean_cost(self, fallback: float) -> float:
        return self.spent / self.done if self.done else fallback

    def add_attempt(self, cost: float, prompt: int, cached: int) -> None:
        with self.lock:
            self.spent += cost
            self.prompt_tokens += prompt
            self.cached_tokens += cached
            self.req_ts.append(time.monotonic())
            cutoff = time.monotonic() - 120
            while self.req_ts and self.req_ts[0] < cutoff:
                self.req_ts.popleft()

    def rpm(self) -> float:
        with self.lock:
            if not self.req_ts:
                return 0.0
            span = time.monotonic() - self.req_ts[0]
            return len(self.req_ts) / span * 60 if span > 1 else 0.0

    def cache_ratio(self) -> float:
        with self.lock:
            return (self.cached_tokens / self.prompt_tokens
                    if self.prompt_tokens else 0.0)


def _send(body: dict, session_id: str, transport: Callable,
          on_http: Callable | None = None):
    """One HTTP call with backoff on 429/529/5xx/transport errors.

    Returns (status, headers, payload, latency_s, error). on_http is
    invoked once per try (ledger) with (status, usage, latency_s,
    error).
    """
    delay = 2.0
    last: tuple = (0, {}, None, 0.0, "no tries")
    for i in range(HTTP_MAX_TRIES):
        t0 = time.monotonic()
        status, headers, payload, err = 0, {}, None, None
        try:
            status, headers, payload = transport(
                body, session_id, REQUEST_TIMEOUT_S)
            if status == 200 and isinstance(payload, dict) \
                    and payload.get("choices"):
                err = None
            elif status in (429, 529) or status >= 500:
                err = f"http {status}"
            else:
                err = f"http {status} (fatal)"
        except Exception as e:  # timeouts, connection errors
            err = f"{type(e).__name__}: {str(e)[:200]}"
        lat = time.monotonic() - t0
        usage = (payload or {}).get("usage") or {} \
            if isinstance(payload, dict) else {}
        if on_http is not None:
            on_http(status, usage, lat, err)
        if err is None:
            return status, headers, payload, lat, None
        last = (status, headers, payload, lat, err)
        retryable = err.startswith("http 429") or err.startswith("http 529") \
            or (status >= 500) or status == 0
        if not retryable or i == HTTP_MAX_TRIES - 1:
            break
        _sleep(delay + random.uniform(0, delay * 0.25))
        delay = min(delay * 2, 32.0)
    status, headers, payload, lat, err = last
    return status, headers, payload, lat, err


def _attempt(job: Job, req_hash: str, attempt_no: int, *,
             run_name: str, session_id: str, transport: Callable,
             parse: Callable, replies, temperature,
             reasoning_effort, max_tokens: int,
             spend: _Spend) -> tuple[bool, Any]:
    """Send once (with HTTP backoff) + parse + reply record.

    Returns (ok, retryable_failure, parsed).
    """
    body = {"model": MODEL, "messages": job.messages,
            "max_tokens": max_tokens}
    if temperature is not None:
        body["temperature"] = temperature
    if reasoning_effort is not None:
        body["reasoning_effort"] = reasoning_effort

    def on_http(status, usage, lat, err):
        line = {
            "ts": round(time.time(), 3), "run": run_name, "job_id": job.id,
            "http_status": status,
            "prompt_tokens": int(usage.get("prompt_tokens") or 0),
            "cached_tokens": int(
                (usage.get("prompt_tokens_details") or {})
                .get("cached_tokens") or 0),
            "completion_tokens": int(usage.get("completion_tokens") or 0),
            "reasoning_tokens": _reasoning_tokens(usage),
            "cost_usd": round(_cost(usage), 8),
            "latency_s": round(lat, 3),
        }
        _ledger_append(line)
        spend.add_attempt(
            line["cost_usd"], line["prompt_tokens"],
            line["cached_tokens"])

    status, _headers, payload, lat, err = _send(
        body, session_id, transport, on_http)

    usage = payload.get("usage") if isinstance(payload, dict) else {}
    usage = usage or {}
    text = ""
    reasoning_chars = 0
    finish = None
    if err is None:
        msg = payload["choices"][0].get("message") or {}
        text = _content_text(msg.get("content"))
        rtxt = msg.get("reasoning") or msg.get("reasoning_content") or ""
        reasoning_chars = len(rtxt) if isinstance(rtxt, str) else 0
        finish = payload["choices"][0].get("finish_reason")

    parsed = None
    ok = False
    retryable = False
    perr = err
    if err is None:
        if finish == "length":
            perr = "finish_reason=length"
            retryable = True
        else:
            try:
                parsed = parse(job, text)
                ok = True
            except ParseError as e:
                perr = f"ParseError: {str(e)[:200]}"
                retryable = True
            except Exception as e:
                perr = f"parse {type(e).__name__}: {str(e)[:200]}"

    rec = {
        "job_id": job.id, "req_hash": req_hash, "attempt": attempt_no,
        "ok": ok, "parsed": parsed, "text": text,
        "reasoning_tokens": _reasoning_tokens(usage),
        "reasoning_chars": reasoning_chars,
        "usage": usage, "cost_usd": round(_cost(usage), 8),
        "latency_s": round(lat, 3), "finish_reason": finish,
        "error": perr, "ts": round(time.time(), 3),
    }
    with _replies_lock:
        replies.write(json.dumps(rec) + "\n")
        replies.flush()
    return ok, retryable, parsed


def run(run_name: str, jobs: list[Job], parse: Callable[[Job, str], Any],
        *, max_usd: float, workers: int = 8, pilot: int = 3,
        temperature: float | None = None,
        reasoning_effort: str | None = None,
        max_tokens: int = 32000, retries: int = 2,
        transport: Callable | None = None) -> dict[str, Any]:
    """job_id -> parsed value for cached + newly succeeded jobs."""
    transport = transport or _http_transport
    run_dir = _run_dir(run_name)
    run_dir.mkdir(parents=True, exist_ok=True)
    replies_path = run_dir / "replies.jsonl"

    hashes = {
        j.id: _req_hash(MODEL, j.messages, temperature,
                        reasoning_effort, max_tokens)
        for j in jobs
    }
    cached_ok: dict[str, dict] = {}
    if replies_path.exists():
        for line in replies_path.read_text().splitlines():
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            if rec.get("ok"):
                cached_ok[rec["job_id"]] = rec

    results: dict[str, Any] = {}
    uncached: list[Job] = []
    for job in jobs:
        rec = cached_ok.get(job.id)
        if rec and rec.get("req_hash") == hashes[job.id]:
            results[job.id] = rec["parsed"]
        else:
            uncached.append(job)

    est = sum(_estimate_usd(j) for j in uncached)
    print(f"[{run_name}] {len(jobs)} jobs: {len(results)} cached, "
          f"{len(uncached)} to send, up-front estimate ${est:.3f}",
          flush=True)
    if est > max_usd:
        raise AbortError(
            f"up-front estimate ${est:.3f} > --max-usd ${max_usd}")
    cap = float(os.environ.get(DEFAULT_CAP_ENV, DEFAULT_CAP_USD))
    spent_prev = _ledger_total_usd()
    if spent_prev + est > cap:
        raise AbortError(
            f"ledger total ${spent_prev:.2f} + estimate ${est:.3f} "
            f"> ${DEFAULT_CAP_ENV}=${cap}")
    if not uncached:
        return results

    spend = _Spend()
    replies = replies_path.open("a")
    total = len(jobs)
    est_per_job = est / len(uncached)

    def worker(k: int, queue: list[Job]) -> None:
        sid = str(uuid.uuid5(
            uuid.NAMESPACE_URL, f"synthexis-{run_name}-w{k}"))
        last_send = 0.0
        while True:
            with spend.lock:
                if spend.stop or not queue:
                    return
                mean = spend.mean_cost(est_per_job)
                if spend.spent + spend.in_flight * mean >= max_usd:
                    spend.stop = True
                    return
                job = queue.pop(0)
                spend.in_flight += 1
            try:
                # pace this session id to <= 50 req/min
                gap = time.monotonic() - last_send
                if gap < MIN_REQUEST_INTERVAL_S:
                    _sleep(MIN_REQUEST_INTERVAL_S - gap)
                attempt = 0
                while True:
                    attempt += 1
                    last_send = time.monotonic()
                    ok, retryable, parsed = _attempt(
                        job, hashes[job.id], attempt,
                        run_name=run_name, session_id=sid,
                        transport=transport, parse=parse,
                        replies=replies, temperature=temperature,
                        reasoning_effort=reasoning_effort,
                        max_tokens=max_tokens, spend=spend)
                    if retryable and attempt <= retries:
                        continue
                    if ok:
                        results[job.id] = parsed
                    break
                with spend.lock:
                    spend.done += 1
                    d = spend.done
                if d % 10 == 0 or d == len(uncached):
                    done_total = (total - len(uncached)) + d
                    print(
                        f"[{run_name}] {done_total}/{total} spent "
                        f"${spend.spent:.3f} mean "
                        f"${spend.mean_cost(0):.4f}/job rpm "
                        f"{spend.rpm():.0f} cache-hit "
                        f"{spend.cache_ratio():.0%}", flush=True)
            finally:
                with spend.lock:
                    spend.in_flight -= 1

    # --- pilot: first `pilot` uncached jobs, then re-estimate -------
    pilot_jobs = uncached[:pilot]
    rest = uncached[pilot:]
    try:
        with ThreadPoolExecutor(max_workers=max(1, workers)) as ex:
            if pilot_jobs:
                futs = [
                    ex.submit(worker, k, [j])
                    for k, j in enumerate(pilot_jobs)
                ]
                for f in futs:
                    f.result()
                reest = spend.spent + spend.mean_cost(est_per_job) \
                    * len(rest)
                print(
                    f"[{run_name}] pilot spent ${spend.spent:.3f} -> "
                    f"re-estimate ${reest:.3f} "
                    f"(up-front ${est:.3f})", flush=True)
                if reest > max_usd:
                    raise AbortError(
                        f"pilot re-estimate ${reest:.3f} > "
                        f"--max-usd ${max_usd}")
                if reest > 1.5 * est:
                    raise AbortError(
                        f"pilot re-estimate ${reest:.3f} > 1.5x "
                        f"up-front estimate ${est:.3f}")
            # --- rolling dispatch under budget ----------------------
            queue = list(rest)
            futs = [
                ex.submit(worker, pilot + k, queue)
                for k in range(max(1, workers))
            ]
            for f in futs:
                f.result()
    finally:
        replies.close()

    with spend.lock:
        leftover = spend.stop
    if leftover:
        done_ids = set(results)
        n_left = sum(1 for j in uncached if j.id not in done_ids)
        print(
            f"[{run_name}] stopped at cap, {n_left} jobs left, "
            f"rerun to resume", flush=True)
    return results


# --- CLI --------------------------------------------------------------------


def _cli_ledger() -> None:
    p = _ledger_path()
    runs: dict[str, dict] = {}
    if p.exists():
        for line in p.read_text().splitlines():
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            agg = runs.setdefault(r["run"], {
                "requests": 0, "prompt_tokens": 0, "cached_tokens": 0,
                "completion_tokens": 0, "reasoning_tokens": 0,
                "cost_usd": 0.0})
            agg["requests"] += 1 if r.get("http_status") == 200 else 0
            for k in ("prompt_tokens", "cached_tokens",
                      "completion_tokens", "reasoning_tokens"):
                agg[k] += int(r.get(k) or 0)
            agg["cost_usd"] += float(r.get("cost_usd") or 0)
    total = {"requests": 0, "prompt_tokens": 0, "cached_tokens": 0,
             "completion_tokens": 0, "reasoning_tokens": 0,
             "cost_usd": 0.0}
    for run, a in sorted(runs.items()):
        print(
            f"{run:<24} req={a['requests']:<5} prompt={a['prompt_tokens']}"
            f" cached={a['cached_tokens']} out={a['completion_tokens']}"
            f" reasoning={a['reasoning_tokens']} ${a['cost_usd']:.4f}")
        for k in total:
            total[k] += a[k]
    print(
        f"{'TOTAL':<24} req={total['requests']:<5} "
        f"prompt={total['prompt_tokens']} cached={total['cached_tokens']}"
        f" out={total['completion_tokens']} "
        f"reasoning={total['reasoning_tokens']} ${total['cost_usd']:.4f}")


def _cli_probe() -> None:
    sent = (
        "You are a deterministic throughput probe for the synthexis "
        "mapping assistant. Answer tersely. ")
    while len(sent) / CHARS_PER_TOKEN < 1500:
        sent += sent
    system = sent[: int(1500 * CHARS_PER_TOKEN)]
    body = {
        "model": MODEL,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": "Reply with OK"},
        ],
        "max_tokens": 512,
    }
    sid = str(uuid.uuid5(uuid.NAMESPACE_URL, "synthexis-probe"))
    print(f"probe: ~{len(system) / CHARS_PER_TOKEN:.0f} input tokens, "
          f"session {sid[:8]}")
    RL_KEYS = [
        "x-ratelimit-limit-requests", "x-ratelimit-remaining-requests",
        "x-ratelimit-limit-tokens", "x-ratelimit-remaining-tokens",
    ]
    for i in (1, 2):
        status, headers, payload, lat, err = _send(
            body, sid, _http_transport)
        usage = payload.get("usage", {}) if isinstance(payload, dict) else {}
        cost = _cost(usage)
        _ledger_append({
            "ts": round(time.time(), 3), "run": "probe",
            "job_id": f"probe-{i}", "http_status": status,
            "prompt_tokens": int(usage.get("prompt_tokens") or 0),
            "cached_tokens": int(
                (usage.get("prompt_tokens_details") or {})
                .get("cached_tokens") or 0),
            "completion_tokens": int(usage.get("completion_tokens") or 0),
            "reasoning_tokens": _reasoning_tokens(usage),
            "cost_usd": round(cost, 8), "latency_s": round(lat, 3),
        })
        if err:
            print(f"req{i}: error {err}")
            continue
        print(
            f"req{i}: {lat:.2f}s prompt={usage.get('prompt_tokens')} "
            f"cached={(usage.get('prompt_tokens_details') or {}).get('cached_tokens', 0)} "
            f"out={usage.get('completion_tokens')} "
            f"reasoning={_reasoning_tokens(usage)} ${cost:.5f}")
        for k in RL_KEYS:
            print(f"      {k}: {headers.get(k, '-')}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["ledger", "probe"])
    args = ap.parse_args()
    try:
        if args.cmd == "ledger":
            _cli_ledger()
        else:
            _cli_probe()
    except AbortError as e:
        print(f"refused: {e}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
