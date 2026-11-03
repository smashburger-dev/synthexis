# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf", "onnx", "onnxruntime",
#                 "onnxconverter-common", "onnxscript", "psutil"]
# ///
"""ONNX Runtime as the alternative plugin runtime: encoder latency and RSS.

  uv run probes/parser-runtime/ort_probe.py --run p-v7a-s0

Exports the run's encoder (ModernBERT + enc_norm, shipped f16 weights) to
ONNX in f32 and in f16, runs each with onnxruntime on one intra-op thread
over the 200 parity sentences (data/mapping/runtime/parity/dev200.txt) and
prints median and p95 per sentence plus the RSS the session adds. The `ort`
crate wraps the same C library, so these numbers carry over to Rust. The
encoder is about three quarters of a parse in the candle runtime.
"""

import argparse
import gc
import json
import statistics
import sys
import time
from pathlib import Path

import numpy as np
import psutil
import torch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import frame_parser as fp  # noqa: E402

TRAIN = ROOT / "data" / "mapping" / "train"
OUT = ROOT / "data" / "mapping" / "runtime" / "onnx"


class Enc(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, ids):
        h = self.m.hf(input_ids=ids,
                      attention_mask=torch.ones_like(ids)).last_hidden_state
        return self.m.enc_norm(h)


def rss_mb() -> float:
    return psutil.Process().memory_info().rss / 2**20


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v7a-s0")
    a = ap.parse_args()
    import onnxruntime as ort
    from onnxconverter_common import float16

    model, sp, _ = fp.load_run(TRAIN / "runs" / a.run)
    sd = torch.load(TRAIN / "runs" / a.run / "model.f16.pt", map_location="cpu")
    model.load_state_dict({k: v.float() for k, v in sd.items()})
    model.hf.config._attn_implementation = "eager"
    model.eval()
    texts = (ROOT / "data/mapping/runtime/parity/dev200.txt").read_text() \
        .splitlines()
    ids = [fp.tokenize(sp, t)[0][:fp.MAX_SRC - 1] for t in texts]
    OUT.mkdir(parents=True, exist_ok=True)
    f32 = OUT / f"{a.run}.enc.f32.onnx"
    torch.onnx.export(Enc(model), (torch.tensor([ids[0]]),), f32,
                      input_names=["ids"], output_names=["h"],
                      dynamic_axes={"ids": {1: "s"}, "h": {1: "s"}},
                      opset_version=17, dynamo=False)
    import onnx
    f16 = OUT / f"{a.run}.enc.f16.onnx"
    g = onnx.load(f32)
    keep = [n.name for n in g.graph.node  # RoPE and mask casts break in f16
            if "rotary_emb" in n.name or n.op_type == "Cast"]
    onnx.save(float16.convert_float_to_float16(
        g, keep_io_types=True, node_block_list=keep), f16)
    with torch.no_grad():
        ref = [Enc(model)(torch.tensor([x]))[0].numpy() for x in ids[:20]]
    del model, sd
    gc.collect()
    res = {}
    for name, path in (("f32", f32), ("f16", f16)):
        so = ort.SessionOptions()
        so.intra_op_num_threads = 1
        so.inter_op_num_threads = 1
        r0 = rss_mb()
        s = ort.InferenceSession(str(path), so,
                                 providers=["CPUExecutionProvider"])
        r1 = rss_mb()
        err = max(float(np.abs(s.run(None, {"ids": np.array([x])})[0][0]
                               - r).max()) for x, r in zip(ids, ref))
        ms = []
        for x in ids:
            t0 = time.perf_counter()
            s.run(None, {"ids": np.array([x], dtype=np.int64)})
            ms.append((time.perf_counter() - t0) * 1e3)
        ms.sort()
        res[name] = {"file_mb": round(path.stat().st_size / 2**20, 1),
                     "session_rss_mb": round(r1 - r0, 1),
                     "median_ms": round(statistics.median(ms), 1),
                     "p95_ms": round(ms[int(len(ms) * 0.95)], 1),
                     "max_abs_diff_vs_torch": round(err, 5)}
        del s
        gc.collect()
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
