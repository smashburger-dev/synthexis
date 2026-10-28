//! Signal sample frame exchanged between the engine and route writers.

/// One delivered value from a signal source.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Normalized value 0..1.
    pub value: f32,
    /// Confidence 0..1 (occlusion handling uses c_min).
    pub confidence: f32,
    /// Capture time on the host clock, nanoseconds (CMClock host time).
    pub t_capture_ns: u64,
    /// False while the source is occluded/not producing.
    pub present: bool,
}

pub const SIG_BRIGHTNESS: &str = "cam0.frame.brightness";
pub const SIG_MOTION: &str = "cam0.frame.motion";
pub const SIG_TRIANGLE: &str = "test.triangle";
