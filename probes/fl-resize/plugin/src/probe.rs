//! The verdict of one resize request, measured on one size source (the egui
//! screen rect, or the host's NSView). Pure: time comes in as a `Duration`
//! since the request.

use std::time::Duration;

/// A request counts as honoured only if the size arrives within this time
/// (ui-redesign spec 2.8, the `ResizeGate` rule).
pub const DEADLINE: Duration = Duration::from_millis(500);
/// After reaching the size it has to stay this long to count as "stayed".
pub const HOLD: Duration = Duration::from_secs(3);
/// Points of slack per side.
pub const TOLERANCE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// No answer yet, the deadline has not passed.
    Waiting,
    /// Reached in time and still there, the hold is running.
    Reached { after_ms: u64 },
    /// Reached in time and stayed for `HOLD`.
    Stayed { after_ms: u64 },
    /// Not reached within the deadline; `got` is the last size seen.
    Refused { got: (f32, f32) },
    /// Reached only after the deadline (a refusal turns into this when the
    /// size still arrives within `HOLD`). The gate treats it as a refusal.
    Late { after_ms: u64 },
    /// Reached, then left the asked size before the hold ended.
    SnappedBack { after_ms: u64, got: (f32, f32) },
}

impl Verdict {
    pub fn text(&self) -> String {
        match self {
            Verdict::Waiting => "waiting".to_string(),
            Verdict::Reached { after_ms } => format!("reached after {after_ms} ms, holding"),
            Verdict::Stayed { after_ms } => format!("reached after {after_ms} ms, stayed"),
            Verdict::Refused { got } => format!("refused, stays {:.0} x {:.0}", got.0, got.1),
            Verdict::Late { after_ms } => format!("late, reached after {after_ms} ms"),
            Verdict::SnappedBack { after_ms, got } => {
                format!("snapped back after {after_ms} ms to {:.0} x {:.0}", got.0, got.1)
            }
        }
    }
}

/// Tracks one source of sizes for one request.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    asked: (f32, f32),
    reached_at: Option<Duration>,
    left: Option<(Duration, (f32, f32))>,
    last: Option<(f32, f32)>,
    elapsed: Duration,
}

fn close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= TOLERANCE && (a.1 - b.1).abs() <= TOLERANCE
}

impl Track {
    pub fn new(asked: (u32, u32)) -> Self {
        Self {
            asked: (asked.0 as f32, asked.1 as f32),
            reached_at: None,
            left: None,
            last: None,
            elapsed: Duration::ZERO,
        }
    }

    /// True once nothing can change the verdict any more: the size stayed,
    /// snapped back, came late, or never came within `DEADLINE + HOLD`.
    pub fn settled(&self) -> bool {
        match self.verdict() {
            Verdict::Waiting | Verdict::Reached { .. } => false,
            Verdict::Refused { .. } => self.elapsed >= DEADLINE + HOLD,
            Verdict::Stayed { .. } | Verdict::Late { .. } | Verdict::SnappedBack { .. } => true,
        }
    }

    /// Feeds the size seen `elapsed` after the request.
    pub fn observe(&mut self, elapsed: Duration, size: (f32, f32)) {
        if self.settled() {
            return;
        }
        self.elapsed = self.elapsed.max(elapsed);
        let here = close(size, self.asked);
        if self.reached_at.is_none() && !here {
            self.last = Some(size);
        }
        match self.reached_at {
            None if here => self.reached_at = Some(elapsed),
            Some(_) if !here && self.left.is_none() => self.left = Some((elapsed, size)),
            _ => {}
        }
    }

    pub fn verdict(&self) -> Verdict {
        let ms = |d: Duration| d.as_millis().min(u64::MAX as u128) as u64;
        match (self.reached_at, self.left) {
            (Some(at), _) if at > DEADLINE => Verdict::Late { after_ms: ms(at) },
            (Some(_), Some((when, got))) => Verdict::SnappedBack {
                after_ms: ms(when),
                got,
            },
            (Some(at), None) if self.elapsed >= at + HOLD => Verdict::Stayed { after_ms: ms(at) },
            (Some(at), None) => Verdict::Reached { after_ms: ms(at) },
            (None, _) if self.elapsed >= DEADLINE => Verdict::Refused {
                got: self.last.unwrap_or((0.0, 0.0)),
            },
            (None, _) => Verdict::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn a_size_that_arrives_in_time_and_stays_is_honoured() {
        let mut t = Track::new((1200, 820));
        t.observe(ms(16), (1000.0, 640.0));
        assert_eq!(t.verdict(), Verdict::Waiting);
        t.observe(ms(33), (1200.0, 820.4));
        assert_eq!(t.verdict(), Verdict::Reached { after_ms: 33 });
        t.observe(ms(2000), (1200.0, 820.0));
        assert!(!t.settled());
        t.observe(ms(3033), (1200.0, 820.0));
        assert_eq!(t.verdict(), Verdict::Stayed { after_ms: 33 });
        assert!(t.settled());
    }

    #[test]
    fn no_size_after_500_ms_is_a_refusal() {
        let mut t = Track::new((1200, 900));
        t.observe(ms(499), (1000.0, 640.0));
        assert_eq!(t.verdict(), Verdict::Waiting);
        t.observe(ms(500), (1000.0, 640.0));
        assert_eq!(t.verdict(), Verdict::Refused { got: (1000.0, 640.0) });
        assert!(!t.settled());
        t.observe(ms(3500), (1000.0, 640.0));
        assert_eq!(t.verdict(), Verdict::Refused { got: (1000.0, 640.0) });
        assert!(t.settled());
        // Settled: a size that shows up afterwards changes nothing.
        t.observe(ms(3600), (1200.0, 900.0));
        assert_eq!(t.verdict(), Verdict::Refused { got: (1000.0, 640.0) });
    }

    #[test]
    fn a_size_that_arrives_after_the_deadline_is_late() {
        let mut t = Track::new((1200, 900));
        t.observe(ms(500), (1000.0, 640.0));
        assert_eq!(t.verdict(), Verdict::Refused { got: (1000.0, 640.0) });
        t.observe(ms(900), (1200.0, 900.0));
        assert_eq!(t.verdict(), Verdict::Late { after_ms: 900 });
        assert!(t.settled());
    }

    #[test]
    fn a_size_that_leaves_again_snapped_back() {
        let mut t = Track::new((1200, 720));
        t.observe(ms(20), (1200.0, 720.0));
        t.observe(ms(300), (1000.0, 640.0));
        assert_eq!(
            t.verdict(),
            Verdict::SnappedBack {
                after_ms: 300,
                got: (1000.0, 640.0)
            }
        );
        assert!(t.settled());
    }

    #[test]
    fn a_jump_past_the_deadline_is_late() {
        let mut t = Track::new((1200, 720));
        // Frames stalled: the first observation after the request is already late.
        t.observe(ms(650), (1200.0, 720.0));
        assert_eq!(t.verdict(), Verdict::Late { after_ms: 650 });
    }
}
