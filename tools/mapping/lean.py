# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf"]
# ///
"""Cost probes for the v10 lean parser: size, latency, weight averaging,
weight-only quantisation.

  uv run tools/mapping/lean.py cost --runs p-v10-s0,p-v10-s1 [--ds ds-v10c]
  uv run tools/mapping/lean.py soup --runs p-v10-s0,...,p-v10-s3 --out lean-soup4
  uv run tools/mapping/lean.py quant --runs p-v11-s0,p-v11-s1 --bits 8
      (writes <run>-q8 per run; --out names the dir for a single run)

cost: f16 weight bytes and median / p95 ms per sentence (tokenize, encode,
constrained greedy; an ensemble decodes as frame_parser.beam at k 1) over
the first 200 rows of dev, one CPU thread, as export_hf.py measures.
soup: a run dir whose model.pt is the mean of the runs' weights.
quant: a run dir whose model.pt holds the f16 weights after a round trip
through ggml Q8_0 or Q4_0 (blocks of 32 along the input dimension, f16
scale), as candle stores them, for every 2-d encoder matrix and the
embedding; the frame decoder stays f16. decode_eval.py scores these dirs
like any run. Bytes count Q8_0 as 34 and Q4_0 as 18 per 32 weights.
"""

import argparse
import json
import shutil
import statistics
import sys
import time
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

RUNS = Path(__file__).resolve().parents[2] / "data" / "mapping" / "train" / "runs"
QBYTES = {8: 34 / 32, 4: 18 / 32}


def cost(a) -> None:
    torch.set_num_threads(1)
    cpu = torch.device("cpu")
    loaded = [fp.load_run(RUNS / r) for r in a.runs.split(",")]
    models = [m.eval() for m, _, _ in loaded]
    sp, vocab = loaded[0][1], loaded[0][2]
    grammar = fp.Grammar(vocab)
    tok_id = {t: i for i, t in enumerate(vocab)}
    dev, _ = fp.load(sp, RUNS.parent / a.ds / "dev.jsonl", tok_id)
    ms = []
    with torch.no_grad():
        for s in dev[:200]:
            t0 = time.perf_counter()
            ids, offs = fp.tokenize(sp, s["text"])
            x = [{"text": s["text"], "ids": ids, "offs": offs}]
            if len(models) == 1:
                fp.greedy(models[0], grammar, x, cpu)
            else:
                fp.beam(models, grammar, x, cpu, 1, 0.0)
            ms.append((time.perf_counter() - t0) * 1000)
    ms.sort()
    n = sum(p.numel() for m in models for p in m.state_dict().values())
    out = {"runs": a.runs, "params_M": round(n / 1e6, 1),
           "f16_MB": round(2 * n / 1e6, 1),
           "median_ms": round(statistics.median(ms), 1),
           "p95_ms": round(ms[int(len(ms) * 0.95)], 1)}
    print(json.dumps(out))


def new_run(src: str, name: str, sd: dict, note: dict) -> None:
    out = RUNS / name
    out.mkdir(parents=True, exist_ok=True)
    shutil.copy(RUNS / src / "meta.json", out / "meta.json")
    torch.save(sd, out / "model.pt")
    (out / "lean.json").write_text(json.dumps(note, indent=1))
    print(name, json.dumps(note))


def soup(a) -> None:
    runs = a.runs.split(",")
    sds = [torch.load(RUNS / r / "model.pt", map_location="cpu") for r in runs]
    sd = {k: sum(s[k].float() for s in sds) / len(sds)
          if sds[0][k].is_floating_point() else sds[0][k] for k in sds[0]}
    new_run(runs[0], a.out, sd, {"soup": runs})


def ggml_round_trip(w: torch.Tensor, bits: int) -> torch.Tensor:
    """Quantise and dequantise rows of w in blocks of 32 as candle's
    from_float does: the f32 scale quantises, its f16 copy dequantises."""
    b = w.float().reshape(-1, 32)
    if bits == 8:
        d = b.abs().amax(1, keepdim=True) / 127
        q = torch.where(d > 0, b / d, torch.zeros_like(b)).round()
        return (q * d.half().float()).reshape(w.shape)
    idx = b.abs().argmax(1, keepdim=True)
    d = b.gather(1, idx) / -8
    q = torch.where(d != 0, b / d, torch.zeros_like(b))
    q = (q + 8.5).floor().clamp(0, 15)
    return ((q - 8) * d.half().float()).reshape(w.shape)


def quant(a) -> None:
    runs = (a.runs or a.run).split(",")
    for run in runs:
        quant_run(run, a.bits, a.out if len(runs) == 1 else "")


def quant_run(run: str, bits: int, name: str) -> None:
    sd = torch.load(RUNS / run / "model.pt", map_location="cpu")
    out, qn, rest = {}, 0, 0
    for k, v in sd.items():
        if k.startswith("hf.") and v.dim() == 2 and v.shape[-1] % 32 == 0:
            out[k] = ggml_round_trip(v.half(), bits)  # runtime holds f16
            qn += v.numel()
        else:
            out[k] = v.half().float() if v.is_floating_point() else v
            rest += v.numel()
    mb = (qn * QBYTES[bits] + 2 * rest) / 1e6
    new_run(run, name or f"{run}-q{bits}", out,
            {"from": run, "bits": bits, "quantised_M": round(qn / 1e6, 1),
             "f16_M": round(rest / 1e6, 1), "MB": round(mb, 1)})


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=("cost", "soup", "quant"))
    ap.add_argument("--runs", default="")
    ap.add_argument("--run", default="")
    ap.add_argument("--ds", default="ds-v10c")
    ap.add_argument("--out", default="")
    ap.add_argument("--bits", type=int, default=8)
    a = ap.parse_args()
    {"cost": cost, "soup": soup, "quant": quant}[a.cmd](a)


if __name__ == "__main__":
    main()
