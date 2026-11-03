//! Process-wide perception engine: camera capture + test triangle, shared
//! by all plugin instances (Weak subscriber handles). Never on the audio
//! thread.

pub mod bench;
pub mod camera;
pub mod coreml;
pub mod geom;
pub mod perception;
pub mod vimage;
pub mod vision;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::signal::{Sample, SIG_TRIANGLE};

/// Receives signal frames on the engine's capture/timer threads.
pub trait SignalSubscriber: Send + Sync {
    fn on_sample(&self, signal_id: &str, sample: &Sample);
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

pub struct Engine {
    pub subscribers: Mutex<Vec<Weak<dyn SignalSubscriber>>>,
    pub camera: &'static Arc<camera::EngineShared>,
    pub running: AtomicBool,
}

impl Engine {
    /// Subscribe an instance; starts the engine on first use.
    pub fn subscribe(sub: &Arc<dyn SignalSubscriber>) -> &'static Engine {
        let e = engine();
        e.subscribers.lock().push(Arc::downgrade(sub));
        e
    }

    /// Fan a sample out to every live subscriber.
    pub fn emit(&self, signal_id: &str, sample: &Sample) {
        let mut dead = Vec::new();
        {
            let subs = self.subscribers.lock();
            for (i, w) in subs.iter().enumerate() {
                match w.upgrade() {
                    Some(s) => s.on_sample(signal_id, sample),
                    None => dead.push(i),
                }
            }
        }
        if !dead.is_empty() {
            let mut subs = self.subscribers.lock();
            for i in dead.iter().rev() {
                subs.remove(*i);
            }
        }
    }
}

/// Start (or get) the process-wide engine. Camera + triangle threads.
pub fn engine() -> &'static Engine {
    ENGINE.get_or_init(|| {
        let camera = camera::start();
        let e = Engine {
            subscribers: Mutex::new(Vec::new()),
            camera,
            running: AtomicBool::new(true),
        };
        e
    })
}

/// 30 Hz triangle wave on test.triangle (4 s period, host time stamps).
pub fn start_triangle(engine: &'static Engine) {
    thread::Builder::new()
        .name("synthexis-triangle".into())
        .spawn(move || {
            let t0 = Instant::now();
            let period = Duration::from_millis(1000 / 30);
            let mut next = t0;
            loop {
                next += period;
                let now = Instant::now();
                if next > now {
                    thread::sleep(next - now);
                }
                let phase = (t0.elapsed().as_secs_f64() % 4.0) / 4.0;
                let tri = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
                engine.emit(
                    SIG_TRIANGLE,
                    &Sample {
                        value: tri as f32,
                        confidence: 1.0,
                        t_capture_ns: host_time_ns(),
                        present: true,
                    },
                );
            }
        })
        .ok();
}

/// Host clock in nanoseconds (same timebase as camera PTS conversion).
#[allow(unsafe_code)]
pub fn host_time_ns() -> u64 {
    // SAFETY: CMClock calls are safe wrappers in objc2 marked unsafe only
    // because they call into Objective-C; the host clock is a global
    // singleton readable at any time.
    unsafe {
        (objc2_core_media::CMClock::host_time_clock().time().seconds() * 1e9)
            .max(0.0) as u64
    }
}
