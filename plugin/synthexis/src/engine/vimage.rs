//! vImage (Accelerate) scaling for the 128 px crops; geom::crop_square
//! stays as the tested reference and the fallback.
#![allow(unsafe_code)]

use std::ffi::c_void;

use super::geom::Bgra;

#[repr(C)]
struct VImageBuffer {
    data: *mut c_void,
    height: usize,
    width: usize,
    row_bytes: usize,
}

#[link(name = "Accelerate", kind = "framework")]
extern "C" {
    fn vImageScale_ARGB8888(src: *const VImageBuffer, dst: *const VImageBuffer, temp: *mut c_void, flags: u32) -> isize;
}

/// Scales the src rect (x, y, w, h) into the dst rect; both rects must lie
/// inside their images. Returns false on a vImage error or a bad rect.
#[allow(clippy::too_many_arguments)]
fn scale(src: &Bgra, sr: (usize, usize, usize, usize), dst: &mut [u8], ds: usize, dh_total: usize, dr: (usize, usize, usize, usize)) -> bool {
    let (sx, sy, sw, sh) = sr;
    let (dx, dy, dw, dh) = dr;
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || sx + sw > src.w || sy + sh > src.h || dy + dh > dh_total || (dx + dw) * 4 > ds {
        return false;
    }
    if dst.len() < ds * dh_total || src.data.len() < src.stride * src.h {
        return false;
    }
    // SAFETY: both rects were checked against their buffers above; vImage
    // reads only inside src and writes only inside the dst rect.
    unsafe {
        let s = VImageBuffer {
            data: src.data.as_ptr().add(sy * src.stride + sx * 4) as *mut c_void,
            height: sh,
            width: sw,
            row_bytes: src.stride,
        };
        let d = VImageBuffer {
            data: dst.as_mut_ptr().add(dy * ds + dx * 4) as *mut c_void,
            height: dh,
            width: dw,
            row_bytes: ds,
        };
        vImageScale_ARGB8888(&s, &d, std::ptr::null_mut(), 0) == 0
    }
}

/// Square crop with black padding outside the frame (same contract as
/// geom::crop_square).
pub fn crop_square(src: &Bgra, x0: f32, y0: f32, side: f32, dst: &mut [u8], ds: usize, n: usize) {
    let ix0 = x0.max(0.0).floor() as usize;
    let iy0 = y0.max(0.0).floor() as usize;
    let ix1 = ((x0 + side).ceil().max(0.0) as usize).min(src.w);
    let iy1 = ((y0 + side).ceil().max(0.0) as usize).min(src.h);
    for row in dst.chunks_mut(ds).take(n) {
        row[..n * 4].fill(0);
    }
    if ix1 <= ix0 || iy1 <= iy0 || side <= 0.0 {
        return;
    }
    let map = |v: usize, o: f32| (((v as f32 - o) / side * n as f32).round().max(0.0) as usize).min(n);
    let (dx0, dx1) = (map(ix0, x0), map(ix1, x0));
    let (dy0, dy1) = (map(iy0, y0), map(iy1, y0));
    if !scale(src, (ix0, iy0, ix1 - ix0, iy1 - iy0), dst, ds, n, (dx0, dy0, dx1 - dx0, dy1 - dy0)) {
        super::geom::crop_square(src, x0, y0, side, dst, ds, n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vimage_crop_matches_the_reference_on_flat_colour() {
        let (w, h) = (64, 40);
        let img: Vec<u8> = (0..w * h).flat_map(|_| [10u8, 20, 30, 255]).collect();
        let src = Bgra { data: &img, stride: w * 4, w, h };
        let mut a = vec![0u8; 16 * 16 * 4];
        crop_square(&src, -16.0, 4.0, 32.0, &mut a, 16 * 4, 16);
        // Left half is outside the frame: black. Right half: the colour.
        assert_eq!(&a[(8 * 16 + 1) * 4..(8 * 16 + 1) * 4 + 3], &[0, 0, 0]);
        assert_eq!(&a[(8 * 16 + 12) * 4..(8 * 16 + 12) * 4 + 3], &[10, 20, 30]);
    }
}
