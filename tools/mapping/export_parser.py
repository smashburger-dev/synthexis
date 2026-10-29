# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy"]
# ///
"""Latency, RSS and Rust-loadable weight blobs for a trained parser.

  uv run tools/mapping/export_parser.py --run p-filt-100-s0

CPU inference per sentence (tokenize + encode + grammar-constrained
greedy decode), median and p95 over 200 dev and challenge sentences at
1 and 4 threads; peak RSS of this process. Writes into the run dir:
weights.f16.bin, weights.int8.bin (per-row symmetric int8 for 2-D
weights, f16 row scales, f16 for the rest), export.json (tensor table,
config, latency, RSS, accuracy of the f16 and int8 weights on dev and
challenge). Then audits the training labels: decodes every non-augmented
train_all sentence on MPS and writes audit.json (label disagreement rate
for round-trip kept vs dropped sentences) and audit_suspects.jsonl
(confident disagreements first, for a second look by Spark).
"""

import argparse
import json
import resource
import statistics
import sys
import time
from pathlib import Path

import sentencepiece as spm
import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
TRAIN = ROOT / "data" / "mapping" / "train"


def rss_mb() -> float:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 2**20


def write_blobs(state: dict, out: Path) -> tuple:
    f16, i8, t16, t8 = bytearray(), bytearray(), [], []
    deq = {}
    for k, t in state.items():
        t = t.float()
        b = t.half().numpy().tobytes()
        t16.append({"name": k, "shape": list(t.shape), "dtype": "f16",
                    "offset": len(f16), "nbytes": len(b)})
        f16 += b
        if t.dim() == 2:
            scale = (t.abs().amax(1, keepdim=True) / 127).clamp(min=1e-8)
            q = torch.round(t / scale).clamp(-127, 127).to(torch.int8)
            s16 = scale.half()
            qb, sb = q.numpy().tobytes(), s16.numpy().tobytes()
            t8.append({"name": k, "shape": list(t.shape), "dtype": "i8",
                       "offset": len(i8), "nbytes": len(qb),
                       "scale_offset": len(i8) + len(qb),
                       "scale_nbytes": len(sb)})
            i8 += qb + sb
            deq[k] = q.float() * s16.float()
        else:
            t8.append({"name": k, "shape": list(t.shape), "dtype": "f16",
                       "offset": len(i8), "nbytes": len(b)})
            i8 += b
            deq[k] = t.half().float()
    (out / "weights.f16.bin").write_bytes(bytes(f16))
    (out / "weights.int8.bin").write_bytes(bytes(i8))
    half = {k: v.half().float() for k, v in state.items()}
    return t16, t8, half, deq


def audit(model, grammar, vocab, sp, tok_id, ds: Path, run: Path) -> None:
    recs = [json.loads(l) for l in (ds / "train_all.jsonl").open()]
    kept = {json.loads(l)["id"] for l in (ds / "train_filtered.jsonl").open()}
    samples, flags = [], []
    for r in recs:
        if r["aug"]:
            continue
        s = fp.encode(sp, r, tok_id)
        if s is not None:
            samples.append(s)
            flags.append(r["id"] in kept)
    dev = torch.device("mps" if torch.backends.mps.is_available() else "cpu")
    model.to(dev)
    _, rows = fp.evaluate(model, grammar, samples, vocab, dev)
    model.to("cpu")
    out = {}
    for name, want in (("roundtrip_kept", True), ("roundtrip_dropped", False)):
        sub = [r for r, f in zip(rows, flags) if f == want]
        k = sum(not r["route_edit"] for r in sub)
        out[name] = {"n": len(sub),
                     "disagree": round(k / max(1, len(sub)), 4),
                     "ci95": fp.wilson(k, len(sub))}
    trained_on = json.loads((run / "metrics.json").read_text())["args"]
    out["model_trained_on"] = trained_on["train"]
    (run / "audit.json").write_text(json.dumps(out, indent=1))
    sus = sorted((r | {"roundtrip_kept": f} for r, f in zip(rows, flags)
                  if not r["route_edit"]), key=lambda r: -r["logp"])
    with (run / "audit_suspects.jsonl").open("w") as f:
        for r in sus:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    print("audit", json.dumps(out), flush=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--ds", default="ds-v1")
    ap.add_argument("--n", type=int, default=200)
    a = ap.parse_args()
    run = TRAIN / "runs" / a.run
    meta = json.loads((run / "meta.json").read_text())
    vocab = meta["frame_vocab"]
    tok_id = {t: i for i, t in enumerate(vocab)}
    rss0 = rss_mb()
    sp = spm.SentencePieceProcessor(model_file=str(ROOT / meta["tokenizer"]))
    cfg = meta["config"]
    model = fp.Parser(cfg["vocab"], cfg["fv"], cfg["d"], cfg["heads"],
                      cfg["ff"], cfg["enc_layers"], cfg["dec_layers"], 0.0)
    state = torch.load(run / "model.pt", map_location="cpu")
    model.load_state_dict(state)
    model.eval()
    grammar = fp.Grammar(vocab)
    ds = TRAIN / a.ds
    devset, _ = fp.load(sp, ds / "dev.jsonl", tok_id)
    chal, _ = fp.load(sp, ds / "challenge.jsonl", tok_id)
    texts = [s["text"] for s in (devset[: a.n // 2] + chal)[: a.n]]
    rss_loaded = rss_mb()

    cpu = torch.device("cpu")
    latency = {}
    for threads in (1, 4):
        torch.set_num_threads(threads)
        times = []
        for i, t in enumerate(texts + texts[:10]):
            t0 = time.perf_counter()
            ids, offs = fp.tokenize(sp, t)
            s = {"ids": ids, "offs": offs, "text": t}
            fp.greedy(model, grammar, [s], cpu, bs=1)
            if i >= 10:                     # first 10 are warm-up
                times.append((time.perf_counter() - t0) * 1000)
        times.sort()
        latency[f"threads_{threads}"] = {
            "n": len(times), "median_ms": round(statistics.median(times), 2),
            "p95_ms": round(times[int(len(times) * 0.95) - 1], 2)}
        print(threads, latency[f"threads_{threads}"], flush=True)
    rss_peak = rss_mb()

    t16, t8, half, deq = write_blobs(state, run)
    acc = {}
    torch.set_num_threads(8)
    for label, st in (("f32", state), ("f16", half), ("int8", deq)):
        model.load_state_dict(st)
        for split, data in (("dev", devset), ("challenge", chal)):
            met, _ = fp.evaluate(model, grammar, data, vocab, cpu)
            acc[f"{label}_{split}"] = {"exact": met["exact"]["acc"],
                                       "route_edit": met["route_edit"]["acc"]}
        print(label, {k: v for k, v in acc.items() if k.startswith(label)},
              flush=True)
    export = {
        "run": a.run, "config": cfg, "tokenizer": meta["tokenizer"],
        "frame_vocab": vocab, "bio": meta["bio"],
        "max_src": meta["max_src"], "max_steps": meta["max_steps"],
        "max_span": meta["max_span"],
        "params": sum(t.numel() for t in state.values()),
        "bytes": {"f16": (run / "weights.f16.bin").stat().st_size,
                  "int8": (run / "weights.int8.bin").stat().st_size},
        "latency_cpu": latency,
        "rss_mb": {"python_torch_baseline": round(rss0, 1),
                   "after_load": round(rss_loaded, 1),
                   "peak_inference": round(rss_peak, 1)},
        "accuracy_by_weights": acc,
        "tensors_f16": t16, "tensors_int8": t8,
        "machine": "M1 Pro, CPU inference, torch "
                   + torch.__version__}
    (run / "export.json").write_text(json.dumps(export, indent=1))
    model.load_state_dict(state)
    audit(model, grammar, vocab, sp, tok_id, ds, run)
    print(json.dumps({k: export[k] for k in
                      ("bytes", "latency_cpu", "rss_mb")}))


if __name__ == "__main__":
    main()
