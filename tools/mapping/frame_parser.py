"""Frame parser: encoding, model, grammar-constrained decoding, metrics.

Shared by train_parser.py and export_parser.py. Shape per DESIGN.md
"Training interfaces": SentencePiece ids -> encoder 6 x d256 -> 2-layer
decoder over closed frame tokens plus pointer steps into the encoder
positions, auxiliary BIO head on the encoder. With a backbone (v7) the
encoder is a pretrained Hugging Face model (mmBERT small, German and
English) read from data/mapping/backbones/<name> or $SYNTHEXIS_BACKBONES,
tokenized by its own tokenizer.json (HFTok).
"""

import os

import json
import math
import re
from collections import Counter
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

TAGS = ["SRC", "TGT", "TRACK", "PLUGIN", "PNAME", "NUM", "AMT", "MOD",
        "NOTE", "GATE", "TIME", "REF", "REJ", "ROLE", "PRESET", "KEY",
        "SCALE", "UNDO", "DELTA"]
BIO = ["O"] + [f"{p}-{t}" for t in TAGS for p in "BI"]
SPECIAL = ["<pad>", "<bos>", "<eos>", "<ptr>"]
PAD, BOS, EOS, PTR = 0, 1, 2, 3
PTR_KINDS = ("TRACK", "PLUGIN", "PNAME", "NOTE", "TIME", "CHORD", "VALUE")
MAX_SRC, MAX_STEPS, MAX_SPAN = 96, 48, 24
WH_BUCKETS, WH_DIM, WH_K = 1 << 14, 64, 24     # optional word hash embedding
HF_MAX_SRC = 128


def backbone_dir(name: str) -> Path:
    root = os.environ.get("SYNTHEXIS_BACKBONES") or \
        Path(__file__).resolve().parents[2] / "data" / "mapping" / "backbones"
    return Path(root) / name


class HFTok:
    """Backbone tokenizer with the SentencePiece calls this module uses.
    Its template adds <bos> (position 0, never a span) and <eos>."""

    def __init__(self, name: str):
        from tokenizers import Tokenizer
        self.tok = Tokenizer.from_file(str(backbone_dir(name) /
                                           "tokenizer.json"))

    def get_piece_size(self) -> int:
        return self.tok.get_vocab_size()


# ---------------------------------------------------------------- data

def frame_vocab(paths: list) -> list:
    toks = set()
    for p in paths:
        for line in Path(p).open():
            toks.update(t for t in json.loads(line)["frame"]
                        if isinstance(t, str))
    return SPECIAL + sorted(toks)


def tokenize(sp, text: str):
    """-> (ids with BOS, [(byte_begin, byte_end)] per position)."""
    if isinstance(sp, HFTok):
        enc = sp.tok.encode(text)
        cb = char_to_byte(text)
        return enc.ids, [(cb[b], cb[e]) for b, e in enc.offsets]
    pieces = sp.encode(text, return_type="proto").pieces[:MAX_SRC - 1]
    ids = [sp.bos_id()] + [p.id for p in pieces]
    offs = [(0, 0)] + [(p.begin, p.end) for p in pieces]
    return ids, offs


def char_to_byte(text: str) -> list:
    out, n = [0], 0
    for ch in text:
        n += len(ch.encode())
        out.append(n)
    return out


def covering(offs: list, bs: int, be: int) -> list:
    return [i for i, (b, e) in enumerate(offs)
            if i > 0 and e > b and b < be and e > bs]


def span_text(text: str, offs: list, i: int, j: int) -> str:
    b = text.encode()
    return b[offs[i][0]:offs[j][1]].decode("utf-8", "ignore")


def fnv1a(s: str) -> int:
    h = 0x811C9DC5
    for b in s.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def word_hashes(text: str, offs: list) -> list:
    """Per position: hashed char 3-5-grams of the whole word the piece lies
    in (ids 1..WH_BUCKETS, at most WH_K), so 'schlie' + bytes + 'e' and a
    typo still share most features with the clean word. BOS and pieces
    without a letter or digit get none."""
    b = text.encode()

    def wordy(c: int) -> bool:          # ASCII letter/digit or any UTF-8 byte
        return c >= 0x80 or chr(c).isalnum()

    out = []
    for i, (s, e) in enumerate(offs):
        if i == 0 or not any(wordy(c) for c in b[s:e]):
            out.append([])
            continue
        while s > 0 and wordy(b[s - 1]):
            s -= 1
        while e < len(b) and wordy(b[e]):
            e += 1
        w = "<" + b[s:e].decode("utf-8", "ignore").casefold() + ">"
        ids = []
        for n in (3, 4, 5):
            for k in range(len(w) - n + 1):
                h = fnv1a(w[k:k + n]) % WH_BUCKETS + 1
                if h not in ids:
                    ids.append(h)
        out.append(ids[:WH_K])
    return out


def wh_tensor(samples: list, S: int) -> torch.Tensor:
    wh = torch.zeros(len(samples), S, WH_K, dtype=torch.long)
    for i, s in enumerate(samples):
        hs = s.get("wh") or word_hashes(s["text"], s["offs"])
        for p, ids in enumerate(hs[:S]):
            if ids:
                wh[i, p, :len(ids)] = torch.tensor(ids)
    return wh


def encode(sp, rec: dict, tok_id: dict):
    """-> sample dict or None when a pointer span is not coverable."""
    text = rec["text"]
    ids, offs = tokenize(sp, text)
    if len(ids) >= MAX_SRC:         # truncated: the frame may name lost text
        return None
    cb = char_to_byte(text)
    tgt = []
    for it in rec["frame"]:
        if isinstance(it, str):
            tgt.append(tok_id[it])
            continue
        pos = covering(offs, cb[it[0]], cb[it[1]])
        if not pos:
            return None
        tgt += [("P", pos[0]), ("P", pos[-1])]
    if len(tgt) + 1 > MAX_STEPS:
        return None
    bio = [0] * len(ids) if rec["bio"] is not None else [-100] * len(ids)
    for cs, ce, tag in rec["bio"] or []:      # null: teacher row, no BIO
        pos = covering(offs, cb[cs], cb[ce])
        for k, p in enumerate(pos):
            bio[p] = BIO.index(("B-" if k == 0 else "I-") + tag)
    bio[0] = -100
    return {"id": rec["id"], "edit_id": rec["edit_id"], "text": text,
            "ids": ids, "offs": offs, "tgt": tgt, "bio": bio,
            "wh": word_hashes(text, offs)}


def load(sp, path, tok_id: dict) -> tuple:
    out, lost = [], 0
    for line in Path(path).open():
        rec = json.loads(line)
        s = encode(sp, rec, tok_id)
        if s is None:
            lost += 1
        else:
            out.append(s)
    return out, lost


def combined(t, fv: int) -> int:
    return fv + t[1] if isinstance(t, tuple) else t


def collate(batch: list, fv: int, device):
    B = len(batch)
    S = max(len(s["ids"]) for s in batch)
    T = max(len(s["tgt"]) for s in batch) + 1
    x = torch.zeros(B, S, dtype=torch.long)
    bio = torch.full((B, S), -100, dtype=torch.long)
    yin = torch.zeros(B, T, dtype=torch.long)
    yout = torch.full((B, T), -100, dtype=torch.long)
    for i, s in enumerate(batch):
        n = len(s["ids"])
        x[i, :n] = torch.tensor(s["ids"])
        bio[i, :n] = torch.tensor(s["bio"])
        seq = [combined(t, fv) for t in s["tgt"]]
        yin[i, :len(seq) + 1] = torch.tensor([BOS] + seq)
        yout[i, :len(seq) + 1] = torch.tensor(seq + [EOS])
    return (x.to(device), bio.to(device), yin.to(device), yout.to(device),
            wh_tensor(batch, S).to(device))


# ---------------------------------------------------------------- model

class Parser(nn.Module):
    def __init__(self, vocab: int, fv: int, d=256, heads=4, ff=1024,
                 enc_layers=6, dec_layers=2, drop=0.1, whash=0, hf="",
                 pretrained=True):
        super().__init__()
        self.fv, self.d, self.whash, self.hf_name = fv, d, whash, hf
        self.cfg = dict(vocab=vocab, fv=fv, d=d, heads=heads, ff=ff,
                        enc_layers=enc_layers, dec_layers=dec_layers,
                        drop=drop, whash=whash, hf=hf)
        if whash:
            self.wh = nn.Embedding(WH_BUCKETS + 1, WH_DIM, padding_idx=0)
            self.wh_proj = nn.Linear(WH_DIM, d)
        if hf:
            from transformers import AutoConfig, AutoModel
            path = backbone_dir(hf)
            self.hf = AutoModel.from_pretrained(path) if pretrained else \
                AutoModel.from_config(AutoConfig.from_pretrained(path))
            assert self.hf.config.hidden_size == d, "d must be the backbone width"
        else:
            self.src = nn.Embedding(vocab, d)
            self.spos = nn.Embedding(MAX_SRC, d)
            self.enc = nn.TransformerEncoder(
                nn.TransformerEncoderLayer(d, heads, ff, drop,
                                           batch_first=True, norm_first=True),
                enc_layers, enable_nested_tensor=False)
        self.tgt = nn.Embedding(fv, d)
        self.tpos = nn.Embedding(MAX_STEPS + 2, d)
        self.ptr_in = nn.Linear(d, d)
        self.dec = nn.TransformerDecoder(
            nn.TransformerDecoderLayer(d, heads, ff, drop, batch_first=True,
                                       norm_first=True), dec_layers)
        self.enc_norm, self.dec_norm = nn.LayerNorm(d), nn.LayerNorm(d)
        self.out = nn.Linear(d, fv)
        self.q, self.k = nn.Linear(d, d), nn.Linear(d, d)
        self.bio = nn.Linear(d, len(BIO))
        self.drop = nn.Dropout(drop)

    def encode(self, x, wh=None):
        pad = x == PAD
        if self.hf_name:
            h = self.hf(input_ids=x, attention_mask=(~pad).long())
            return self.enc_norm(self.drop(h.last_hidden_state)), pad
        pos = torch.arange(x.shape[1], device=x.device)
        h = self.src(x) + self.spos(pos)
        if self.whash and wh is not None:   # mean of the word's n-gram vectors
            n = (wh > 0).sum(-1, keepdim=True).clamp(min=1)
            h = h + self.wh_proj(self.wh(wh).sum(2) / n)
        h = self.drop(h)
        return self.enc_norm(self.enc(h, src_key_padding_mask=pad)), pad

    def decode(self, mem, pad, y):
        is_ptr = y >= self.fv
        emb = self.tgt(torch.where(is_ptr, PTR, y))
        idx = (y - self.fv).clamp(min=0, max=mem.shape[1] - 1)
        gathered = mem.gather(1, idx.unsqueeze(-1).expand(-1, -1, self.d))
        emb = emb + is_ptr.unsqueeze(-1) * self.ptr_in(gathered)
        pos = torch.arange(y.shape[1], device=y.device)
        h = self.drop(emb + self.tpos(pos))
        mask = nn.Transformer.generate_square_subsequent_mask(
            y.shape[1], device=y.device)
        h = self.dec_norm(self.dec(h, mem, tgt_mask=mask, tgt_is_causal=True,
                                   memory_key_padding_mask=pad))
        ptr = self.q(h) @ self.k(mem).transpose(1, 2) / math.sqrt(self.d)
        no = pad.clone()
        no[:, 0] = True                      # BOS is never a span end
        ptr = ptr.masked_fill(no.unsqueeze(1), -1e9)
        return torch.cat([self.out(h), ptr], -1)

    def forward(self, x, yin, wh=None):
        mem, pad = self.encode(x, wh)
        return self.decode(mem, pad, yin), self.bio(mem)


def loss_fn(model, x, bio, yin, yout, bio_w=0.5, wh=None):
    logits, bl = model(x, yin, wh)
    l1 = F.cross_entropy(logits.reshape(-1, logits.shape[-1]),
                         yout.reshape(-1), ignore_index=-100)
    l2 = F.cross_entropy(bl.reshape(-1, len(BIO)), bio.reshape(-1),
                         ignore_index=-100)
    return l1 + bio_w * l2, l1, l2


# ------------------------------------------------------ grammar decoding

class Grammar:
    """State machine over the frame grammar; masks closed tokens and
    pointer positions so every decoded frame is well formed."""

    def __init__(self, vocab: list):
        self.vocab, fv = vocab, len(vocab)
        self.fv = fv

        def m(pred):
            return np.array([pred(t) for t in vocab], dtype=bool)

        self.intent = m(lambda t: t.startswith("I:"))
        self.ptrk = m(lambda t: t in PTR_KINDS)
        self.tparam = m(lambda t: t.startswith("T:param:"))
        self.objshape = m(lambda t: t.startswith(("object:", "shape:")))
        self.mode = m(lambda t: t.startswith("mode:"))
        self.scope = m(lambda t: t.startswith("scope:"))
        self.open = m(lambda t: t == "<route>")
        self.close = m(lambda t: t == "</route>")
        self.eos = m(lambda t: t == "<eos>")
        self.base = ~(self.intent | self.objshape | self.mode | self.scope
                      | self.open | self.close | self.eos
                      | m(lambda t: t in SPECIAL))
        self.ptr_ids = set(np.flatnonzero(self.ptrk))
        self.ft_ids = {vocab.index(t) for t in ("FROM", "TO") if t in vocab}

    def allowed(self, seq: list, n: int) -> np.ndarray:
        """seq: combined ids so far; n: encoder length incl. BOS."""
        fv = self.fv
        out = np.zeros(fv + n, dtype=bool)
        if not seq:
            out[:fv] = self.intent
            return out
        last = seq[-1]
        if last < fv and last in self.ptr_ids:
            out[fv + 1:fv + n] = True
            return out
        if last >= fv and len(seq) >= 2 and seq[-2] in self.ptr_ids:
            p = last - fv
            out[fv + p:fv + min(n, p + MAX_SPAN + 1)] = True
            return out
        if last < fv and last in self.ft_ids:
            out[:fv] = self.tparam
            return out
        opened = sum(1 for t in seq if t < fv and self.open[t])
        closed = sum(1 for t in seq if t < fv and self.close[t])
        in_route = opened > closed
        if len(seq) >= MAX_STEPS - 2:      # force the frame to finish
            out[:fv] = self.close if in_route else self.eos
            return out
        a = self.base.copy()
        lt = self.vocab[last] if last < fv else ""
        if lt.startswith(("S:", "G:", "side:", "object:", "shape:")):
            a |= self.objshape
        if lt.startswith(("T:", "mode:")):
            a |= self.mode
        if in_route:
            a |= self.close if lt != "<route>" else False
        else:
            a |= self.open | self.eos | self.scope
        out[:fv] = a
        return out


@torch.no_grad()
def greedy(model, grammar: Grammar, samples: list, device, bs=128):
    """-> list of (combined id seq without EOS, frame log-prob)."""
    model.eval()
    res = []
    fv = model.fv
    for k in range(0, len(samples), bs):
        part = samples[k:k + bs]
        B = len(part)
        S = max(len(s["ids"]) for s in part)
        x = torch.zeros(B, S, dtype=torch.long)
        for i, s in enumerate(part):
            x[i, :len(s["ids"])] = torch.tensor(s["ids"])
        x = x.to(device)
        wh = wh_tensor(part, S).to(device) if model.whash else None
        mem, pad = model.encode(x, wh)
        y = torch.full((B, 1), BOS, dtype=torch.long, device=device)
        seqs = [[] for _ in range(B)]
        done = [False] * B
        logp = [0.0] * B
        for _ in range(MAX_STEPS):
            lg = model.decode(mem, pad, y)[:, -1].float()
            mask = np.zeros((B, fv + S), dtype=bool)
            for i in range(B):
                if done[i]:
                    mask[i, PAD] = True
                else:
                    a = grammar.allowed(seqs[i], len(part[i]["ids"]))
                    mask[i, :len(a)] = a
            lg = lg.masked_fill(~torch.from_numpy(mask).to(device),
                                float("-inf"))
            lp = F.log_softmax(lg, -1)
            nxt = lp.argmax(-1)
            best = lp.gather(1, nxt.unsqueeze(1)).squeeze(1).cpu().tolist()
            nl = nxt.cpu().tolist()
            for i in range(B):
                if done[i]:
                    continue
                logp[i] += best[i]
                if nl[i] == EOS:
                    done[i] = True
                else:
                    seqs[i].append(nl[i])
            if all(done):
                break
            y = torch.cat([y, nxt.unsqueeze(1)], 1)
        res += list(zip(seqs, logp))
    return res


# ---------------------------------------------------------------- metrics

def norm(s: str) -> str:
    s = re.sub(r"\s+", " ", s.casefold()).strip()
    return s.strip(" .,;:!?\"'()")


def items(seq: list, s: dict, vocab: list) -> list:
    """Combined ids -> closed tokens with pointer pairs as span text."""
    fv, out, i = len(vocab), [], 0
    while i < len(seq):
        t = seq[i]
        if t >= fv:
            j = seq[i + 1] - fv if i + 1 < len(seq) and seq[i + 1] >= fv \
                else t - fv
            out.append("=" + norm(span_text(s["text"], s["offs"],
                                             t - fv, j)))
            i += 2
            continue
        out.append(vocab[t])
        i += 1
    return out


def gold_seq(s: dict, fv: int) -> list:
    return [combined(t, fv) for t in s["tgt"]]


def units(toks: list) -> list:
    """Group tokens into units: head plus its attributes or pointer."""
    out = []
    for t in toks:
        attach = out and (
            (t.startswith(("object:", "shape:")) or
             (t.startswith("side:") and out[-1].startswith(("S:", "G:"))))
            or (t.startswith("mode:") and out[-1].startswith("T:"))
            or t.startswith("=")
            or (t.startswith("T:") and out[-1] in ("FROM", "TO")))
        if attach:
            out[-1] += "|" + t
        else:
            out.append(t)
    return out


def structure(toks: list) -> dict:
    intent = toks[0] if toks else ""
    routes, rest, cur = [], [], None
    for u in units(toks[1:]):
        if u == "<route>":
            cur = []
        elif u == "</route>":
            routes.append(cur or [])
            cur = None
        elif cur is not None:
            cur.append(u)
        else:
            rest.append(u)
    if cur is not None:
        routes.append(cur)
    allu = [u for r in routes for u in r] + rest
    return {
        "intent": intent,
        "routes": Counter(tuple(sorted(r)) for r in routes),
        "rest": Counter(rest),
        "sources": Counter(u for u in allu if u.startswith("S:")),
        "targets": Counter(u for u in allu
                           if u.startswith(("T:", "FROM", "TO"))),
        "invert": Counter((tuple(sorted(u for u in r if u.startswith("S:"))),
                           "INV" in r) for r in routes),
        "scope": Counter(u for u in allu
                         if u.startswith(PTR_KINDS + ("TRACK:ref",))),
    }


FIELDS = ["exact", "route_edit", "intent", "sources", "targets", "invert",
          "scope"]


def compare(pred: list, gold: list) -> dict:
    p, g = structure(pred), structure(gold)
    return {
        "exact": pred == gold,
        "route_edit": p["intent"] == g["intent"]
        and p["routes"] == g["routes"] and p["rest"] == g["rest"],
        **{f: p[f] == g[f] for f in FIELDS[2:]},
    }


def wilson(k: int, n: int, z=1.96) -> list:
    if n == 0:
        return [0.0, 0.0]
    p = k / n
    c = (p + z * z / (2 * n)) / (1 + z * z / n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return [round(c - h, 4), round(c + h, 4)]


def evaluate(model, grammar, samples, vocab, device) -> tuple:
    """-> (metrics dict, per-sentence rows)."""
    fv = len(vocab)
    preds = greedy(model, grammar, samples, device)
    rows = []
    for s, (seq, lp) in zip(samples, preds):
        pt = items(seq, s, vocab)
        gt = items(gold_seq(s, fv), s, vocab)
        rows.append({"id": s["id"], "text": s["text"], "logp": lp,
                     "pred": pt, "gold": gt, **compare(pt, gt)})
    n = len(rows)
    met = {"n": n}
    for f in FIELDS:
        k = sum(r[f] for r in rows)
        met[f] = {"acc": round(k / n, 4), "ci95": wilson(k, n)}
    order = sorted(rows, key=lambda r: -r["logp"])
    curve = []
    for cov in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0):
        top = order[:max(1, round(cov * n))]
        curve.append([cov, round(sum(r["route_edit"] for r in top)
                                 / len(top), 4)])
    met["confidence_curve"] = curve
    best_cov = 0.0
    hits = 0
    for i, r in enumerate(order, 1):
        hits += r["route_edit"]
        if hits / i >= 0.95:
            best_cov = i / n
    met["coverage_at_0.95"] = round(best_cov, 4)
    return met, rows


def load_run(run: Path, device="cpu"):
    """Trained run dir -> (model, tokenizer, frame vocab); sets MAX_SRC."""
    global MAX_SRC
    meta = json.loads((run / "meta.json").read_text())
    cfg = meta["config"]
    MAX_SRC = meta.get("max_src", MAX_SRC)
    if cfg.get("hf"):
        sp = HFTok(cfg["hf"])
    else:
        import sentencepiece as spm
        sp = spm.SentencePieceProcessor(
            model_file=str(Path(__file__).resolve().parents[2]
                           / meta["tokenizer"]))
    model = Parser(cfg["vocab"], cfg["fv"], cfg["d"], cfg["heads"], cfg["ff"],
                   cfg["enc_layers"], cfg["dec_layers"], 0.0,
                   cfg.get("whash", 0), cfg.get("hf", ""), pretrained=False)
    model.load_state_dict(torch.load(run / "model.pt", map_location=device))
    return model.to(device), sp, meta["frame_vocab"]
