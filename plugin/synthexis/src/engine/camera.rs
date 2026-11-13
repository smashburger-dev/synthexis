//! AVFoundation camera capture adapted from probes/fl-camera-plugin
//! (our own code). Built-in camera, 1280x720 preset, BGRA, sync-clock
//! corrected PTS. Per frame emits cam0.frame.brightness and
//! cam0.frame.motion Samples to all engine subscribers, and records the
//! pts->callback and callback->features stage timings.
//!
//! All unsafe objc2 calls live here; the rest of the crate stays safe.
#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVCaptureDeviceInput,
    AVCaptureConnection, AVCaptureOutput, AVCaptureSession,
    AVCaptureSessionPreset1280x720, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
};
use objc2_core_media::{
    CMClock, CMSampleBuffer, CMSyncConvertTime,
};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{ns_string, NSBundle, NSDictionary, NSNumber, NSObjectProtocol, NSString};

use crate::log;
use crate::signal::{Sample, SIG_BRIGHTNESS, SIG_MOTION};

const MOTION_W: usize = 64;
const MOTION_H: usize = 36;
/// Scaled so casual hand waving lands around 0.5-1.0.
const MOTION_GAIN: f32 = 8.0;
const STATS_WINDOW: usize = 300;
const LOG_EVERY: Duration = Duration::from_secs(5);
/// How often the camera thread checks what is needed (engine::demand).
const POLL: Duration = Duration::from_millis(250);
/// Nothing needed the camera this long: the session stops (the light goes
/// off); a window or a route that needs it starts it again in about 1.3 s.
const IDLE_STOP: Duration = Duration::from_secs(10);

/// Everything the editor and the latency log read.
pub struct EngineShared {
    pub frame_count: AtomicUsize,
    /// Frames go to the perception thread only while something needs it.
    pub perceive: std::sync::atomic::AtomicBool,
    pub running: std::sync::atomic::AtomicBool,
    pub drop_count: AtomicUsize,
    pub auth_status: parking_lot::Mutex<String>,
    /// The newest mirrored frame for the window (only while one is open).
    pub preview: parking_lot::Mutex<Option<Arc<Preview>>>,
    pub sync_clock: parking_lot::Mutex<Option<Retained<CMClock>>>,
    /// Rolling stage stats (ms): pts->cb, cb->features.
    pub pts_to_cb_ms: parking_lot::Mutex<VecDeque<f64>>,
    pub cb_to_feat_ms: parking_lot::Mutex<VecDeque<f64>>,
    pub intervals_ms: parking_lot::Mutex<VecDeque<f64>>,
    pub last_cb: parking_lot::Mutex<Option<Instant>>,
    pub last_log: parking_lot::Mutex<Option<Instant>>,
    pub emitted: AtomicUsize,
}

impl EngineShared {
    fn new() -> Self {
        Self {
            frame_count: AtomicUsize::new(0),
            perceive: false.into(),
            running: false.into(),
            drop_count: AtomicUsize::new(0),
            auth_status: parking_lot::Mutex::new("unknown".into()),
            preview: parking_lot::Mutex::new(None),
            sync_clock: parking_lot::Mutex::new(None),
            pts_to_cb_ms: parking_lot::Mutex::new(VecDeque::new()),
            cb_to_feat_ms: parking_lot::Mutex::new(VecDeque::new()),
            intervals_ms: parking_lot::Mutex::new(VecDeque::new()),
            last_cb: parking_lot::Mutex::new(None),
            last_log: parking_lot::Mutex::new(None),
            emitted: AtomicUsize::new(0),
        }
    }
}

pub fn median_p95(v: &VecDeque<f64>) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let mut s: Vec<f64> = v.iter().copied().collect();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    let med = if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 };
    let p95 = s[((n - 1) as f64 * 0.95).round() as usize];
    (med, p95)
}

pub struct Engine {
    pub shared: Arc<EngineShared>,
    pub subs: &'static crate::engine::Engine,
}

struct DelegateIvars {
    shared: Arc<EngineShared>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate only
    // runs on our serial dispatch queue.
    #[unsafe(super = NSObject)]
    #[name = "SynthexisCamDelegate"]
    #[ivars = DelegateIvars]
    struct CamDelegate;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for CamDelegate {}

    // SAFETY: signature matches the protocol requirement.
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CamDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            handle_frame(self.ivars().shared.clone(), sample_buffer);
        }

        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_drop_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            _sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            self.ivars().shared.drop_count.fetch_add(1, Ordering::Relaxed);
        }
    }
);

impl CamDelegate {
    fn new(shared: Arc<EngineShared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { shared });
        // SAFETY: init is a valid NSObject method.
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// Mean Rec.601 luma of the central quarter (step 2), 0..1.
unsafe fn mean_luminance(base: *const u8, stride: usize, w: usize, h: usize) -> f32 {
    let mut sum = 0u64;
    let mut count = 0u64;
    for y in (h * 3 / 8..h * 5 / 8).step_by(2) {
        let row = base.add(y * stride);
        for x in (w * 3 / 8..w * 5 / 8).step_by(2) {
            let px = row.add(x * 4);
            sum += (*px.add(2) as u64 * 3 + *px.add(1) as u64 * 6 + *px as u64) / 10;
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum as f32 / (count as f32 * 255.0) }
}

/// Luma downscale into `out` (MOTION_W*MOTION_H) — reused, no alloc.
unsafe fn luma_downscale(
    base: *const u8, stride: usize, w: usize, h: usize, out: &mut [u8],
) {
    for dy in 0..MOTION_H {
        let sy = dy * h / MOTION_H;
        let row = base.add(sy * stride);
        for dx in 0..MOTION_W {
            let sx = dx * w / MOTION_W;
            let px = row.add(sx * 4);
            out[dy * MOTION_W + dx] =
                ((*px.add(2) as u32 * 3 + *px.add(1) as u32 * 6 + *px as u32) / 10) as u8;
        }
    }
}

/// The camera frame for the window: source size, mirrored, RGBA with
/// alpha 255. No egui type, so the engine stays UI-free.
pub struct Preview {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// BGRA rows of `stride` bytes -> a mirrored RGBA preview at the source
/// size (the only per-frame allocation). Rows missing from `src` stay
/// black, so a short buffer never reads out of bounds.
fn mirror_to_rgba(src: &[u8], stride: usize, w: usize, h: usize) -> Preview {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = y.checked_mul(stride).and_then(|a| src.get(a..a + w * 4));
        match row {
            Some(row) => {
                for &[b, g, r, _] in row.as_chunks::<4>().0.iter().rev() {
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
            None => rgba.extend(std::iter::repeat_n([0, 0, 0, 255], w).flatten()),
        }
    }
    Preview { w, h, rgba }
}

/// The preview of a locked pixel buffer: `base` points at `h` rows of
/// `stride` bytes, each at least `w * 4` long.
unsafe fn preview_rgba(base: *const u8, stride: usize, w: usize, h: usize) -> Preview {
    let len = stride * h.saturating_sub(1) + w * 4;
    // SAFETY: the caller checked base non-null, w, h > 0 and stride >= w * 4
    // on a locked CVPixelBuffer, whose data holds h rows of stride bytes.
    let src = std::slice::from_raw_parts(base, len);
    mirror_to_rgba(src, stride, w, h)
}

fn push(v: &mut VecDeque<f64>, x: f64) {
    v.push_back(x);
    while v.len() > STATS_WINDOW {
        v.pop_front();
    }
}

/// Per-frame: stage timings + features + emit.
fn handle_frame(shared: Arc<EngineShared>, sample_buffer: &CMSampleBuffer) {
    let cb = Instant::now();
    let host_clock = unsafe { CMClock::host_time_clock() };
    let cb_host_s = unsafe { host_clock.time().seconds() };
    let pts = unsafe { sample_buffer.presentation_time_stamp() };
    let pts_host_s = {
        let sync = shared.sync_clock.lock();
        match sync.as_ref() {
            Some(s) => unsafe { CMSyncConvertTime(pts, s, &host_clock).seconds() },
            None => unsafe { pts.seconds() },
        }
    };
    shared.frame_count.fetch_add(1, Ordering::Relaxed);
    shared.running.store(true, Ordering::Relaxed);
    push(&mut shared.pts_to_cb_ms.lock(), (cb_host_s - pts_host_s) * 1000.0);
    {
        let mut last = shared.last_cb.lock();
        if let Some(prev) = *last {
            push(&mut shared.intervals_ms.lock(),
                 cb.duration_since(prev).as_secs_f64() * 1000.0);
        }
        *last = Some(cb);
    }

    // Features.
    static mut PREV: Option<Box<[u8; MOTION_W * MOTION_H]>> = None;
    let mut bright = 0.0f32;
    let mut motion = 0.0f32;
    if let Some(image) = unsafe { sample_buffer.image_buffer() } {
        unsafe {
            let _ = CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly);
            let w = CVPixelBufferGetWidth(&image);
            let h = CVPixelBufferGetHeight(&image);
            let stride = CVPixelBufferGetBytesPerRow(&image);
            let base = CVPixelBufferGetBaseAddress(&image) as *const u8;
            if !base.is_null() && w > 0 && h > 0 && stride >= w * 4 {
                bright = mean_luminance(base, stride, w, h);
                let mut small = [0u8; MOTION_W * MOTION_H];
                luma_downscale(base, stride, w, h, &mut small);
                if let Some(prev) = PREV.as_ref() {
                    let mut acc = 0u64;
                    for i in 0..MOTION_W * MOTION_H {
                        acc += small[i].abs_diff(prev[i]) as u64;
                    }
                    motion =
                        ((acc as f32 / (MOTION_W * MOTION_H) as f32 / 255.0) * MOTION_GAIN)
                            .clamp(0.0, 1.0);
                }
                PREV = Some(Box::new(small));
                // the preview only for an open window
                if crate::editor::OPEN_EDITORS.load(Ordering::Acquire) > 0 {
                    *shared.preview.lock() = Some(Arc::new(preview_rgba(base, stride, w, h)));
                }
            }
            let _ = CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly);
        }
        // The camera frame is unmirrored (only the preview is mirrored).
        if shared.perceive.load(Ordering::Acquire) {
            crate::engine::perception::submit(
                crate::engine::coreml::Frame::new(image),
                (pts_host_s.max(0.0) * 1e9) as u64,
                cb,
            );
        }
    }
    let feat_done = Instant::now();
    push(&mut shared.cb_to_feat_ms.lock(),
         feat_done.duration_since(cb).as_secs_f64() * 1000.0);
    shared.emitted.fetch_add(1, Ordering::Relaxed);

    let t_cap = (pts_host_s.max(0.0) * 1e9) as u64;
    let eng = crate::engine::engine();
    eng.emit(
        SIG_BRIGHTNESS,
        &Sample { value: bright, confidence: 1.0, t_capture_ns: t_cap, present: true },
    );
    eng.emit(
        SIG_MOTION,
        &Sample { value: motion, confidence: 1.0, t_capture_ns: t_cap, present: true },
    );

    // 5 s log.
    {
        let mut last = shared.last_log.lock();
        if last.map(|t| t.elapsed() >= LOG_EVERY).unwrap_or(true) {
            *last = Some(feat_done);
            let (p2c_m, p2c_p) = median_p95(&shared.pts_to_cb_ms.lock());
            let (c2f_m, c2f_p) = median_p95(&shared.cb_to_feat_ms.lock());
            let (iv_m, _) = median_p95(&shared.intervals_ms.lock());
            let fps = 1000.0 / iv_m.max(1.0);
            log::log(&format!(
                "engine fps={fps:.1} pts_to_cb_ms med={p2c_m:.1} p95={p2c_p:.1} \
                 cb_to_feat_ms med={c2f_m:.2} p95={c2f_p:.2} \
                 frames={} dropped={}",
                shared.frame_count.load(Ordering::Relaxed),
                shared.drop_count.load(Ordering::Relaxed)
            ));
        }
    }
}

fn auth_status_string() -> String {
    let media = match unsafe { AVMediaTypeVideo } {
        Some(m) => m,
        None => return "no AVMediaTypeVideo".into(),
    };
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) } {
        s if s == AVAuthorizationStatus::NotDetermined => "notDetermined".into(),
        s if s == AVAuthorizationStatus::Restricted => "restricted".into(),
        s if s == AVAuthorizationStatus::Denied => "denied".into(),
        s if s == AVAuthorizationStatus::Authorized => "authorized".into(),
        s => format!("{:?}", s.0),
    }
}

/// auth_status when the host app declares no camera use (editor.rs).
pub const NO_HOST_ACCESS: &str = "noHostAccess";

/// Whether the host app (the main bundle the plugin runs in) declares
/// NSCameraUsageDescription. Without it TCC may kill the host when the
/// plugin asks for the camera (Logic 12.4 has none; REAPER crashed this
/// way, SPARTA#82), so synthexis does not ask (docs/host-support.md).
fn host_declares_camera() -> bool {
    NSBundle::mainBundle()
        .infoDictionary()
        .is_some_and(|info| info.objectForKey(ns_string!("NSCameraUsageDescription")).is_some())
}

fn run_session(shared: Arc<EngineShared>) {
    // Nothing happens (no permission prompt either, e.g. during a plugin
    // scan) until a window or a route needs the camera.
    while !crate::engine::engine().demand().camera {
        std::thread::sleep(POLL);
    }
    // A host without a usage string gets no request. Access granted
    // earlier still works, so only an unanswered or denied status stops.
    let declared = host_declares_camera();
    log::log(&format!("host declares camera use: {declared}"));
    if !declared && auth_status_string() != "authorized" {
        *shared.auth_status.lock() = NO_HOST_ACCESS.into();
        log::log("host app declares no camera use, not asking for access");
        return;
    }
    // Permission gate, same as the probe.
    if auth_status_string() == "notDetermined" {
        if let Some(media) = unsafe { AVMediaTypeVideo } {
            let s2 = shared.clone();
            let done = RcBlock::new(move |granted: objc2::runtime::Bool| {
                *s2.auth_status.lock() = auth_status_string();
                log::log(&format!("camera access granted={}", granted.as_bool()));
            });
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &done)
            };
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        while auth_status_string() == "notDetermined" && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let status = auth_status_string();
    *shared.auth_status.lock() = status.clone();
    log::log(&format!("camera authorizationStatus = {status}"));
    if status != "authorized" {
        log::log("camera not authorized, capture disabled");
        return;
    }

    unsafe {
        let media = match AVMediaTypeVideo { Some(m) => m, None => return };
        // Explicit built-in camera — never Continuity/virtual cameras.
        let device = {
            use objc2_av_foundation::AVCaptureDeviceTypeBuiltInWideAngleCamera;
            let devices = AVCaptureDevice::devices();
            let mut found = None;
            for i in 0..devices.len() {
                let d = devices.objectAtIndex(i);
                if &*d.deviceType() == AVCaptureDeviceTypeBuiltInWideAngleCamera {
                    found = Some(d);
                    break;
                }
            }
            match found.or_else(|| AVCaptureDevice::defaultDeviceWithMediaType(media)) {
                Some(d) => d,
                None => {
                    log::log("no built-in video capture device");
                    return;
                }
            }
        };
        log::log(&format!("device name=\"{}\" uniqueID={}",
            device.localizedName(), device.uniqueID()));

        let input = match AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
            .map_err(|e| e.localizedDescription().to_string())
        {
            Ok(i) => i,
            Err(e) => {
                log::log(&format!("AVCaptureDeviceInput failed: {e}"));
                return;
            }
        };
        let output = AVCaptureVideoDataOutput::new();
        let bgra: u32 = 0x4247_5241;
        let key: &NSString = {
            use objc2_core_video::kCVPixelBufferPixelFormatTypeKey;
            &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString)
        };
        let value = NSNumber::new_u32(bgra);
        let obj: &objc2::runtime::AnyObject = &*value;
        let settings: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
            NSDictionary::from_slices(&[key], &[obj]);
        output.setVideoSettings(Some(&settings));

        let delegate = CamDelegate::new(shared.clone());
        let queue = DispatchQueue::new("synthexis.capture", DispatchQueueAttr::SERIAL);
        output.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        );

        let session = AVCaptureSession::new();
        session.beginConfiguration();
        if session.canSetSessionPreset(AVCaptureSessionPreset1280x720) {
            session.setSessionPreset(AVCaptureSessionPreset1280x720);
        }
        if session.canAddInput(&input) { session.addInput(&input); }
        if session.canAddOutput(&output) { session.addOutput(&output); }
        session.commitConfiguration();

        // Runs while something needs it: perception follows the demand at
        // once (its models stay loaded), the session stops after IDLE_STOP
        // without any need. The thread owns session, delegate and queue for
        // the life of the process.
        let mut running = false;
        let mut idle_since: Option<Instant> = None;
        loop {
            let need = crate::engine::engine().demand();
            shared.perceive.store(need.perception, Ordering::Release);
            if need.camera {
                idle_since = None;
                if !running {
                    session.startRunning();
                    running = true;
                    log::log("capture session running");
                    if shared.sync_clock.lock().is_none() {
                        match session.synchronizationClock() {
                            Some(sync) => {
                                let host = CMClock::host_time_clock();
                                let off = (sync.time().seconds() - host.time().seconds()) * 1000.0;
                                log::log(&format!(
                                    "synchronization_clock is_host={} offset_ms={off:.3}",
                                    *sync == *host
                                ));
                                *shared.sync_clock.lock() = Some(sync);
                            }
                            None => log::log("synchronization_clock = none"),
                        }
                    }
                }
            } else if running && idle_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_STOP {
                session.stopRunning();
                running = false;
                // the window shows "starting" and no stale picture next time
                *shared.last_cb.lock() = None;
                *shared.preview.lock() = None;
                shared.running.store(false, Ordering::Relaxed);
                log::log(&format!("capture session stopped, nothing needed it for {} s", IDLE_STOP.as_secs()));
            }
            std::thread::sleep(POLL);
        }
    }
}

/// Start the capture thread once per process.
pub fn start() -> &'static Arc<EngineShared> {
    static SHARED: OnceLock<Arc<EngineShared>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let shared = Arc::new(EngineShared::new());
        let s2 = shared.clone();
        std::thread::Builder::new()
            .name("synthexis-camera".into())
            .spawn(move || run_session(s2))
            .ok();
        log::log("camera engine thread started");
        shared
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_to_rgba_flips_and_swizzles() {
        // 4x2 BGRA, stride 20: 4 bytes of padding after each row
        let mut src = Vec::new();
        for y in 0..2u8 {
            for x in 0..4u8 {
                src.extend_from_slice(&[10 * y + x, 100 + x, 200 + y, 7]);
            }
            src.extend_from_slice(&[99; 4]);
        }
        let p = mirror_to_rgba(&src, 20, 4, 2);
        assert_eq!((p.w, p.h, p.rgba.len()), (4, 2, 32));
        for y in 0..2u8 {
            for x in 0..4u8 {
                let at = (usize::from(y) * 4 + usize::from(x)) * 4;
                let sx = 3 - x;
                assert_eq!(&p.rgba[at..at + 4], &[200 + y, 100 + sx, 10 * y + sx, 255], "pixel {x},{y}");
            }
        }
        // a buffer one row short: the missing row is black, nothing panics
        let p = mirror_to_rgba(&src[..20], 20, 4, 2);
        assert_eq!(&p.rgba[16..20], &[0, 0, 0, 255]);
    }
}
