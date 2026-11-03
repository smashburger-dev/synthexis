//! Thin CoreML layer for the perception students: load a compiled model
//! (or compile an .mlpackage) for CPU + Neural Engine, IOSurface-backed
//! BGRA input buffers, an fp16 MLMultiArray input, and fp16 outputs read
//! in place. All objc2 unsafe of the students lives here.
#![allow(unsafe_code)]

use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::AnyThread;
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLModelConfiguration, MLMultiArray, MLMultiArrayDataType,
};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelFormatType_32BGRA, CVPixelBuffer,
    CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

pub struct Model {
    model: Retained<MLModel>,
    /// Output names with their element counts.
    pub outputs: Vec<(String, usize)>,
}

impl Model {
    /// Loads `.mlmodelc`, or compiles an `.mlpackage` / `.mlmodel` first.
    pub fn load(path: &Path) -> Result<Self, String> {
        let s = path.to_str().ok_or("path not utf-8")?;
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(s));
            let compiled = if s.ends_with(".mlmodelc") {
                url
            } else {
                #[allow(deprecated)]
                MLModel::compileModelAtURL_error(&url).map_err(|e| e.localizedDescription().to_string())?
            };
            let cfg = MLModelConfiguration::new();
            cfg.setComputeUnits(MLComputeUnits::CPUAndNeuralEngine);
            let model = MLModel::modelWithContentsOfURL_configuration_error(&compiled, &cfg)
                .map_err(|e| e.localizedDescription().to_string())?;
            let desc = model.modelDescription();
            let outs = desc.outputDescriptionsByName();
            let mut outputs = Vec::new();
            for key in outs.allKeys().iter() {
                let count = outs
                    .objectForKey(&key)
                    .and_then(|d| d.multiArrayConstraint())
                    .map(|c| {
                        let shape = c.shape();
                        shape.iter().map(|n| n.integerValue().max(1) as usize).product()
                    })
                    .unwrap_or(0);
                outputs.push((key.to_string(), count));
            }
            Ok(Self { model, outputs })
        }
    }

    /// Name of the output with `count` elements.
    pub fn output_with(&self, count: usize) -> Option<String> {
        self.outputs.iter().find(|(_, c)| *c == count).map(|(n, _)| n.clone())
    }

    /// One synchronous prediction.
    pub fn predict(&self, inputs: &[(&str, Input<'_>)]) -> Result<Outputs, String> {
        unsafe {
            let mut keys: Vec<Retained<NSString>> = Vec::new();
            let mut vals: Vec<Retained<MLFeatureValue>> = Vec::new();
            for (name, input) in inputs {
                keys.push(NSString::from_str(name));
                vals.push(match input {
                    Input::Pixels(p) => MLFeatureValue::featureValueWithPixelBuffer(&p.buf),
                    Input::Array(a) => MLFeatureValue::featureValueWithMultiArray(&a.arr),
                });
            }
            let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let val_refs: Vec<&AnyObject> = vals.iter().map(|v| &***v as &AnyObject).collect();
            let dict = NSDictionary::from_slices(&key_refs, &val_refs);
            let provider = MLDictionaryFeatureProvider::initWithDictionary_error(
                MLDictionaryFeatureProvider::alloc(),
                &dict,
            )
            .map_err(|e| e.localizedDescription().to_string())?;
            let out = self
                .model
                .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
                .map_err(|e| e.localizedDescription().to_string())?;
            Ok(Outputs { out })
        }
    }
}

pub enum Input<'a> {
    Pixels(&'a PixelBuf),
    Array(&'a F16Array),
}

pub struct Outputs {
    out: Retained<ProtocolObject<dyn MLFeatureProvider>>,
}

impl Outputs {
    /// Calls `f(data, strides)` on an fp16 output, in place.
    pub fn with_f16<R>(&self, name: &str, f: impl FnOnce(&[u16], &[usize]) -> R) -> Option<R> {
        unsafe {
            let v = self.out.featureValueForName(&NSString::from_str(name))?;
            let arr = v.multiArrayValue()?;
            if arr.dataType() != MLMultiArrayDataType::Float16 {
                return None;
            }
            let strides: Vec<usize> = arr.strides().iter().map(|n| n.integerValue().max(0) as usize).collect();
            let shape: Vec<usize> = arr.shape().iter().map(|n| n.integerValue().max(0) as usize).collect();
            // Last element offset + 1 bounds the backing store.
            let span = shape.iter().zip(&strides).map(|(n, s)| n.saturating_sub(1) * s).sum::<usize>() + 1;
            #[allow(deprecated)]
            let ptr = arr.dataPointer().as_ptr() as *const u16;
            let data = std::slice::from_raw_parts(ptr, span);
            Some(f(data, &strides))
        }
    }
}

/// IOSurface-backed BGRA pixel buffer, reused across frames.
pub struct PixelBuf {
    buf: CFRetained<CVPixelBuffer>,
    pub w: usize,
    pub h: usize,
}

impl PixelBuf {
    pub fn new(w: usize, h: usize) -> Result<Self, String> {
        unsafe {
            let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
            let key: &NSString = &*(kCVPixelBufferIOSurfacePropertiesKey as *const _ as *const NSString);
            let attrs = NSDictionary::from_slices(&[key], &[&*empty as &AnyObject]);
            let attrs_cf: &CFDictionary = &*(&*attrs as *const NSDictionary<NSString, AnyObject> as *const CFDictionary);
            let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
            let rc = CVPixelBufferCreate(
                None,
                w,
                h,
                kCVPixelFormatType_32BGRA,
                Some(attrs_cf),
                NonNull::from(&mut out),
            );
            let ptr = NonNull::new(out).ok_or(format!("CVPixelBufferCreate failed ({rc})"))?;
            Ok(Self { buf: CFRetained::from_raw(ptr), w, h })
        }
    }

    pub fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buf
    }

    /// Read-only BGRA view while locked: `f(bytes, stride, w, h)`.
    pub fn read<R>(&self, f: impl FnOnce(&[u8], usize, usize, usize) -> R) -> Option<R> {
        read_locked(&self.buf, f)
    }

    /// Writes the pixels: `f(bytes, stride)`.
    pub fn write(&self, f: impl FnOnce(&mut [u8], usize)) {
        unsafe {
            CVPixelBufferLockBaseAddress(&self.buf, CVPixelBufferLockFlags(0));
            let base = CVPixelBufferGetBaseAddress(&self.buf) as *mut u8;
            let stride = CVPixelBufferGetBytesPerRow(&self.buf);
            if !base.is_null() {
                f(std::slice::from_raw_parts_mut(base, stride * self.h), stride);
            }
            CVPixelBufferUnlockBaseAddress(&self.buf, CVPixelBufferLockFlags(0));
        }
    }
}

/// Contiguous fp16 MLMultiArray input.
pub struct F16Array {
    arr: Retained<MLMultiArray>,
    len: usize,
}

impl F16Array {
    pub fn new(shape: &[usize]) -> Result<Self, String> {
        unsafe {
            let dims: Vec<Retained<NSNumber>> = shape.iter().map(|&n| NSNumber::new_usize(n)).collect();
            let refs: Vec<&NSNumber> = dims.iter().map(|d| &**d).collect();
            let arr = MLMultiArray::initWithShape_dataType_error(
                MLMultiArray::alloc(),
                &NSArray::from_slice(&refs),
                MLMultiArrayDataType::Float16,
            )
            .map_err(|e| e.localizedDescription().to_string())?;
            let len = shape.iter().product();
            let a = Self { arr, len };
            a.write(|d| d.fill(0));
            Ok(a)
        }
    }

    pub fn write(&self, f: impl FnOnce(&mut [u16])) {
        unsafe {
            #[allow(deprecated)]
            let ptr = self.arr.dataPointer().as_ptr() as *mut u16;
            f(std::slice::from_raw_parts_mut(ptr, self.len));
        }
    }
}

/// A camera frame the perception thread owns until it is done.
pub struct Frame {
    buf: CFRetained<CVPixelBuffer>,
}

// SAFETY: CVPixelBuffers are reference-counted CF objects that may be
// retained and read from any thread; only one thread locks this one.
unsafe impl Send for Frame {}

fn read_locked<R>(buf: &CVPixelBuffer, f: impl FnOnce(&[u8], usize, usize, usize) -> R) -> Option<R> {
    use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetWidth};
    unsafe {
        CVPixelBufferLockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly);
        let w = CVPixelBufferGetWidth(buf);
        let h = CVPixelBufferGetHeight(buf);
        let stride = CVPixelBufferGetBytesPerRow(buf);
        let base = CVPixelBufferGetBaseAddress(buf) as *const u8;
        let r = if base.is_null() || stride < w * 4 || w == 0 || h == 0 {
            None
        } else {
            Some(f(std::slice::from_raw_parts(base, stride * h), stride, w, h))
        };
        CVPixelBufferUnlockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly);
        r
    }
}

// SAFETY: as for Frame; a PixelBuf moves between the perception thread and
// the landmark worker and is only touched by the thread that holds it.
unsafe impl Send for PixelBuf {}

impl Frame {
    pub fn new(buf: CFRetained<CVPixelBuffer>) -> Self {
        Self { buf }
    }

    /// Read-only BGRA view while locked: `f(bytes, stride, w, h)`.
    pub fn read<R>(&self, f: impl FnOnce(&[u8], usize, usize, usize) -> R) -> Option<R> {
        read_locked(&self.buf, f)
    }

    pub fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buf
    }
}
