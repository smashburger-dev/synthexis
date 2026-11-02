# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Measure how many prompt tokens Muse Spark Open bills per image.

  uv run probes/budget-motion-labels/image_tokens.py

Sends a text-only baseline and synthetic noise PNGs (no recorded frames)
at frame-strip sizes, one request each, and prints prompt tokens minus
the baseline. Requests go through tools/mapping/spark.py, so they land
in data/mapping/spark/ledger.jsonl under run "image-probe".
"""

import base64
import random
import struct
import sys
import time
import uuid
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "mapping"))
import spark  # noqa: E402

SIZES = [(384, 216), (768, 432), (1536, 216), (1280, 720)]
TEXT = "Describe the image in five words."


def png(w: int, h: int, seed: int) -> bytes:
    rng = random.Random(seed)
    rows = b"".join(b"\x00" + bytes(rng.getrandbits(8) for _ in range(w * 3))
                    for _ in range(h))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xffffffff))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))


def ask(content, label: str) -> dict:
    body = {"model": spark.MODEL, "max_tokens": 2000,
            "messages": [{"role": "user", "content": content}]}
    sid = str(uuid.uuid5(uuid.NAMESPACE_URL, f"synthexis-image-probe-{label}"))
    status, _h, payload, lat, err = spark._send(body, sid, spark._http_transport)
    usage = payload.get("usage", {}) if isinstance(payload, dict) else {}
    spark._ledger_append({
        "ts": round(time.time(), 3), "run": "image-probe", "job_id": label,
        "http_status": status,
        "prompt_tokens": int(usage.get("prompt_tokens") or 0),
        "cached_tokens": int((usage.get("prompt_tokens_details") or {})
                             .get("cached_tokens") or 0),
        "completion_tokens": int(usage.get("completion_tokens") or 0),
        "reasoning_tokens": spark._reasoning_tokens(usage),
        "cost_usd": round(spark._cost(usage), 8), "latency_s": round(lat, 3)})
    if err:
        print(f"{label}: error {err}")
    return usage


def main() -> None:
    base = ask(TEXT, "text")
    b0 = int(base.get("prompt_tokens") or 0)
    print(f"text only: prompt {b0} out {base.get('completion_tokens')}")
    for i, (w, h) in enumerate(SIZES):
        url = "data:image/png;base64," + base64.b64encode(png(w, h, i)).decode()
        u = ask([{"type": "text", "text": TEXT},
                 {"type": "image_url", "image_url": {"url": url}}], f"{w}x{h}")
        p = int(u.get("prompt_tokens") or 0)
        print(f"{w}x{h}: prompt {p} -> image {p - b0} tokens, "
              f"out {u.get('completion_tokens')}, ${spark._cost(u):.5f}")


if __name__ == "__main__":
    main()
