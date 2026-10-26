//! Process-wide AVFoundation camera capture engine.
//!
//! One `OnceLock`-guarded engine shared by all plugin instances. The session
//! is configured and started on a dedicated thread; frames arrive on a serial
//! dispatch queue. Nothing here runs on the audio thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureInput,
    AVCaptureOutput, AVCaptureSession, AVCaptureSessionPreset1280x720,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
    AVCaptureConnection, AVMediaTypeVideo,
};
use objc2_core_media::{
    CMClock, CMSampleBuffer,
};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSString};

use crate::probe_log;
use crate::ProbeParams;

const STATS_WINDOW: usize = 300;
const LOG_EVERY: Duration = Duration::from_secs(5);
const PREVIEW_W: usize = 320;
const PREVIEW_H: usize = 180;

/// Latest camera state shared with the editor and the param pump.
pub struct CaptureShared {
    /// Latest mean luminance of the central 25% region, 0..1 (f32 bits).
    pub luminance_bits: AtomicU32,
    /// Frames delivered so far.
    pub frame_count: AtomicUsize,
    /// Whether the capture session produced at least one frame.
    pub running: AtomicBool,
    /// Authorization status as AVAuthorizationStatus raw integer, or -1 if unknown.
    pub auth_status: parking_lot::Mutex<String>,
    /// Rolling stats.
    pub stats: parking_lot::Mutex<Stats>,
    /// Latest 320x180 RGBA preview frame for the editor.
    pub preview: parking_lot::Mutex<Option<Arc<[u8]>>>,
}

#[derive(Default)]
pub struct Stats {
    pub(crate) latencies_ms: VecDeque<f64>,
    pub(crate) intervals_ms: VecDeque<f64>,
    pub(crate) callback_times: VecDeque<Instant>,
    pub last_log: Option<Instant>,
}

impl Stats {
    pub(crate) fn median_p95(v: &VecDeque<f64>) -> (f64, f64) {
        if v.is_empty() {
            return (0.0, 0.0);
        }
        let mut s: Vec<f64> = v.iter().copied().collect();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = s.len();
        let med = if n % 2 == 1 {
            s[n / 2]
        } else {
            0.5 * (s[n / 2 - 1] + s[n / 2])
        };
        let p95 = s[((n - 1) as f64 * 0.95).round() as usize];
        (med, p95)
    }

    pub(crate) fn delivered_fps(&self) -> f64 {
        if self.callback_times.len() < 2 {
            return 0.0;
        }
        let span = self
            .callback_times
            .back()
            .unwrap()
            .duration_since(*self.callback_times.front().unwrap())
            .as_secs_f64();
        if span <= 0.0 {
            return 0.0;
        }
        (self.callback_times.len() - 1) as f64 / span
    }
}

impl CaptureShared {
    fn new() -> Self {
        Self {
            luminance_bits: AtomicU32::new(0),
            frame_count: AtomicUsize::new(0),
            running: AtomicBool::new(false),
            auth_status: parking_lot::Mutex::new("unknown".into()),
            stats: parking_lot::Mutex::new(Stats::default()),
            preview: parking_lot::Mutex::new(None),
        }
    }

    pub fn luminance(&self) -> f32 {
        f32::from_bits(self.luminance_bits.load(Ordering::Relaxed))
    }
}

pub struct CaptureEngine {
    pub shared: Arc<CaptureShared>,
}

struct DelegateIvars {
    shared: Arc<CaptureShared>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate is used
    // from an arbitrary (non-main) dispatch queue only.
    #[unsafe(super = NSObject)]
    #[name = "SynthexisCaptureDelegate"]
    #[ivars = DelegateIvars]
    struct CaptureDelegate;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for CaptureDelegate {}

    // SAFETY: the method signature matches the protocol requirement.
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            handle_frame(self.ivars().shared.clone(), sample_buffer);
        }
    }
);

impl CaptureDelegate {
    fn new(shared: Arc<CaptureShared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { shared });
        // SAFETY: init is a valid NSObject method.
        unsafe { objc2::msg_send![super(this), init] }
    }
}

fn host_time_seconds() -> f64 {
    unsafe {
        let clock = CMClock::host_time_clock();
        clock.time().seconds()
    }
}

/// Called on the capture dispatch queue for every delivered frame.
fn handle_frame(shared: Arc<CaptureShared>, sample_buffer: &CMSampleBuffer) {
    let now = Instant::now();
    let pts = unsafe { sample_buffer.presentation_time_stamp().seconds() };
    let latency_ms = (host_time_seconds() - pts) * 1000.0;
    shared.frame_count.fetch_add(1, Ordering::Relaxed);
    shared.running.store(true, Ordering::Relaxed);

    // Luminance of the central 25% region + a 320x180 RGBA preview.
    if let Some(image) = unsafe { sample_buffer.image_buffer() } {
        unsafe {
            let _ = CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly);
            let w = CVPixelBufferGetWidth(&image);
            let h = CVPixelBufferGetHeight(&image);
            let stride = CVPixelBufferGetBytesPerRow(&image);
            let base = CVPixelBufferGetBaseAddress(&image) as *const u8;
            if !base.is_null() && w > 0 && h > 0 && stride >= w * 4 {
                let lum = mean_luminance(base, stride, w, h);
                shared
                    .luminance_bits
                    .store(lum.to_bits(), Ordering::Relaxed);
                *shared.preview.lock() = Some(downscale_bgra(base, stride, w, h));
            }
            let _ = CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly);
        }
    }

    // Stats + periodic log.
    {
        {
            let mut st = shared.stats.lock();
            if let Some(prev) = st.callback_times.back() {
                let interval = now.duration_since(*prev).as_secs_f64() * 1000.0;
                st.intervals_ms.push_back(interval);
            }
            st.latencies_ms.push_back(latency_ms);
            st.callback_times.push_back(now);
            while st.latencies_ms.len() > STATS_WINDOW {
                st.latencies_ms.pop_front();
            }
            while st.intervals_ms.len() > STATS_WINDOW {
                st.intervals_ms.pop_front();
            }
            while st.callback_times.len() > STATS_WINDOW {
                st.callback_times.pop_front();
            }

            let due = st
                .last_log
                .map(|t| t.elapsed() >= LOG_EVERY)
                .unwrap_or(true);
            if due {
                let (lat_med, lat_p95) = Stats::median_p95(&st.latencies_ms);
                let (int_med, int_p95) = Stats::median_p95(&st.intervals_ms);
                let fps = st.delivered_fps();
                st.last_log = Some(now);
                probe_log::log(&format!(
                    "capture fps={fps:.1} interval_ms med={int_med:.1} p95={int_p95:.1} \
                     capture_latency_ms med={lat_med:.2} p95={lat_p95:.2} \
                     n={} lum={:.3}",
                    st.latencies_ms.len(),
                    shared.luminance()
                ));
            }
        }
    }

    crate::pump_signal_param(shared.luminance());
}

/// Mean luminance (Rec.601) of the central quarter of a BGRA frame.
unsafe fn mean_luminance(base: *const u8, stride: usize, w: usize, h: usize) -> f32 {
    let x0 = w * 3 / 8;
    let x1 = w * 5 / 8;
    let y0 = h * 3 / 8;
    let y1 = h * 5 / 8;
    let mut sum = 0u64;
    let mut count = 0u64;
    for y in (y0..y1).step_by(2) {
        let row = base.add(y * stride);
        for x in (x0..x1).step_by(2) {
            let px = row.add(x * 4);
            let b = *px as u64;
            let g = *px.add(1) as u64;
            let r = *px.add(2) as u64;
            sum += (r * 3 + g * 6 + b) / 10;
            count += 1;
        }
    }
    if count == 0 {
        return 0.0;
    }
    sum as f32 / (count as f32 * 255.0)
}

/// Nearest-neighbour downscale BGRA -> RGBA at 320x180.
unsafe fn downscale_bgra(base: *const u8, stride: usize, w: usize, h: usize) -> Arc<[u8]> {
    let mut out = Vec::with_capacity(PREVIEW_W * PREVIEW_H * 4);
    for y in 0..PREVIEW_H {
        let sy = y * h / PREVIEW_H;
        let row = base.add(sy * stride);
        for x in 0..PREVIEW_W {
            let sx = x * w / PREVIEW_W;
            let px = row.add(sx * 4);
            out.push(*px.add(2)); // R
            out.push(*px.add(1)); // G
            out.push(*px); // B
            out.push(255);
        }
    }
    out.into()
}

fn auth_status_string() -> String {
    let media = match unsafe { AVMediaTypeVideo } {
        Some(m) => m,
        None => return "no AVMediaTypeVideo".into(),
    };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) };
    match status {
        s if s == AVAuthorizationStatus::NotDetermined => "notDetermined".into(),
        s if s == AVAuthorizationStatus::Restricted => "restricted".into(),
        s if s == AVAuthorizationStatus::Denied => "denied".into(),
        s if s == AVAuthorizationStatus::Authorized => "authorized".into(),
        s => format!("{:?}", s.0),
    }
}

/// Start the capture engine once for the whole process.
pub fn start() -> &'static CaptureEngine {
    static ENGINE: OnceLock<CaptureEngine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let shared = Arc::new(CaptureShared::new());
        let s2 = shared.clone();
        std::thread::Builder::new()
            .name("synthexis-camera".into())
            .spawn(move || run_session(s2))
            .ok();
        probe_log::log("camera engine thread started");
        CaptureEngine { shared }
    })
}

fn run_session(shared: Arc<CaptureShared>) {
    let status = auth_status_string();
    *shared.auth_status.lock() = status.clone();
    probe_log::log(&format!("camera authorizationStatus = {status}"));

    if status == "notDetermined" {
        let media = match unsafe { AVMediaTypeVideo } {
            Some(m) => m,
            None => return,
        };
        probe_log::log("requesting camera access");
        let shared2 = shared.clone();
        let done = RcBlock::new(move |granted: objc2::runtime::Bool| {
            let s = auth_status_string();
            *shared2.auth_status.lock() = s.clone();
            probe_log::log(&format!(
                "requestAccess completed granted={} status={s}",
                granted.as_bool()
            ));
        });
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &done) };
    }

    // Wait briefly for a notDetermined -> resolved transition so a granted
    // session starts without a plugin reload.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let status = auth_status_string();
        if status != "notDetermined" {
            *shared.auth_status.lock() = status.clone();
            probe_log::log(&format!("camera authorizationStatus = {status}"));
            break;
        }
        if Instant::now() > deadline {
            probe_log::log("camera authorization still notDetermined after 120 s");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    if auth_status_string() != "authorized" {
        probe_log::log("camera not authorized, capture disabled");
        return;
    }

    unsafe {
        let media = match AVMediaTypeVideo {
            Some(m) => m,
            None => return,
        };
        let device = match AVCaptureDevice::defaultDeviceWithMediaType(media) {
            Some(d) => d,
            None => {
                probe_log::log("no default video capture device");
                return;
            }
        };
        let input = match AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
            .map_err(|e| e.localizedDescription().to_string())
        {
            Ok(i) => i,
            Err(e) => {
                probe_log::log(&format!("AVCaptureDeviceInput failed: {e}"));
                return;
            }
        };
        let output = AVCaptureVideoDataOutput::new();

        // BGRA pixel format.
        let bgra: u32 = 0x4247_5241; // kCVPixelFormatType_32BGRA
        let key: &NSString = {
            use objc2_core_video::kCVPixelBufferPixelFormatTypeKey;
            // SAFETY: CFString and NSString are toll-free bridged.
            &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString)
        };
        let value = NSNumber::new_u32(bgra);
        let obj: &objc2::runtime::AnyObject = &*value;
        let settings: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
            NSDictionary::from_slices(&[key], &[obj]);
        output.setVideoSettings(Some(&settings));

        let delegate = CaptureDelegate::new(shared.clone());
        let queue = DispatchQueue::new("synthexis.capture", DispatchQueueAttr::SERIAL);
        output.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        );

        let session = AVCaptureSession::new();
        session.beginConfiguration();
        if session.canSetSessionPreset(AVCaptureSessionPreset1280x720) {
            session.setSessionPreset(AVCaptureSessionPreset1280x720);
            probe_log::log("session preset 1280x720");
        } else {
            probe_log::log("1280x720 preset unsupported, using default");
        }
        if session.canAddInput(&input) {
            session.addInput(&input);
        } else {
            probe_log::log("cannot add camera input");
            return;
        }
        if session.canAddOutput(&output) {
            session.addOutput(&output);
        } else {
            probe_log::log("cannot add video data output");
            return;
        }
        session.commitConfiguration();
        session.startRunning();
        probe_log::log("capture session running");

        // Keep session + delegate + queue alive for the process lifetime.
        std::mem::forget(session);
        std::mem::forget(delegate);
        std::mem::forget(queue);
        std::thread::park();
    }
}
