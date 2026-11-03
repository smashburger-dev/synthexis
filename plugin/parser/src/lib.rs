//! Offline frame parser of the mapping assistant: sentence -> frame of
//! intent, routes, source ids, target kinds and pointer spans.
//!
//! Same model and decode as tools/mapping/frame_parser.py: mmBERT small
//! (ModernBERT) encoder, frame decoder with pointer head, grammar-masked
//! greedy decoding over an ensemble of runs (per step the mean of the
//! members' masked log-softmax, renormalised; `beam` with k = 1), then the
//! decode-time rules of tools/mapping/rules.py. Weights, vocab, rules and
//! the Q8_0 setting come from tools/mapping/export_runtime.py:
//!
//!   <dir>/tokenizer.json, <dir>/config.json      backbone (shared)
//!   <dir>/<run>/model.safetensors, parser.json   per run
//!
//! Loading the four v11 runs in Q8_0 takes 0.2 to 0.6 s and about 350 MB;
//! call it off the audio thread. `parse` is CPU-bound and blocking.

mod decoder;
mod encoder;
mod grammar;
mod nn;
pub mod pin;
pub mod resolve;
mod rules;
mod text;

use std::path::Path;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use candle_core::DType;
use serde::Deserialize;
use tokenizers::Tokenizer;

use decoder::Decoder;
use encoder::Encoder;
use grammar::Grammar;
pub use pin::{pin, Frame, Pin, PinWhat};

const BOS: u32 = 1;
const EOS: u32 = 2;

/// Span tags of the auxiliary BIO head (frame_parser.TAGS); the head's 39
/// outputs are O, then B- and I- of each tag in this order.
pub const BIO_TAGS: [&str; 19] = [
    "SRC", "TGT", "TRACK", "PLUGIN", "PNAME", "NUM", "AMT", "MOD", "NOTE", "GATE", "TIME", "REF", "REJ", "ROLE",
    "PRESET", "KEY", "SCALE", "UNDO", "DELTA",
];

#[derive(Deserialize)]
struct RunMeta {
    config: RunConfig,
    frame_vocab: Vec<String>,
    max_src: usize,
    max_steps: usize,
    max_span: usize,
    /// Decode-time rules the run was scored with (export_runtime.py --rules).
    #[serde(default)]
    rules: Vec<String>,
    /// Encoder weights to be quantised to Q8_0 at load (export_runtime.py --q8).
    #[serde(default)]
    q8: bool,
}

#[derive(Deserialize)]
struct RunConfig {
    d: usize,
    heads: usize,
    dec_layers: usize,
    hf: String,
    #[serde(default)]
    hf_layers: usize,
}

struct Run {
    enc: Encoder,
    dec: Decoder,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Keep the large matrices in f16 (default) or upcast them to f32.
    pub f32_weights: bool,
    /// Worker threads for the matrix kernels; 1 keeps the parser on one core.
    pub threads: usize,
    /// Quantise the encoder matrices and the embedding to ggml Q8_0 at load
    /// (about 58 MB instead of 110 MB per run); the decoder stays f32.
    /// None takes the runs' own setting from parser.json.
    pub q8: Option<bool>,
    /// Decode-time rules applied in order, names from rules::NAMES
    /// (tools/mapping/rules.py); None takes the runs' own from parser.json
    /// (["conv", "pronoun"] for parser v11, none before v10).
    pub rules: Option<Vec<String>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            f32_weights: false,
            threads: 1,
            q8: None,
            rules: None,
        }
    }
}

/// One frame item: a closed token or a pointer span.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Token(String),
    /// Normalised span text and its byte range in the sentence.
    Span { text: String, bytes: (usize, usize) },
}

/// A span the BIO head tagged: one of BIO_TAGS and its byte range.
#[derive(Clone, Debug, PartialEq)]
pub struct Tagged {
    pub tag: &'static str,
    pub bytes: (usize, usize),
}

#[derive(Clone, Debug)]
pub struct Parse {
    pub items: Vec<Item>,
    /// BIO head spans in sentence order (ensemble mean of the members'
    /// log-softmax, argmax per piece; frame_parser's bio_spans grouping).
    pub spans: Vec<Tagged>,
    /// Frame log-probability under the ensemble; exp() is the confidence.
    pub logp: f32,
    /// Wall time of the encoders and of the decode loop, ms.
    pub enc_ms: f32,
    pub dec_ms: f32,
}

impl Parse {
    /// Items as frame_parser.items prints them (spans as "=text").
    pub fn frame(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|it| match it {
                Item::Token(t) => t.clone(),
                Item::Span { text, .. } => format!("={text}"),
            })
            .collect()
    }
}

pub struct Parser {
    tok: Tokenizer,
    runs: Vec<Run>,
    vocab: Vec<String>,
    grammar: Grammar,
    max_src: usize,
    max_steps: usize,
    pool: rayon::ThreadPool,
    rules: Vec<String>,
}

impl Parser {
    /// Loads `runs` (sharing tokenizer and frame vocab) from `dir`.
    pub fn load(dir: &Path, runs: &[&str], opt: &Options) -> Result<Self> {
        if runs.is_empty() {
            bail!("no parser run given");
        }
        let tok = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        let bb = encoder::Config::read(&dir.join("config.json"))?;
        let dtype = if opt.f32_weights { DType::F32 } else { DType::F16 };
        let mut loaded = Vec::new();
        let mut first: Option<RunMeta> = None;
        for name in runs {
            let rd = dir.join(name);
            let meta: RunMeta = serde_json::from_str(&std::fs::read_to_string(rd.join("parser.json"))?)?;
            if meta.config.d != bb.hidden_size {
                bail!("{name}: width {} is not the backbone's", meta.config.d);
            }
            if let Some(f) = &first {
                if f.frame_vocab != meta.frame_vocab || f.config.hf != meta.config.hf {
                    bail!("{name}: frame vocab or backbone differs from {}", runs[0]);
                }
                if f.rules != meta.rules || f.q8 != meta.q8 {
                    bail!("{name}: decode rules or Q8_0 differ from {}", runs[0]);
                }
            }
            let mut w = nn::Weights::load(
                &rd.join("model.safetensors"),
                dtype,
                opt.q8.unwrap_or(meta.q8),
            )?;
            let enc = Encoder::load(&bb, &mut w, "hf.", meta.config.hf_layers)?;
            // the decoder runs up to 48 steps per sentence: keep its 5M
            // weights in f32 instead of widening them every step
            w.set_dtype(DType::F32);
            let dec = Decoder::load(&mut w, meta.config.d, meta.config.heads, meta.config.dec_layers)?;
            if dec.fv() != meta.frame_vocab.len() {
                bail!("{name}: decoder vocab {} != frame vocab {}", dec.fv(), meta.frame_vocab.len());
            }
            loaded.push(Run { enc, dec });
            first.get_or_insert(meta);
        }
        let meta = first.ok_or_else(|| anyhow!("no run"))?;
        let rules = opt.rules.clone().unwrap_or_else(|| meta.rules.clone());
        if let Some(r) = rules.iter().find(|r| !rules::NAMES.contains(&r.as_str())) {
            bail!("unknown rule {r}, known: {}", rules::NAMES.join(", "));
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(opt.threads.max(1))
            .thread_name(|i| format!("synthexis-parser-{i}"))
            .build()?;
        Ok(Self {
            tok,
            runs: loaded,
            grammar: Grammar::new(&meta.frame_vocab, meta.max_steps, meta.max_span),
            vocab: meta.frame_vocab,
            max_src: meta.max_src,
            max_steps: meta.max_steps,
            pool,
            rules,
        })
    }

    pub fn parse(&self, text: &str) -> Result<Parse> {
        self.pool.install(|| self.parse_inner(text))
    }

    fn parse_inner(&self, text: &str) -> Result<Parse> {
        let enc = self.tok.encode(text, true).map_err(|e| anyhow!("{e}"))?;
        let keep = enc.get_ids().len().min(self.max_src - 1);
        let ids = &enc.get_ids()[..keep];
        let offs = &enc.get_offsets()[..keep];
        let n = ids.len();
        let fv = self.vocab.len();
        let t0 = Instant::now();
        let mut states = Vec::with_capacity(self.runs.len());
        for r in &self.runs {
            states.push(r.dec.start(&r.enc.forward(ids)?)?);
        }
        let spans = self.bio(&states, offs)?;
        let t1 = Instant::now();
        let mut seq: Vec<u32> = Vec::new();
        let mut logp = 0.0f32;
        let mut prev = BOS;
        for t in 0..self.max_steps {
            let mask = self.grammar.allowed(&seq, n);
            let mut acc = vec![0.0f32; fv + n];
            for (r, st) in self.runs.iter().zip(states.iter_mut()) {
                let lg = r.dec.step(st, prev, t)?;
                let lp = masked_log_softmax(&lg[..fv + n], &mask);
                acc.iter_mut().zip(&lp).for_each(|(a, l)| *a += l);
            }
            let m = self.runs.len() as f32;
            acc.iter_mut().for_each(|a| *a /= m);
            let lp = if self.runs.len() > 1 {
                masked_log_softmax(&acc, &mask)
            } else {
                acc
            };
            let best = argmax(&lp, &mask).ok_or_else(|| anyhow!("grammar allows nothing"))?;
            logp += lp[best];
            if best as u32 == EOS {
                break;
            }
            seq.push(best as u32);
            prev = best as u32;
        }
        Ok(Parse {
            items: rules::apply(self.items(&seq, text, offs), text, &self.rules),
            spans,
            logp,
            enc_ms: (t1 - t0).as_secs_f32() * 1e3,
            dec_ms: t1.elapsed().as_secs_f32() * 1e3,
        })
    }

    /// BIO spans: a B- piece or a change of tag opens a span, an I- piece
    /// of the same tag extends it; O, BOS and pieces without text close it.
    fn bio(&self, states: &[decoder::State], offs: &[(usize, usize)]) -> Result<Vec<Tagged>> {
        let mut acc: Vec<Vec<f32>> = Vec::new();
        for (r, st) in self.runs.iter().zip(states) {
            for (i, row) in r.dec.bio(st)?.iter().enumerate() {
                let all = vec![true; row.len()];
                let lp = masked_log_softmax(row, &all);
                match acc.get_mut(i) {
                    Some(a) => a.iter_mut().zip(&lp).for_each(|(a, l)| *a += l),
                    None => acc.push(lp),
                }
            }
        }
        let mut out: Vec<Tagged> = Vec::new();
        let mut open = false;
        for (i, row) in acc.iter().enumerate() {
            let all = vec![true; row.len()];
            let tag = argmax(row, &all).unwrap_or(0);
            let (a, b) = offs.get(i).copied().unwrap_or((0, 0));
            if i == 0 || tag == 0 || a >= b {
                open = false;
                continue;
            }
            let name = BIO_TAGS[(tag - 1) / 2];
            let begin = (tag - 1).is_multiple_of(2);
            match out.last_mut() {
                Some(t) if open && !begin && t.tag == name => t.bytes.1 = b,
                _ => {
                    out.push(Tagged { tag: name, bytes: (a, b) });
                    open = true;
                }
            }
        }
        Ok(out)
    }

    fn items(&self, seq: &[u32], text: &str, offs: &[(usize, usize)]) -> Vec<Item> {
        let fv = self.vocab.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i < seq.len() {
            let t = seq[i] as usize;
            if t >= fv {
                let j = match seq.get(i + 1) {
                    Some(&nx) if nx as usize >= fv => nx as usize - fv,
                    _ => t - fv,
                };
                let bytes = (offs[t - fv].0, offs[j].1);
                out.push(Item::Span {
                    text: text::norm(&text::slice(text, bytes.0, bytes.1)),
                    bytes,
                });
                i += 2;
                continue;
            }
            out.push(Item::Token(self.vocab[t].clone()));
            i += 1;
        }
        out
    }
}

fn masked_log_softmax(x: &[f32], mask: &[bool]) -> Vec<f32> {
    let mx = x
        .iter()
        .zip(mask)
        .filter(|(_, &m)| m)
        .map(|(&v, _)| v)
        .fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = x
        .iter()
        .zip(mask)
        .filter(|(_, &m)| m)
        .map(|(&v, _)| (v - mx).exp())
        .sum();
    let lse = mx + sum.ln();
    x.iter()
        .zip(mask)
        .map(|(&v, &m)| if m { v - lse } else { f32::NEG_INFINITY })
        .collect()
}

/// First index of the largest allowed value (torch.argmax order).
fn argmax(x: &[f32], mask: &[bool]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, (&v, &m)) in x.iter().zip(mask).enumerate() {
        if m && best.is_none_or(|b| v > x[b]) {
            best = Some(i);
        }
    }
    best
}
