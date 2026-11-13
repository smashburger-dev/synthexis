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

use std::sync::{Arc, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::signal::{Sample, SIG_TRIANGLE};

/// Receives signal frames on the engine's capture/timer threads.
pub trait SignalSubscriber: Send + Sync {
    fn on_sample(&self, signal_id: &str, sample: &Sample);
    /// What this subscriber needs running now (its enabled routes).
    fn needs(&self) -> Demand {
        Demand::default()
    }
}

/// What has to run: the camera (frames, brightness, motion, the preview)
/// and perception (the models on those frames). Perception implies the
/// camera.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Demand {
    pub camera: bool,
    pub perception: bool,
}

impl Demand {
    /// What a route reading `signal` needs.
    pub fn of_signal(signal: &str) -> Demand {
        let camera = signal.starts_with("cam0.");
        Demand { camera, perception: camera && !signal.starts_with("cam0.frame.") }
    }

    pub fn or(self, o: Demand) -> Demand {
        Demand { camera: self.camera || o.camera, perception: self.perception || o.perception }
    }
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

/// The last skeleton of each hand (L, R) with its capture time, in preview
/// coordinates, for the window's hand map; perception writes it.
pub static SKELETON: Mutex<[Option<Seen<[[f32; 2]; 21]>>; 2]> = Mutex::new([None, None]);

/// A value with the capture time (ns) of the frame it came from.
pub type Seen<T> = (T, u64);

/// The face values in `signal::FACE_IDS` order as the emitter sends them
/// (a part not seen keeps its last value) and which ones this frame saw,
/// with the capture time: the window's Head section draws them.
pub static FACE: Mutex<Option<Seen<FaceSeen>>> = Mutex::new(None);

/// The face values and which of them the frame saw.
pub type FaceSeen = ([f32; 9], [bool; 9]);

/// The face's tracking boxes (face, eyes, mouth) in preview coordinates
/// (0..1, mirrored like the preview) with the capture time: the window
/// draws them over the camera.
pub static FACE_GEOM: Mutex<Option<Seen<geom::FaceGeom>>> = Mutex::new(None);

/// The debounced finger mask of each hand (L, R) the triggers see, with its
/// capture time: the window shows which fingers count as shown.
pub static MASKS: Mutex<[Option<(u8, u64)>; 2]> = Mutex::new([None, None]);

/// The smoothed palm speed (HandFeatures::speed, 0..1) of each hand with
/// its capture time: the Pong point shoots only from a hand held still.
pub static HAND_SPEED: Mutex<[Option<Seen<f32>>; 2]> = Mutex::new([None, None]);

/// The trigger values per finger of each hand (geom::trigger_fingers) with
/// their capture time, for the finger calibration.
pub static FINGER_VALUES: Mutex<[Option<Seen<[f32; 5]>>; 2]> = Mutex::new([None, None]);

/// The two sources behind FINGER_VALUES per hand: the student's finger
/// probabilities and the geometric extension, for the calibration log.
pub static FINGER_RAW: Mutex<[Option<Seen<[[f32; 5]; 2]>>; 2]> = Mutex::new([None, None]);

/// Per hand and finger the threshold the triggers use (settings `fingers`,
/// calib.rs); 0.5 until the person calibrates.
pub static FINGER_THR: Mutex<[[f32; 5]; 2]> = Mutex::new([[0.5; 5]; 2]);

/// Per hand the person's skeleton templates from the finger calibration
/// (geom::MaskTemplates); empty until a calibration found poses it can
/// tell apart. Not kept in settings: a restart needs a new calibration.
pub static MASK_TEMPLATES: Mutex<[geom::MaskTemplates; 2]> = Mutex::new([geom::MaskTemplates::EMPTY, geom::MaskTemplates::EMPTY]);

/// The thresholds the triggers use; None (Reset) also drops the
/// skeleton templates.
pub fn set_finger_thresholds(t: Option<[[f32; 5]; 2]>) {
    if t.is_none() {
        *MASK_TEMPLATES.lock() = [geom::MaskTemplates::EMPTY, geom::MaskTemplates::EMPTY];
    }
    *FINGER_THR.lock() = t.unwrap_or([[0.5; 5]; 2]);
}

pub struct Engine {
    pub subscribers: Mutex<Vec<Weak<dyn SignalSubscriber>>>,
    pub camera: &'static Arc<camera::EngineShared>,
}

impl Engine {
    /// Subscribe an instance; starts the engine on first use.
    pub fn subscribe(sub: &Arc<dyn SignalSubscriber>) -> &'static Engine {
        let e = engine();
        e.subscribers.lock().push(Arc::downgrade(sub));
        e
    }

    /// What has to run: everything while a plugin window is open, else
    /// what the enabled routes of the live instances read.
    pub fn demand(&self) -> Demand {
        if crate::editor::OPEN_EDITORS.load(std::sync::atomic::Ordering::Acquire) > 0 {
            return Demand { camera: true, perception: true };
        }
        let subs: Vec<_> = self.subscribers.lock().iter().filter_map(Weak::upgrade).collect();
        subs.iter().fold(Demand::default(), |d, s| d.or(s.needs()))
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
        Engine { subscribers: Mutex::new(Vec::new()), camera: camera::start() }
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

#[cfg(test)]
mod tests {
    use super::Demand;

    #[test]
    fn a_route_needs_what_its_signal_comes_from() {
        let (none, cam, both) = (
            Demand::default(),
            Demand { camera: true, perception: false },
            Demand { camera: true, perception: true },
        );
        assert_eq!(Demand::of_signal("test.triangle"), none);
        assert_eq!(Demand::of_signal("cam0.frame.brightness"), cam);
        assert_eq!(Demand::of_signal("cam0.hand.R.openness"), both);
        assert_eq!(cam.or(Demand::of_signal("cam0.mouth.open")), both);
    }
}
