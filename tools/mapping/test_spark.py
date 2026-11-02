# /// script
# requires-python = ">=3.11"
# ///
"""Unit tests for spark.py with an injected fake transport (no network).

  uv run --with pytest --with httpx pytest tools/mapping/test_spark.py
"""

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spark  # noqa: E402


@pytest.fixture(autouse=True)
def tmp_data(tmp_path, monkeypatch):
    monkeypatch.setattr(spark, "DATA_DIR", tmp_path)
    monkeypatch.setattr(spark, "MIN_REQUEST_INTERVAL_S", 0)
    monkeypatch.setattr(spark, "_sleep", lambda s: None)


def job(i, est_out=10, content="say hi"):
    return spark.Job(
        id=f"j{i}",
        messages=[{"role": "user", "content": content}],
        est_out_tokens=est_out)


def ok_payload(usage=None):
    u = {"prompt_tokens": 100, "completion_tokens": 10,
         "prompt_tokens_details": {"cached_tokens": 0},
         "completion_tokens_details": {"reasoning_tokens": 5}}
    u.update(usage or {})
    return {"choices": [{"message": {"content": "txt"},
                         "finish_reason": "stop"}], "usage": u}


class FakeTransport:
    """statuses: queue of HTTP statuses (default 200 forever)."""

    def __init__(self, statuses=None, usage=None):
        self.calls = 0
        self.bodies = []
        self.statuses = list(statuses or [])
        self.usage = usage or {}

    def __call__(self, body, session_id, timeout_s):
        self.calls += 1
        self.bodies.append(body)
        status = self.statuses.pop(0) if self.statuses else 200
        if status == 200:
            return 200, {}, ok_payload(self.usage)
        return status, {"x-ratelimit-remaining-requests": "0"}, \
            {"error": {"message": "boom"}}


def test_upfront_refuse():
    jobs = [job(0, est_out=2_000_000)]
    with pytest.raises(spark.AbortError, match="up-front"):
        spark.run("t", jobs, lambda j, t: t, max_usd=0.01,
                  transport=FakeTransport())


def test_global_cap_refuse(tmp_path, monkeypatch):
    ledger = tmp_path / "ledger.jsonl"
    ledger.write_text(json.dumps(
        {"run": "old", "cost_usd": 60.0, "http_status": 200}) + "\n")
    monkeypatch.setenv("SYNTHEXIS_BASETEN_CAP", "60")
    with pytest.raises(spark.AbortError, match="ledger"):
        spark.run("t", [job(0)], lambda j, t: t, max_usd=50,
                  transport=FakeTransport())


def test_pilot_abort():
    # every request bills 100k completion tokens = $0.425
    t = FakeTransport(usage={"completion_tokens": 100_000})
    with pytest.raises(spark.AbortError, match="pilot"):
        spark.run("t", [job(i) for i in range(10)],
                  lambda j, t: t, max_usd=0.5, pilot=2, transport=t)
    assert t.calls == 2  # only the pilot jobs were sent


def test_rolling_stop(capsys):
    # each job bills 80k prompt tokens = $0.10; cap $0.35
    t = FakeTransport(usage={"prompt_tokens": 80_000})
    res = spark.run("t", [job(i) for i in range(20)],
                    lambda j, t: t, max_usd=0.35, pilot=0,
                    workers=2, transport=t)
    assert 1 <= len(res) <= 7
    assert t.calls == len(res)
    assert "stopped at cap" in capsys.readouterr().out


def test_cache_resume(tmp_path):
    jobs = [job(i) for i in range(3)]
    t1 = FakeTransport()
    r1 = spark.run("t", jobs, lambda j, t: f"P:{t}", max_usd=5,
                   pilot=0, workers=2, transport=t1)
    assert len(r1) == 3 and t1.calls == 3
    t2 = FakeTransport()
    r2 = spark.run("t", jobs, lambda j, t: f"P:{t}", max_usd=5,
                   pilot=0, workers=2, transport=t2)
    assert r2 == r1 and t2.calls == 0


def test_parse_retry_then_failed(tmp_path):
    def bad(job, text):
        raise spark.ParseError("nope")
    t = FakeTransport()
    res = spark.run("t", [job(0)], bad, max_usd=5, pilot=0,
                    retries=2, transport=t)
    assert res == {}
    assert t.calls == 3  # 1 + 2 retries
    replies = (tmp_path / "t" / "replies.jsonl").read_text().splitlines()
    assert len(replies) == 3
    assert all(not json.loads(l)["ok"] for l in replies)


def test_backoff_429(monkeypatch):
    sleeps = []
    monkeypatch.setattr(spark, "_sleep", lambda s: sleeps.append(s))
    t = FakeTransport(statuses=[429, 529, 200])
    res = spark.run("t", [job(0)], lambda j, t: t, max_usd=5,
                    pilot=0, transport=t)
    assert res == {"j0": "txt"}
    assert t.calls == 3
    assert len(sleeps) == 2
    assert 2 <= sleeps[0] <= 3 and 4 <= sleeps[1] <= 5


def test_finish_reason_length_is_retried(tmp_path):
    calls = {"n": 0}

    def tr(body, session_id, timeout_s):
        calls["n"] += 1
        p = ok_payload()
        p["choices"][0]["finish_reason"] = "length"
        return 200, {}, p

    res = spark.run("t", [job(0)], lambda j, t: t, max_usd=5,
                    pilot=0, retries=1, transport=tr)
    assert res == {} and calls["n"] == 2


def test_estimate_counts_images_by_part_not_base64_length():
    big = "data:image/jpeg;base64," + "A" * 200_000
    content = [{"type": "text", "text": "look"},
               {"type": "image_url", "image_url": {"url": big}}]
    with_img = spark._estimate_usd(job(0, content=content))
    text_only = spark._estimate_usd(job(1, content=[content[0]]))
    extra = (with_img - text_only) / spark.PRICE_IN
    assert spark.IMAGE_TOKENS_EST * 0.9 < extra < spark.IMAGE_TOKENS_EST * 1.1
