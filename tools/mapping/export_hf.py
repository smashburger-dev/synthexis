# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf"]
# ///
"""CPU size, latency and int8 accuracy of a backbone parser run (v7).

  uv run tools/mapping/export_hf.py --run p-v7-s0 --ds ds-v7p

Dynamic int8 quantization of every nn.Linear (torch.ao), as a stand-in for
the plugin runtime, which does not load backbone models yet. Measures:
size of the f16 and int8 state dicts, median and p95 per sentence
(tokenize + encode + constrained greedy) over 200 dev sentences at one
thread for f32 and int8, and dev route_edit of f32, f16-rounded weights
and int8. Writes export_hf.json, model.f16.pt and model.int8.pt into the
run dir.
"""

import argparse
import copy
import io
import json
import statistics
import sys
import time
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

TRAIN = Path(__file__).resolve().parents[2] / "data" / "mapping" / "train"


def nbytes(sd: dict) -> int:
    buf = io.BytesIO()
    torch.save(sd, buf)
    return buf.tell()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--ds", default="ds-v7p")
    a = ap.parse_args()
    torch.set_num_threads(1)
    run = TRAIN / "runs" / a.run
    model, sp, vocab = fp.load_run(run)
    model.eval()
    tok_id = {t: i for i, t in enumerate(vocab)}
    dev, _ = fp.load(sp, TRAIN / a.ds / "dev.jsonl", tok_id)
    grammar = fp.Grammar(vocab)
    cpu = torch.device("cpu")
    if "qnnpack" in torch.backends.quantized.supported_engines:
        torch.backends.quantized.engine = "qnnpack"     # arm64 CPUs
    q = torch.ao.quantization.quantize_dynamic(model, {torch.nn.Linear},
                                               dtype=torch.qint8)
    out = {"bytes": {
        "f16": nbytes({k: v.half() if v.is_floating_point() else v
                       for k, v in model.state_dict().items()}),
        "int8": nbytes(q.state_dict())}}
    h = copy.deepcopy(model)
    with torch.no_grad():
        for p in h.parameters():
            p.copy_(p.half().float())
        for name, m in (("f32", model), ("f16", h), ("int8", q)):
            met, _ = fp.evaluate(m, grammar, dev, vocab, cpu)
            out[f"{name}_dev_route_edit"] = met["route_edit"]["acc"]
        for name, m in (("f32", model), ("int8", q)):
            ms = []
            for s in dev[:200]:
                t0 = time.perf_counter()
                ids, offs = fp.tokenize(sp, s["text"])
                fp.greedy(m, grammar, [{"text": s["text"], "ids": ids,
                                        "offs": offs}], cpu)
                ms.append((time.perf_counter() - t0) * 1000)
            ms.sort()
            out[f"latency_{name}_1thread"] = {
                "median_ms": round(statistics.median(ms), 1),
                "p95_ms": round(ms[int(len(ms) * 0.95)], 1)}
    torch.save({k: v.half() if v.is_floating_point() else v
                for k, v in model.state_dict().items()}, run / "model.f16.pt")
    torch.save(q.state_dict(), run / "model.int8.pt")
    (run / "export_hf.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out))


if __name__ == "__main__":
    main()
