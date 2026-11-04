//! The flick shot (option; the default shot is pointing, shot.rs): the
//! index pressed against the thumb, then sprung off. Read from the 21 hand
//! joints per camera frame (engine::SKELETON: preview coordinates,
//! mirrored, each axis 0..1) and, when the frame has them, the index
//! finger's trigger value.
//!
//! A load (thumb and index tips together, the index curled) held for
//! LOAD_MIN_FRAMES, then a candidate frame within FIRE_WINDOW_NS where the
//! tips are apart, then a confirmation frame that still shows them apart.
//! Between the last loaded frame and the candidate: the gap opened fast,
//! the index reached out, the index tip travelled while the thumb stayed
//! put (a pinch release moves both), and the other fingers kept their
//! place (a fist springing open moves all). An index pointing into the
//! lens is too short in the picture and is refused. All times are capture
//! times of the hand's own frames.

use super::shot::{features, Feat, Index, Shot};

/// Loaded: thumb and index tips at most this far apart (palms).
pub const LOAD_GAP: f32 = 0.35;
/// Loaded: the index tip at most this far from the wrist (palms), a curled
/// index; a straight one reaches about 1.8.
pub const LOAD_REACH: f32 = 1.45;
pub const LOAD_MIN_FRAMES: u32 = 2;
/// The candidate frame comes at most this long after the last loaded one.
pub const FIRE_WINDOW_NS: u64 = 200_000_000;
/// Candidate: tips at least this far apart (palms).
pub const FIRE_GAP: f32 = 0.8;
/// Confirmation: tips still at least this far apart.
pub const CONFIRM_GAP: f32 = 0.7;
/// The gap opened at least this fast, last load to candidate (palms/s).
pub const FIRE_RATE: f32 = 5.0;
/// The index tip reached out at least this much more than at the load.
pub const REACH_GAIN: f32 = 0.35;
/// The middle, ring and pinky tips reached out at most this much on average.
pub const OTHERS_GAIN: f32 = 0.3;
/// Against the palm centre the index tip travelled at least this far ...
pub const TIP_TRAVEL: f32 = 0.6;
/// ... and the thumb tip less than this.
pub const THUMB_TRAVEL: f32 = 0.35;
/// The index chain this straight (knuckle-to-tip over its length).
pub const FIRE_STRAIGHT: f32 = 0.8;
pub const REFRACTORY_NS: u64 = 250_000_000;
/// A capture-time gap this long between a hand's frames starts over.
pub const LOST_NS: u64 = 200_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Idle,
    /// Loaded frames so far and the last one.
    Loaded { frames: u32, last: (u64, Feat) },
    /// The last loaded frame and the candidate after it.
    Candidate { load: (u64, Feat), cand: (u64, Feat) },
}

/// The flick detector of one hand.
#[derive(Clone, Debug, Default)]
pub struct Flick {
    state: State,
    seen_ns: Option<u64>,
    shot_ns: Option<u64>,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

impl Flick {
    /// One camera frame of the hand (capture time `t_ns`, `index` the
    /// index finger's trigger value and threshold if known); a shot when
    /// it fires. Frames not newer than the last are ignored.
    pub fn update(&mut self, joints: &[[f32; 2]; 21], index: Index, t_ns: u64) -> Option<Shot> {
        if self.seen_ns.is_some_and(|s| t_ns <= s) {
            return None;
        }
        if self.seen_ns.is_some_and(|s| t_ns - s > LOST_NS) {
            self.state = State::Idle;
        }
        self.seen_ns = Some(t_ns);
        let Some(f) = features(joints) else {
            self.state = State::Idle;
            return None;
        };
        let loaded = f.gap <= LOAD_GAP && f.reach <= LOAD_REACH && index.is_none_or(|(v, thr)| v <= thr);
        let state = std::mem::take(&mut self.state);
        if loaded {
            let frames = match state {
                State::Loaded { frames, .. } => frames + 1,
                _ => 1,
            };
            self.state = State::Loaded { frames, last: (t_ns, f) };
            return None;
        }
        match state {
            State::Idle => None,
            State::Loaded { frames, last } => {
                if t_ns - last.0 > FIRE_WINDOW_NS || frames < LOAD_MIN_FRAMES {
                    return None;
                }
                self.state = if f.gap >= FIRE_GAP { State::Candidate { load: last, cand: (t_ns, f) } } else { State::Loaded { frames, last } };
                None
            }
            State::Candidate { load, cand } => self.confirm(load, cand, f, index),
        }
    }

    /// The frame after a candidate: fires when everything holds.
    fn confirm(&mut self, (lt, l): (u64, Feat), (ct, c): (u64, Feat), f: Feat, index: Index) -> Option<Shot> {
        let dt = (ct - lt) as f32 / 1e9;
        let travel = dist(c.tip, l.tip);
        let fires = f.gap >= CONFIRM_GAP
            && (c.gap - l.gap) / dt.max(1e-3) >= FIRE_RATE
            && c.reach - l.reach >= REACH_GAIN
            && c.others - l.others <= OTHERS_GAIN
            && travel >= TIP_TRAVEL
            && dist(c.thumb, l.thumb) < THUMB_TRAVEL
            && c.proj.min(f.proj) >= super::shot::MIN_PROJ
            && c.straight.min(f.straight) >= FIRE_STRAIGHT
            && index.is_none_or(|(v, thr)| v >= thr)
            && self.shot_ns.is_none_or(|s| ct - s >= REFRACTORY_NS);
        if !fires {
            return None;
        }
        self.shot_ns = Some(ct);
        Some(Shot { at: c.at, dir: c.dir, strength: ((travel - TIP_TRAVEL) / 0.8).clamp(0.0, 1.0), t_ns: ct })
    }

    /// The hand is gone.
    pub fn lost(&mut self) {
        self.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::super::shot::synth::{foreshortened, hand, pinch, place, THUMB_LOAD, THUMB_OUT};
    use super::*;

    const FRAME_NS: u64 = 33_333_333;
    type Pose = [[f32; 2]; 21];

    /// Runs poses through a detector, one per frame from `t0`.
    fn run(f: &mut Flick, poses: &[Pose], t0: u64) -> Vec<Shot> {
        poses.iter().enumerate().filter_map(|(i, p)| f.update(p, None, t0 + i as u64 * FRAME_NS)).collect()
    }

    fn put(h: Pose, angle: f32) -> Pose {
        place(h, angle, 0.2, [0.5, 0.5])
    }

    fn loaded(angle: f32) -> Pose {
        put(hand(1.0, THUMB_LOAD, 0.0), angle)
    }

    fn out(angle: f32) -> Pose {
        put(hand(0.0, THUMB_LOAD, 0.0), angle)
    }

    /// Loaded for `hold` frames, then the index out in `steps` frames, the
    /// thumb staying, then held out.
    fn flick(angle: f32, hold: usize, steps: usize) -> Vec<Pose> {
        let mut v = vec![loaded(angle); hold];
        for k in 1..=steps {
            v.push(put(hand(1.0 - k as f32 / steps as f32, THUMB_LOAD, 0.0), angle));
        }
        v.extend(std::iter::repeat_n(out(angle), 5));
        v
    }

    #[test]
    fn the_synthetic_poses_read_as_meant() {
        let l = features(&loaded(0.0));
        let o = features(&out(0.0));
        assert!(l.is_some_and(|f| f.gap <= LOAD_GAP && f.reach <= LOAD_REACH), "{l:?}");
        assert!(o.is_some_and(|f| f.gap >= FIRE_GAP && f.straight >= 0.95), "{o:?}");
    }

    #[test]
    fn a_flick_fires_once_along_the_index() {
        for angle in [0.0f32, 0.6, -1.2, 2.5] {
            let mut f = Flick::default();
            let shots = run(&mut f, &flick(angle, 4, 1), 0);
            assert_eq!(shots.len(), 1, "angle {angle}");
            let d = shots[0].dir;
            let want = [angle.sin(), -angle.cos()];
            assert!(d[0] * want[0] + d[1] * want[1] > 0.99, "angle {angle}: {d:?}");
            assert_eq!(shots[0].t_ns, 4 * FRAME_NS, "the candidate frame's time");
            assert!(shots[0].strength > 0.0);
        }
    }

    #[test]
    fn a_flick_over_two_frames_fires_too() {
        let mut f = Flick::default();
        assert_eq!(run(&mut f, &flick(0.3, 3, 2), 0).len(), 1);
    }

    #[test]
    fn a_slow_opening_does_not_fire() {
        let mut f = Flick::default();
        assert!(run(&mut f, &flick(0.0, 4, 10), 0).is_empty());
    }

    #[test]
    fn a_pinch_release_does_not_fire_even_fast() {
        let mut poses = vec![put(pinch(0.1), 0.0); 5];
        poses.extend(std::iter::repeat_n(put(hand(0.1, THUMB_OUT, 0.0), 0.0), 4));
        let mut f = Flick::default();
        assert!(run(&mut f, &poses, 0).is_empty());
        let mut poses = vec![put(pinch(0.9), 0.0); 5];
        poses.extend(std::iter::repeat_n(put(hand(0.0, THUMB_OUT, 0.0), 0.0), 4));
        assert!(run(&mut f, &poses, 1_000_000_000).is_empty());
    }

    #[test]
    fn a_fist_springing_open_does_not_fire_an_ok_sign_does() {
        let mut f = Flick::default();
        let mut poses = vec![put(hand(1.0, THUMB_LOAD, 0.0), 0.0); 4];
        poses.extend(std::iter::repeat_n(put(hand(0.0, THUMB_LOAD, 1.0), 0.0), 4));
        assert!(run(&mut f, &poses, 0).is_empty(), "all fingers opened");
        let mut poses = vec![put(hand(1.0, THUMB_LOAD, 1.0), 0.0); 4];
        poses.extend(std::iter::repeat_n(put(hand(0.0, THUMB_LOAD, 1.0), 0.0), 4));
        assert_eq!(run(&mut f, &poses, 1_000_000_000).len(), 1, "the others were out already");
    }

    #[test]
    fn an_unloaded_hand_and_a_one_frame_load_do_not_fire() {
        let mut f = Flick::default();
        let mut poses = vec![put(hand(1.0, THUMB_OUT, 0.0), 0.0); 4];
        poses.extend(std::iter::repeat_n(out(0.0), 4));
        assert!(run(&mut f, &poses, 0).is_empty());
        assert!(run(&mut f, &flick(0.0, 1, 1), 1_000_000_000).is_empty());
    }

    #[test]
    fn a_held_or_moving_load_an_index_into_the_lens_and_an_outlier_do_not_fire() {
        let mut f = Flick::default();
        let poses: Vec<_> = (0..30).map(|i| place(hand(1.0, THUMB_LOAD, 0.0), 0.0, 0.2, [0.2 + i as f32 * 0.02, 0.75])).collect();
        assert!(run(&mut f, &poses, 0).is_empty());
        let mut poses = vec![loaded(0.0); 4];
        poses.extend(std::iter::repeat_n(put(foreshortened(0.3), 0.0), 4));
        assert!(run(&mut f, &poses, 2_000_000_000).is_empty());
        let mut poses = vec![loaded(0.0); 4];
        poses.push(out(0.0));
        poses.extend(std::iter::repeat_n(loaded(0.0), 4));
        assert!(run(&mut f, &poses, 4_000_000_000).is_empty());
    }

    #[test]
    fn a_second_flick_inside_the_refractory_time_is_dropped() {
        let mut f = Flick::default();
        let one = flick(0.0, 2, 1);
        let mut poses = one[..4].to_vec();
        poses.extend(one.iter().copied());
        // candidates at frames 2 and 6: 133 ms apart
        assert_eq!(run(&mut f, &poses, 0).len(), 1);
        assert_eq!(run(&mut f, &one, 2_000_000_000).len(), 1);
    }

    #[test]
    fn a_hand_lost_between_load_and_release_starts_over() {
        let mut f = Flick::default();
        let poses = flick(0.0, 3, 1);
        for (i, p) in poses.iter().take(3).enumerate() {
            assert!(f.update(p, None, i as u64 * FRAME_NS).is_none());
        }
        let t = 2 * FRAME_NS + 300_000_000;
        assert!(f.update(&poses[3], None, t).is_none());
        assert!(f.update(&poses[4], None, t + FRAME_NS).is_none());
    }

    #[test]
    fn the_index_value_has_to_fold_and_show() {
        let poses = flick(0.0, 4, 1);
        let mut f = Flick::default();
        let shots: Vec<_> = poses.iter().enumerate().filter_map(|(i, p)| f.update(p, Some((0.9, 0.5)), i as u64 * FRAME_NS)).collect();
        assert!(shots.is_empty());
        let mut f = Flick::default();
        let shots: Vec<_> = poses.iter().enumerate().filter_map(|(i, p)| f.update(p, Some((if i < 4 { 0.1 } else { 0.9 }, 0.5)), i as u64 * FRAME_NS)).collect();
        assert_eq!(shots.len(), 1);
    }

    #[test]
    fn broken_joints_reset_and_old_frames_are_ignored() {
        let mut f = Flick::default();
        let poses = flick(0.0, 3, 1);
        for (i, p) in poses.iter().take(3).enumerate() {
            f.update(p, None, i as u64 * FRAME_NS);
        }
        let mut nan = poses[3];
        nan[8][0] = f32::NAN;
        assert!(f.update(&nan, None, 3 * FRAME_NS).is_none());
        assert!(f.update(&poses[3], None, 4 * FRAME_NS).is_none(), "the NaN frame reset the load");
        assert!(f.update(&poses[3], None, FRAME_NS).is_none(), "an older frame");
    }
}
