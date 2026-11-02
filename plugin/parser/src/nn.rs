//! Weight loading and the two layer kinds both halves of the model use.
//! Large matrices stay in the storage dtype (f16 by default, so two runs
//! fit in about 250 MB); activations, norms and biases are f32.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use candle_core::{DType, Device, Module, Storage, Tensor};
use candle_nn::LayerNorm;
use half::{f16, slice::HalfFloatSliceExt};
use serde::Deserialize;

pub struct Weights {
    map: HashMap<String, Tensor>,
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
    /// left its 120 MB in macOS malloc's cache of freed large blocks.
    pub fn load(path: &Path, dtype: DType) -> Result<Self> {
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
            map.insert(name, Tensor::from_vec(v, e.shape, &Device::Cpu)?);
        }
        Ok(Self { map, dtype })
    }

    /// Storage dtype for the matrices taken from now on.
    pub fn set_dtype(&mut self, dtype: DType) {
        self.dtype = dtype;
    }

    pub fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
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
        let w = self.mat(&format!("{prefix}.weight"))?;
        let b = if bias {
            Some(self.f32(&format!("{prefix}.bias"))?)
        } else {
            None
        };
        Lin::new(&w, b)
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

/// y = x W^T + b, computed in f32. An f16 weight is widened per call: the
/// f16 kernels of the gemm crate accumulate in f16 and changed 1 of 200
/// frames, and widening plus Accelerate's sgemm is faster than either
/// gemm path (docs/parser-runtime-plan.md, "Measurements").
pub struct Lin {
    w: Tensor,
    b: Option<Tensor>,
}

impl Lin {
    /// w: (out, in) as in PyTorch.
    pub fn new(w: &Tensor, b: Option<Tensor>) -> Result<Self> {
        Ok(Self {
            w: w.contiguous()?,
            b,
        })
    }

    /// The weight in f32: as stored, or widened with half's SIMD slice
    /// conversion (candle's to_dtype converts one element at a time and
    /// took half the parse time).
    fn weight_f32(&self) -> Result<Tensor> {
        if self.w.dtype() == DType::F32 {
            return Ok(self.w.clone());
        }
        let n = self.w.elem_count();
        let (st, layout) = self.w.storage_and_layout();
        let Storage::Cpu(cpu) = &*st else {
            bail!("parser weights live on the CPU");
        };
        let from = layout.start_offset();
        let src = &cpu.as_slice::<f16>()?[from..from + n];
        let mut dst = vec![0f32; n];
        src.convert_to_f32_slice(&mut dst);
        Ok(Tensor::from_vec(dst, self.w.shape(), &Device::Cpu)?)
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
        let y = x.matmul(&self.weight_f32()?.t()?)?;
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
