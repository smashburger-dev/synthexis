# /// script
# requires-python = ">=3.11"
# dependencies = ["torch"]
# ///
"""Training-step timing of a parser-sized model on this Mac (random data).

  uv run probes/mapping-pipeline/train_bench.py [--device mps|cpu]

Model shape from DESIGN.md: vocab 8k, encoder 6 x d256 (4 heads, ff 1024),
decoder 2 x d256 over a 512-token frame vocabulary plus 48 pointer slots.
Batch 64, source length 48, frame length 24. Prints params, median and
p95 ms per optimizer step after 10 warm-up steps.
"""

import argparse
import statistics
import time

import torch
import torch.nn as nn

V, D, H, FF, LE, LD, FV, S, T, B = 8000, 256, 4, 1024, 6, 2, 512, 48, 24, 64


class Parser(nn.Module):
    def __init__(self):
        super().__init__()
        self.src = nn.Embedding(V, D)
        self.tgt = nn.Embedding(FV + S, D)
        self.pos = nn.Embedding(64, D)
        self.tf = nn.Transformer(D, H, LE, LD, FF, batch_first=True,
                                 dropout=0.1)
        self.out = nn.Linear(D, FV)
        self.bio = nn.Linear(D, 40)

    def forward(self, x, y):
        ps, pt = torch.arange(x.shape[1], device=x.device), \
            torch.arange(y.shape[1], device=x.device)
        mem = self.tf.encoder(self.src(x) + self.pos(ps))
        mask = self.tf.generate_square_subsequent_mask(
            y.shape[1], device=x.device)
        h = self.tf.decoder(self.tgt(y) + self.pos(pt), mem, tgt_mask=mask)
        ptr = torch.einsum("btd,bsd->bts", h, mem)        # pointer scores
        return torch.cat([self.out(h), ptr], -1), self.bio(mem)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--device", default="mps")
    ap.add_argument("--steps", type=int, default=60)
    a = ap.parse_args()
    dev = torch.device(a.device)
    m = Parser().to(dev)
    n = sum(p.numel() for p in m.parameters())
    opt = torch.optim.AdamW(m.parameters(), lr=3e-4)
    x = torch.randint(0, V, (B, S), device=dev)
    y = torch.randint(0, FV + S, (B, T), device=dev)
    yt = torch.randint(0, FV + S, (B, T), device=dev)
    bt = torch.randint(0, 40, (B, S), device=dev)
    ce = nn.CrossEntropyLoss()
    times = []
    for i in range(a.steps):
        t0 = time.perf_counter()
        logits, bio = m(x, y)
        loss = ce(logits.reshape(-1, logits.shape[-1]), yt.reshape(-1)) \
            + ce(bio.reshape(-1, 40), bt.reshape(-1))
        opt.zero_grad()
        loss.backward()
        opt.step()
        if dev.type == "mps":
            torch.mps.synchronize()
        if i >= 10:
            times.append((time.perf_counter() - t0) * 1000)
    times.sort()
    print(f"device {a.device} params {n/1e6:.2f}M batch {B} "
          f"median {statistics.median(times):.1f} ms "
          f"p95 {times[int(len(times) * 0.95) - 1]:.1f} ms per step")


if __name__ == "__main__":
    main()
