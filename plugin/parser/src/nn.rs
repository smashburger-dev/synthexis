//! Weight loading and the layer kinds both halves of the model use.
//! Large matrices stay in the storage dtype (f16 by default, so two runs
//! fit in about 250 MB) or, with Options::q8, the encoder's are quantised
//! to ggml Q8_0 at load; activations, norms and biases are f32.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use candle_core::quantized::{GgmlDType, QTensor};
use candle_core::{DType, Device, Module, Storage, Tensor};
use candle_nn::LayerNorm;
use half::{f16, slice::HalfFloatSliceExt};
use serde::Deserialize;

pub struct Weights {
    map: HashMap<String, Tensor>,
    quant: HashMap<String, Q8>,
    dtype: DType,
}

#[derive(Deserialize)]
struct Entry {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: (usize, usize),
}

impl Weights {
    /// Reads an f16 safetensors file tensor by tensor straight into the
    /// tensors' own buffers. Reading the whole file into one buffer first
    /// left its 120 MB in macOS malloc's cache of freed large blocks. With
    /// `q8`, every 2-d `hf.*` tensor (encoder matrices and embedding) is
    /// quantised to Q8_0 as soon as it is read, so the f16 copy of a run
    /// never sits next to the quantised one.
    pub fn load(path: &Path, dtype: DType, q8: bool) -> Result<Self> {
        let mut f = std::fs::File::open(path)?;
        let mut len = [0u8; 8];
        f.read_exact(&mut len)?;
        let mut header = vec![0u8; u64::from_le_bytes(len) as usize];
        f.read_exact(&mut header)?;
        let mut entries: Vec<(String, Entry)> =
            serde_json::from_slice::<HashMap<String, serde_json::Value>>(&header)?
                .into_iter()
                .filter(|(k, _)| k != "__metadata__")
                .map(|(k, v)| Ok((k, serde_json::from_value(v)?)))
                .collect::<Result<_>>()?;
        entries.sort_by_key(|(_, e)| e.data_offsets.0);
        let base = 8 + header.len() as u64;
        let mut map = HashMap::with_capacity(entries.len());
        let mut quant = HashMap::new();
        for (name, e) in entries {
            if e.dtype != "F16" {
                bail!("{name}: dtype {} (export writes F16)", e.dtype);
            }
            let n: usize = e.shape.iter().product();
            if e.data_offsets.1 - e.data_offsets.0 != 2 * n {
                bail!("{name}: byte range does not match shape");
            }
            f.seek(SeekFrom::Start(base + e.data_offsets.0 as u64))?;
            let mut v = vec![f16::ZERO; n];
            f.read_exact(bytemuck::cast_slice_mut(&mut v))?;
            let t = Tensor::from_vec(v, e.shape, &Device::Cpu)?;
            if q8 && name.starts_with("hf.") && t.rank() == 2 {
                quant.insert(name, Q8::quantize(&t)?);
            } else {
                map.insert(name, t);
            }
        }
        Ok(Self {
            map,
            quant,
            dtype,
        })
    }

    /// Storage dtype for the matrices taken from now on.
    pub fn set_dtype(&mut self, dtype: DType) {
        self.dtype = dtype;
    }

    pub fn has(&self, name: &str) -> bool {
        self.map.contains_key(name) || self.quant.contains_key(name)
    }

    fn take(&mut self, name: &str) -> Result<Tensor> {
        self.map
            .remove(name)
            .ok_or_else(|| anyhow!("missing tensor {name}"))
    }

    /// Large matrix in the storage dtype.
    pub fn mat(&mut self, name: &str) -> Result<Tensor> {
        let t = self.take(name)?;
        Ok(if t.dtype() == self.dtype {
            t
        } else {
            t.to_dtype(self.dtype)?
        })
    }

    /// Small tensor (bias, norm, position table) in f32.
    pub fn f32(&mut self, name: &str) -> Result<Tensor> {
        Ok(self.take(name)?.to_dtype(DType::F32)?)
    }

    pub fn lin(&mut self, prefix: &str, bias: bool) -> Result<Lin> {
        let name = format!("{prefix}.weight");
        let b = if bias {
            Some(self.f32(&format!("{prefix}.bias"))?)
        } else {
            None
        };
        let w = match self.quant.remove(&name) {
            Some(q) => Mat::Q8(q),
            None => Mat::Dense(self.mat(&name)?.contiguous()?),
        };
        Ok(Lin { w, b })
    }

    /// Token embedding table, Q8_0 rows when quantised at load.
    pub fn emb(&mut self, name: &str) -> Result<Emb> {
        Ok(match self.quant.remove(name) {
            Some(q) => Emb::Q8(q),
            None => Emb::Dense(self.mat(name)?),
        })
    }

    pub fn ln(&mut self, prefix: &str, bias: bool, eps: f64) -> Result<LayerNorm> {
        let w = self.f32(&format!("{prefix}.weight"))?;
        Ok(if bias {
            LayerNorm::new(w, self.f32(&format!("{prefix}.bias"))?, eps)
        } else {
            LayerNorm::new_no_bias(w, eps)
        })
    }
}

/// ggml Q8_0: blocks of 32 weights, an f16 scale then 32 i8.
const Q8_BLOCK: usize = 32;
const Q8_BYTES: usize = 34;

/// A contiguous tensor in f32: as stored, or an f16 one widened with half's
/// SIMD slice conversion (candle's to_dtype converts one element at a time
/// and took half the parse time).
fn widen(t: &Tensor) -> Result<Tensor> {
    if t.dtype() == DType::F32 {
        return Ok(t.clone());
    }
    let n = t.elem_count();
    let (st, layout) = t.storage_and_layout();
    let Storage::Cpu(cpu) = &*st else {
        bail!("parser weights live on the CPU");
    };
    let from = layout.start_offset();
    let src = &cpu.as_slice::<f16>()?[from..from + n];
    let mut dst = vec![0f32; n];
    src.convert_to_f32_slice(&mut dst);
    Ok(Tensor::from_vec(dst, t.shape(), &Device::Cpu)?)
}

/// A matrix in ggml Q8_0 blocks, quantised by candle (QTensor::quantize, the
/// rounding lean.py simulates) and dequantised here row by row.
pub struct Q8 {
    data: Vec<u8>,
    rows: usize,
    cols: usize,
}

impl Q8 {
    fn quantize(w: &Tensor) -> Result<Self> {
        let (rows, cols) = w.dims2()?;
        if cols % Q8_BLOCK != 0 {
            bail!("Q8_0 needs rows of a multiple of {Q8_BLOCK}, got {cols}");
        }
        let mut data = Vec::with_capacity(rows * cols / Q8_BLOCK * Q8_BYTES);
        for r in (0..rows).step_by(1024) {
            // in chunks, so no 50 MB f32 copy of the embedding is ever alive
            let part = widen(&w.narrow(0, r, 1024.min(rows - r))?.contiguous()?)?;
            data.extend_from_slice(&QTensor::quantize(&part, GgmlDType::Q8_0)?.data()?);
        }
        Ok(Self { data, rows, cols })
    }

    fn row_bytes(&self) -> usize {
        self.cols / Q8_BLOCK * Q8_BYTES
    }

    /// Writes whole blocks into `out` (len = blocks * 32). Fixed-size
    /// blocks let LLVM vectorise the loop; pushing element by element made
    /// Q8_0 slower than widening f16.
    fn dequant_into(bytes: &[u8], out: &mut [f32]) {
        let (blocks, _) = bytes.as_chunks::<Q8_BYTES>();
        let (outs, _) = out.as_chunks_mut::<Q8_BLOCK>();
        for (b, o) in blocks.iter().zip(outs) {
            let d = f16::from_le_bytes([b[0], b[1]]).to_f32();
            for (x, &q) in o.iter_mut().zip(&b[2..]) {
                *x = d * f32::from(q as i8);
            }
        }
    }

    fn rows_f32(&self, ids: &[u32]) -> Result<Tensor> {
        let n = self.row_bytes();
        let mut out = vec![0f32; ids.len() * self.cols];
        for (i, &id) in ids.iter().enumerate() {
            let r = id as usize * n;
            let bytes = self
                .data
                .get(r..r + n)
                .ok_or_else(|| anyhow!("row {id} outside a {}-row matrix", self.rows))?;
            Self::dequant_into(bytes, &mut out[i * self.cols..(i + 1) * self.cols]);
        }
        Ok(Tensor::from_vec(out, (ids.len(), self.cols), &Device::Cpu)?)
    }

    fn all_f32(&self) -> Result<Tensor> {
        let mut out = vec![0f32; self.rows * self.cols];
        Self::dequant_into(&self.data, &mut out);
        Ok(Tensor::from_vec(out, (self.rows, self.cols), &Device::Cpu)?)
    }
}

pub enum Emb {
    Dense(Tensor),
    Q8(Q8),
}

impl Emb {
    /// Rows `ids` as an f32 (len, cols) tensor.
    pub fn rows(&self, ids: &[u32]) -> Result<Tensor> {
        match self {
            Emb::Dense(w) => {
                let idx = Tensor::new(ids, &Device::Cpu)?;
                Ok(w.index_select(&idx, 0)?.to_dtype(DType::F32)?)
            }
            Emb::Q8(q) => q.rows_f32(ids),
        }
    }
}

enum Mat {
    Dense(Tensor),
    Q8(Q8),
}

/// y = x W^T + b with f32 activations. An f16 weight is widened per call:
/// the f16 kernels of the gemm crate accumulate in f16 and changed 1 of 200
/// frames, and widening plus Accelerate's sgemm is faster than either gemm
/// path (docs/parser-runtime-plan.md, "Measurements"). A Q8_0 weight is
/// dequantised per call the same way: candle's quantised matmul, which also
/// quantises x, had p95 and max several times higher and more memory.
pub struct Lin {
    w: Mat,
    b: Option<Tensor>,
}

impl Lin {
    /// w: (out, in) as in PyTorch.
    pub fn new(w: &Tensor, b: Option<Tensor>) -> Result<Self> {
        Ok(Self {
            w: Mat::Dense(w.contiguous()?),
            b,
        })
    }

    /// Rows `[from, from + n)` of the PyTorch weight (and bias), e.g. the
    /// q, k or v part of a packed in-projection.
    pub fn rows(w: &Tensor, b: Option<&Tensor>, from: usize, n: usize) -> Result<Self> {
        let b = match b {
            Some(b) => Some(b.narrow(0, from, n)?.contiguous()?),
            None => None,
        };
        Self::new(&w.narrow(0, from, n)?, b)
    }

    /// x: (rows, in) f32 -> (rows, out) f32.
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let y = match &self.w {
            Mat::Dense(w) => x.matmul(&widen(w)?.t()?)?,
            Mat::Q8(q) => x.matmul(&q.all_f32()?.t()?)?,
        };
        Ok(match &self.b {
            Some(b) => y.broadcast_add(b)?,
            None => y,
        })
    }
}

pub fn norm(ln: &LayerNorm, x: &Tensor) -> Result<Tensor> {
    Ok(ln.forward(x)?)
}

/// Attention for one sequence: q (H, Tq, D), k and v (H, Tk, D), optional
/// additive mask (Tq, Tk) -> (Tq, H * D).
pub fn attend(q: &Tensor, k: &Tensor, v: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
    let (h, tq, d) = q.dims3()?;
    let scale = 1.0 / (d as f64).sqrt();
    let mut s = (q.matmul(&k.t()?)? * scale)?;
    if let Some(m) = mask {
        s = s.broadcast_add(m)?;
    }
    let p = candle_nn::ops::softmax_last_dim(&s)?;
    Ok(p.matmul(v)?.transpose(0, 1)?.reshape((tq, h * d))?)
}

/// (T, H * D) -> (H, T, D), contiguous.
pub fn heads(x: &Tensor, h: usize) -> Result<Tensor> {
    let (t, hd) = x.dims2()?;
    Ok(x.reshape((t, h, hd / h))?.transpose(0, 1)?.contiguous()?)
}
