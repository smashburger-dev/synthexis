//! Frame decoder of frame_parser.Parser: 2 pre-norm nn.TransformerDecoder
//! layers (ReLU FFN) over closed frame tokens and pointer steps, closed-token
//! head plus a pointer head q(h) . k(mem) / sqrt(d). Decodes one step at a
//! time with a self-attention cache; the result equals the full causal pass.

use anyhow::Result;
use candle_core::{DType, Tensor};
use candle_nn::LayerNorm;

use crate::nn::{attend, heads, norm, Lin, Weights};

const EPS: f64 = 1e-5;
pub const PTR: u32 = 3;

struct Layer {
    sa_qkv: Lin,
    sa_out: Lin,
    ca_q: Lin,
    ca_kv: Lin,
    ca_out: Lin,
    l1: Lin,
    l2: Lin,
    n1: LayerNorm,
    n2: LayerNorm,
    n3: LayerNorm,
}

pub struct Decoder {
    tgt: Tensor,
    tpos: Tensor,
    ptr_in: Lin,
    layers: Vec<Layer>,
    enc_norm: LayerNorm,
    dec_norm: LayerNorm,
    out: Lin,
    q: Lin,
    k: Lin,
    /// Auxiliary BIO head over the encoder memory (frame_parser.BIO tags).
    bio: Lin,
    heads: usize,
    d: usize,
    fv: usize,
}

/// Per-sentence decoder state: encoder memory and the attention caches.
pub struct State {
    mem: Tensor,
    kmem: Tensor,                // k(mem), (S, d)
    cross: Vec<(Tensor, Tensor)>, // per layer K, V of mem, (H, S, D)
    selfkv: Vec<Option<(Tensor, Tensor)>>,
    s: usize,
}

impl Decoder {
    pub fn load(w: &mut Weights, d: usize, heads: usize, layers: usize) -> Result<Self> {
        let mut ls = Vec::with_capacity(layers);
        for i in 0..layers {
            let p = format!("dec.layers.{i}.");
            let ca_w = w.mat(&format!("{p}multihead_attn.in_proj_weight"))?;
            let ca_b = w.f32(&format!("{p}multihead_attn.in_proj_bias"))?;
            let sa_w = w.mat(&format!("{p}self_attn.in_proj_weight"))?;
            let sa_b = w.f32(&format!("{p}self_attn.in_proj_bias"))?;
            ls.push(Layer {
                sa_qkv: Lin::new(&sa_w, Some(sa_b))?,
                sa_out: w.lin(&format!("{p}self_attn.out_proj"), true)?,
                ca_q: Lin::rows(&ca_w, Some(&ca_b), 0, d)?,
                ca_kv: Lin::rows(&ca_w, Some(&ca_b), d, 2 * d)?,
                ca_out: w.lin(&format!("{p}multihead_attn.out_proj"), true)?,
                l1: w.lin(&format!("{p}linear1"), true)?,
                l2: w.lin(&format!("{p}linear2"), true)?,
                n1: w.ln(&format!("{p}norm1"), true, EPS)?,
                n2: w.ln(&format!("{p}norm2"), true, EPS)?,
                n3: w.ln(&format!("{p}norm3"), true, EPS)?,
            });
        }
        let tgt = w.f32("tgt.weight")?;
        let fv = tgt.dim(0)?;
        Ok(Self {
            tgt,
            tpos: w.f32("tpos.weight")?,
            ptr_in: w.lin("ptr_in", true)?,
            layers: ls,
            enc_norm: w.ln("enc_norm", true, EPS)?,
            dec_norm: w.ln("dec_norm", true, EPS)?,
            out: w.lin("out", true)?,
            q: w.lin("q", true)?,
            k: w.lin("k", true)?,
            bio: w.lin("bio", true)?,
            heads,
            d,
            fv,
        })
    }

    pub fn fv(&self) -> usize {
        self.fv
    }

    /// hidden: encoder output (S, d) before enc_norm.
    pub fn start(&self, hidden: &Tensor) -> Result<State> {
        let mem = norm(&self.enc_norm, hidden)?;
        let s = mem.dim(0)?;
        let mut cross = Vec::with_capacity(self.layers.len());
        for l in &self.layers {
            let kv = l.ca_kv.forward(&mem)?;
            cross.push((
                heads(&kv.narrow(1, 0, self.d)?, self.heads)?,
                heads(&kv.narrow(1, self.d, self.d)?, self.heads)?,
            ));
        }
        Ok(State {
            kmem: self.k.forward(&mem)?,
            mem,
            cross,
            selfkv: vec![None; self.layers.len()],
            s,
        })
    }

    /// BIO logits per source piece, (S, 39) row-major.
    pub fn bio(&self, st: &State) -> Result<Vec<Vec<f32>>> {
        Ok(self.bio.forward(&st.mem)?.to_dtype(DType::F32)?.to_vec2::<f32>()?)
    }

    /// Feed combined id `y` at step `t` (BOS at t = 0); returns raw logits
    /// over fv closed tokens followed by S pointer positions.
    pub fn step(&self, st: &mut State, y: u32, t: usize) -> Result<Vec<f32>> {
        let fv = self.fv as u32;
        let is_ptr = y >= fv;
        let tok = if is_ptr { PTR } else { y };
        let mut h = self.tgt.narrow(0, tok as usize, 1)?;
        if is_ptr {
            let m = st.mem.narrow(0, (y - fv) as usize, 1)?;
            h = (h + self.ptr_in.forward(&m)?)?;
        }
        h = (h + self.tpos.narrow(0, t, 1)?)?;
        for (i, l) in self.layers.iter().enumerate() {
            let qkv = l.sa_qkv.forward(&norm(&l.n1, &h)?)?;
            let q = heads(&qkv.narrow(1, 0, self.d)?, self.heads)?;
            let k = heads(&qkv.narrow(1, self.d, self.d)?, self.heads)?;
            let v = heads(&qkv.narrow(1, 2 * self.d, self.d)?, self.heads)?;
            let (k, v) = match st.selfkv[i].take() {
                Some((pk, pv)) => (Tensor::cat(&[&pk, &k], 1)?, Tensor::cat(&[&pv, &v], 1)?),
                None => (k, v),
            };
            let a = attend(&q, &k, &v, None)?;
            st.selfkv[i] = Some((k, v));
            h = (h + l.sa_out.forward(&a)?)?;
            let q = heads(&l.ca_q.forward(&norm(&l.n2, &h)?)?, self.heads)?;
            let (ck, cv) = &st.cross[i];
            h = (&h + l.ca_out.forward(&attend(&q, ck, cv, None)?)?)?;
            let f = l.l2.forward(&l.l1.forward(&norm(&l.n3, &h)?)?.relu()?)?;
            h = (h + f)?;
        }
        let h = norm(&self.dec_norm, &h)?;
        let closed = self.out.forward(&h)?;
        let ptr = (self.q.forward(&h)?.matmul(&st.kmem.t()?)? / (self.d as f64).sqrt())?;
        let mut v = Tensor::cat(&[&closed, &ptr], 1)?
            .squeeze(0)?
            .to_dtype(DType::F32)?
            .to_vec1::<f32>()?;
        v[self.fv] = -1e9; // BOS is never a span end
        debug_assert_eq!(v.len(), self.fv + st.s);
        Ok(v)
    }
}
