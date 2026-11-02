//! ModernBERT encoder (mmBERT small de-en) as in transformers'
//! ModernBertModel, for one unpadded sentence: token embedding + norm,
//! pre-norm layers with RoPE attention (global every n-th layer, else a
//! sliding window of +-local_attention/2) and a gated GELU MLP, final norm.

use std::path::Path;

use anyhow::Result;
use candle_core::{DType, Device, Tensor};
use candle_nn::LayerNorm;
use serde::Deserialize;

use crate::nn::{attend, heads, norm, Lin, Weights};

#[derive(Deserialize)]
pub struct Config {
    pub hidden_size: usize,
    num_attention_heads: usize,
    num_hidden_layers: usize,
    intermediate_size: usize,
    global_attn_every_n_layers: usize,
    local_attention: usize,
    global_rope_theta: f64,
    local_rope_theta: f64,
    norm_eps: f64,
}

impl Config {
    pub fn read(path: &Path) -> Result<Self> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

struct Layer {
    attn_norm: Option<LayerNorm>, // identity in layer 0
    wqkv: Lin,
    wo: Lin,
    mlp_norm: LayerNorm,
    wi: Lin,
    mlp_wo: Lin,
    global: bool,
}

pub struct Encoder {
    emb: Tensor,
    emb_norm: LayerNorm,
    layers: Vec<Layer>,
    final_norm: LayerNorm,
    heads: usize,
    inter: usize,
    window: usize,
    global_theta: f64,
    local_theta: f64,
}

impl Encoder {
    /// Takes the `{prefix}*` tensors out of `w`.
    pub fn load(cfg: &Config, w: &mut Weights, prefix: &str) -> Result<Self> {
        let eps = cfg.norm_eps;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            let p = format!("{prefix}layers.{i}.");
            let an = format!("{p}attn_norm");
            layers.push(Layer {
                attn_norm: if w.has(&format!("{an}.weight")) {
                    Some(w.ln(&an, false, eps)?)
                } else {
                    None
                },
                wqkv: w.lin(&format!("{p}attn.Wqkv"), false)?,
                wo: w.lin(&format!("{p}attn.Wo"), false)?,
                mlp_norm: w.ln(&format!("{p}mlp_norm"), false, eps)?,
                wi: w.lin(&format!("{p}mlp.Wi"), false)?,
                mlp_wo: w.lin(&format!("{p}mlp.Wo"), false)?,
                global: i % cfg.global_attn_every_n_layers == 0,
            });
        }
        Ok(Self {
            emb: w.mat(&format!("{prefix}embeddings.tok_embeddings.weight"))?,
            emb_norm: w.ln(&format!("{prefix}embeddings.norm"), false, eps)?,
            layers,
            final_norm: w.ln(&format!("{prefix}final_norm"), false, eps)?,
            heads: cfg.num_attention_heads,
            inter: cfg.intermediate_size,
            window: cfg.local_attention / 2,
            global_theta: cfg.global_rope_theta,
            local_theta: cfg.local_rope_theta,
        })
    }

    /// ids incl. BOS/EOS -> last hidden state (S, d), f32.
    pub fn forward(&self, ids: &[u32]) -> Result<Tensor> {
        let s = ids.len();
        let idx = Tensor::new(ids, &Device::Cpu)?;
        let mut x = norm(
            &self.emb_norm,
            &self.emb.index_select(&idx, 0)?.to_dtype(DType::F32)?,
        )?;
        let d = x.dim(1)? / self.heads;
        let global_rope = rope_table(s, d, self.global_theta)?;
        let local_rope = if self.local_theta == self.global_theta {
            None
        } else {
            Some(rope_table(s, d, self.local_theta)?)
        };
        let window = window_mask(s, self.window)?;
        for l in &self.layers {
            let h = match &l.attn_norm {
                Some(n) => norm(n, &x)?,
                None => x.clone(),
            };
            let qkv = l.wqkv.forward(&h)?; // (S, 3 * H * D)
            let hd = self.heads * d;
            let (cos, sin) = match (&local_rope, l.global) {
                (Some(r), false) => (&r.0, &r.1),
                _ => (&global_rope.0, &global_rope.1),
            };
            let rot = |t: Tensor| -> Result<Tensor> {
                let t = heads(&t, self.heads)?.unsqueeze(0)?;
                Ok(candle_nn::rotary_emb::rope(&t, cos, sin)?.squeeze(0)?)
            };
            let q = rot(qkv.narrow(1, 0, hd)?)?;
            let k = rot(qkv.narrow(1, hd, hd)?)?;
            let v = heads(&qkv.narrow(1, 2 * hd, hd)?, self.heads)?;
            let mask = if l.global { None } else { window.as_ref() };
            x = (x + l.wo.forward(&attend(&q, &k, &v, mask)?)?)?;
            let m = l.wi.forward(&norm(&l.mlp_norm, &x)?)?;
            x = (x + l.mlp_wo.forward(&gelu_gate(&m, self.inter)?)?)?;
        }
        norm(&self.final_norm, &x)
    }
}

/// m (S, 2I) = [input | gate] -> gelu(input) * gate (S, I), exact GELU.
/// erf by Abramowitz-Stegun 7.1.26 (|error| < 1.5e-7, one exp), the form
/// of PyTorch's vectorised CPU erf; libm's erff took 13 % of a parse.
fn gelu_gate(m: &Tensor, inter: usize) -> Result<Tensor> {
    let s = m.dim(0)?;
    let v = m.flatten_all()?.to_vec1::<f32>()?;
    let mut out = Vec::with_capacity(s * inter);
    for row in v.chunks_exact(2 * inter) {
        let (a, g) = row.split_at(inter);
        out.extend(a.iter().zip(g).map(|(&a, &g)| 0.5 * a * (1.0 + erf(a * std::f32::consts::FRAC_1_SQRT_2)) * g));
    }
    Ok(Tensor::from_vec(out, (s, inter), &Device::Cpu)?)
}

fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let p = ((((1.061_405_4 * t - 1.453_152_1) * t + 1.421_413_8) * t - 0.284_496_74) * t + 0.254_829_6) * t;
    (1.0 - p * (-x * x).exp()).copysign(x)
}

/// cos, sin (S, D/2) for rotate-half RoPE.
fn rope_table(s: usize, d: usize, theta: f64) -> Result<(Tensor, Tensor)> {
    let half = d / 2;
    let mut cos = Vec::with_capacity(s * half);
    let mut sin = Vec::with_capacity(s * half);
    for t in 0..s {
        for i in 0..half {
            let inv = (1.0 / theta.powf((2 * i) as f64 / d as f64)) as f32;
            let f = t as f32 * inv;
            cos.push(f.cos());
            sin.push(f.sin());
        }
    }
    Ok((
        Tensor::from_vec(cos, (s, half), &Device::Cpu)?,
        Tensor::from_vec(sin, (s, half), &Device::Cpu)?,
    ))
}

/// Additive mask hiding keys further than `w` positions away; None when no
/// pair in a sentence of length `s` is that far apart.
fn window_mask(s: usize, w: usize) -> Result<Option<Tensor>> {
    if s <= w + 1 {
        return Ok(None);
    }
    let m: Vec<f32> = (0..s * s)
        .map(|k| {
            if (k / s).abs_diff(k % s) <= w {
                0.0
            } else {
                f32::NEG_INFINITY
            }
        })
        .collect();
    Ok(Some(Tensor::from_vec(m, (s, s), &Device::Cpu)?))
}
