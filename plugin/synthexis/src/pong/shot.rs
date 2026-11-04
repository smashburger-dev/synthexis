//! The Pong shot: pointing (Noa, 11-04). Each point shoots once: the
//! hand's debounced finger mask (the one the MIDI triggers use, with the
//! person's calibration) shows a point (index shown, middle, ring and
//! pinky folded, the thumb free), the point has held for HOLD_NS, and on
//! some frame of it the point is clear and the hand still (POINT_INDEX).
//! A flick is a fast point and shoots the same way. It flies from the
//! index tip along the index, fitted through its four joints. Opening all
//! fingers does not shoot; a hand back in view already pointing shoots
//! once its point has held.
//!
//! Joints are engine::SKELETON's: preview coordinates, mirrored, each axis
//! 0..1, written together with the mask and with the same capture time.

use super::arena::to_world;

const WRIST: usize = 0;
const THUMB_TIP: usize = 4;
const INDEX: [usize; 4] = [5, 6, 7, 8];
const MIDDLE_MCP: usize = 9;
const PINKY_MCP: usize = 17;
/// Joints that must not sit on the clamp border (a hand leaving the frame).
const KEY_JOINTS: [usize; 6] = [0, 4, 5, 8, 9, 17];
const BORDER: f32 = 0.002;
/// A hand smaller than this (palm, frame heights) is noise.
const MIN_PALM: f32 = 0.03;
/// The index knuckle-to-tip in the picture at least this long (palms): an
/// index pointing into the lens is too short to give a direction.
pub const MIN_PROJ: f32 = 0.55;
/// No second shot from a hand before this.
pub const REFRACTORY_NS: u64 = 250_000_000;
/// A hand unseen this long (capture time) starts over: its point starts
/// again.
pub const LOST_NS: u64 = 300_000_000;
/// A point has no speed of its own.
pub const POINT_STRENGTH: f32 = 0.8;
/// A point shoots only with the index's trigger value at least this, the
/// middle, ring and pinky values at most POINT_REST, and the hand's speed
/// (HandFeatures::speed, 0..1) at most POINT_SPEED. Chosen on S2 and S3
/// (probes/pong-shot/RESULT.md, follow-up): false shots 11.4 -> 2.0 per
/// minute with 5 of 6 index-alone segments kept; most false shots came
/// from throws, where the hand moves fast.
pub const POINT_INDEX: f32 = 0.9;
pub const POINT_REST: f32 = 0.3;
pub const POINT_SPEED: f32 = 0.25;
/// A point shoots only after its mask has shown a point this long
/// (capture time): fast hand motion makes point masks of 0.1 to 0.3 s.
/// Measured on S2 and S3 (probes/pong-shot/RESULT.md, hold): 300 ms gave
/// 1.5 false shots per minute, 100 ms 2.7. Noa chose 100 ms in FL (11-04
/// evening): 300 ms felt too slow, the extra false shots are acceptable.
pub const HOLD_NS: u64 = 100_000_000;

/// Mask bits (route::finger_bit order: thumb 0b10000 .. pinky 0b00001).
const INDEX_BIT: u8 = 0b01000;
const REST_BITS: u8 = 0b00111;

/// A shot from one hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    /// The index tip, preview coordinates.
    pub at: [f32; 2],
    /// Where the index points, physics units, length 1.
    pub dir: [f32; 2],
    /// 0..1, how hard (the flick's tip travel; fixed for a point).
    pub strength: f32,
    /// Capture time of the frame it fired on.
    pub t_ns: u64,
}

/// What one frame of a hand says (lengths in palms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feat {
    pub gap: f32,
    pub reach: f32,
    pub others: f32,
    /// Index tip and thumb tip against the palm centre.
    pub tip: [f32; 2],
    pub thumb: [f32; 2],
    pub proj: f32,
    pub straight: f32,
    pub dir: [f32; 2],
    /// The index tip in preview coordinates.
    pub at: [f32; 2],
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// Direction of the line through `pts` (least squares), pointing from the
/// first to the last.
fn fit_dir(pts: &[[f32; 2]]) -> [f32; 2] {
    let n = pts.len().max(1) as f32;
    let c = pts.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
    let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
    for p in pts {
        let d = sub(*p, c);
        sxx += d[0] * d[0];
        syy += d[1] * d[1];
        sxy += d[0] * d[1];
    }
    let a = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let d = [a.cos(), a.sin()];
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return [0.0, -1.0] };
    let ray = sub(*last, *first);
    if d[0] * ray[0] + d[1] * ray[1] < 0.0 {
        [-d[0], -d[1]]
    } else {
        d
    }
}

/// The features of one skeleton; None when it is not a usable hand (a
/// joint not a number, a key joint on the frame border, a tiny hand).
pub fn features(joints: &[[f32; 2]; 21]) -> Option<Feat> {
    if !joints.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let on_border = |v: f32| v <= BORDER || v >= 1.0 - BORDER;
    if KEY_JOINTS.iter().any(|&k| on_border(joints[k][0]) || on_border(joints[k][1])) {
        return None;
    }
    let j: Vec<[f32; 2]> = joints.iter().map(|p| to_world(*p)).collect();
    let palm = len(sub(j[MIDDLE_MCP], j[WRIST])).max(1.2 * len(sub(j[INDEX[0]], j[PINKY_MCP])));
    if palm < MIN_PALM {
        return None;
    }
    let centre = [0, 5, 9, 17].iter().fold([0.0, 0.0], |a, &k| [a[0] + j[k][0] / 4.0, a[1] + j[k][1] / 4.0]);
    let rel = |k: usize| {
        let d = sub(j[k], centre);
        [d[0] / palm, d[1] / palm]
    };
    let chain = len(sub(j[INDEX[1]], j[INDEX[0]])) + len(sub(j[INDEX[2]], j[INDEX[1]])) + len(sub(j[INDEX[3]], j[INDEX[2]]));
    let proj = len(sub(j[INDEX[3]], j[INDEX[0]]));
    let pts: Vec<[f32; 2]> = INDEX.iter().map(|&k| j[k]).collect();
    Some(Feat {
        gap: len(sub(j[THUMB_TIP], j[INDEX[3]])) / palm,
        reach: len(sub(j[INDEX[3]], j[WRIST])) / palm,
        others: [12, 16, 20].iter().map(|&t| len(sub(j[t], j[WRIST]))).sum::<f32>() / (3.0 * palm),
        tip: rel(INDEX[3]),
        thumb: rel(THUMB_TIP),
        proj: proj / palm,
        straight: if chain > 1e-6 { proj / chain } else { 0.0 },
        dir: fit_dir(&pts),
        at: joints[INDEX[3]],
    })
}

/// Index shown, middle, ring and pinky folded.
pub fn pointing(mask: u8) -> bool {
    mask & INDEX_BIT != 0 && mask & REST_BITS == 0
}

/// What else the frame knows about the hand: trigger finger values
/// (thumb..pinky) and speed, when perception gave them for this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hand {
    pub vals: Option<[f32; 5]>,
    pub speed: Option<f32>,
}

impl Hand {
    /// A clear point held still: see POINT_INDEX.
    fn steady_point(&self) -> bool {
        self.vals.is_none_or(|v| v[1] >= POINT_INDEX && v[2..].iter().all(|x| *x <= POINT_REST)) && self.speed.is_none_or(|s| s <= POINT_SPEED)
    }
}

/// The pointing shot of one hand.
#[derive(Clone, Debug)]
pub struct Point {
    seen_ns: Option<u64>,
    shot_ns: Option<u64>,
    /// The point under way: its first frame and whether it has shot.
    run: Option<(u64, bool)>,
    hold_ns: u64,
}

impl Default for Point {
    fn default() -> Self {
        Self::with_hold(HOLD_NS)
    }
}

impl Point {
    /// A detector whose point has to hold `hold_ns` (the probe's variants).
    pub fn with_hold(hold_ns: u64) -> Self {
        Self { seen_ns: None, shot_ns: None, run: None, hold_ns }
    }

    /// One frame of the hand: its debounced finger mask, the joints and
    /// what else is known of the same frame. A shot on the first frame of
    /// a point that has held, is clear and still, and gives a direction.
    pub fn update(&mut self, mask: u8, joints: &[[f32; 2]; 21], hand: Hand, t_ns: u64) -> Option<Shot> {
        if self.seen_ns.is_some_and(|s| t_ns <= s) {
            return None;
        }
        if self.seen_ns.is_some_and(|s| t_ns - s > LOST_NS) {
            self.run = None;
        }
        self.seen_ns = Some(t_ns);
        if !pointing(mask) {
            self.run = None;
            return None;
        }
        let (start, done) = *self.run.get_or_insert((t_ns, false));
        if done || t_ns - start < self.hold_ns || !hand.steady_point() || self.shot_ns.is_some_and(|s| t_ns - s < REFRACTORY_NS) {
            return None;
        }
        let f = features(joints).filter(|f| f.proj >= MIN_PROJ)?;
        self.run = Some((start, true));
        self.shot_ns = Some(t_ns);
        Some(Shot { at: f.at, dir: f.dir, strength: POINT_STRENGTH, t_ns })
    }

    /// The hand is gone.
    pub fn lost(&mut self) {
        self.run = None;
    }
}

#[cfg(test)]
pub(crate) mod synth {
    //! Synthetic hands for the tests: a right hand in its own frame (u to
    //! the right, v up, palm about 1), turned and placed in the picture.

    use super::super::arena::{to_preview, to_world};

    /// The thumb tip pressed under the curled index tip.
    pub const THUMB_LOAD: [f32; 2] = [-0.75, 0.90];
    /// The thumb tip out to the side.
    pub const THUMB_OUT: [f32; 2] = [-1.1, 0.55];

    fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }

    /// Joints: `curl` 0 straight index .. 1 curled onto the thumb, the
    /// thumb tip at `thumb`, middle, ring and pinky `others` 0 folded .. 1
    /// out.
    pub fn hand(curl: f32, thumb: [f32; 2], others: f32) -> [[f32; 2]; 21] {
        let straight = [[-0.35, 0.95], [-0.38, 1.35], [-0.40, 1.62], [-0.42, 1.88]];
        let curled = [[-0.35, 0.95], [-0.45, 1.28], [-0.62, 1.12], [-0.70, 0.94]];
        let mut j = [[0.0f32; 2]; 21];
        for (k, (s, c)) in straight.iter().zip(&curled).enumerate() {
            j[5 + k] = lerp(*s, *c, curl);
        }
        for (base, x) in [(9, 0.0f32), (13, 0.3), (17, 0.55)] {
            let b = 1.0 - x * 0.25;
            j[base] = [x, b];
            j[base + 1] = lerp([x, 1.2], [x, b + 0.4], others);
            j[base + 2] = lerp([x - 0.02, 1.0], [x, b + 0.65], others);
            j[base + 3] = lerp([x - 0.03, 0.85], [x, b + 0.88], others);
        }
        j[1] = [-0.4, 0.2];
        j[2] = [-0.7, 0.45];
        j[3] = [-0.8, 0.68];
        j[4] = thumb;
        j
    }

    /// An index pointing into the lens: straight but `k` as long in the
    /// picture.
    pub fn foreshortened(k: f32) -> [[f32; 2]; 21] {
        let mut j = hand(0.0, THUMB_LOAD, 0.0);
        for i in 6..=8 {
            j[i] = lerp(j[5], j[i], k);
        }
        j
    }

    /// `h` turned by `angle` (rad, 0 = index up, positive clockwise in the
    /// picture), palm `scale` frame heights, wrist at `at` (preview).
    pub fn place(h: [[f32; 2]; 21], angle: f32, scale: f32, at: [f32; 2]) -> [[f32; 2]; 21] {
        let (s, c) = angle.sin_cos();
        let w = to_world(at);
        // picture y runs down: v up is -y
        h.map(|[u, v]| to_preview([w[0] + (u * c + v * s) * scale, w[1] + (u * s - v * c) * scale]))
    }
}

#[cfg(test)]
mod tests {
    use super::synth::{foreshortened, hand, place, THUMB_OUT};
    use super::*;

    const FRAME_NS: u64 = 33_333_333;
    const FIST: u8 = 0b00000;
    const POINT: u8 = 0b01000;
    const GUN: u8 = 0b11000;
    const OPEN: u8 = 0b11111;

    fn pose(angle: f32) -> [[f32; 2]; 21] {
        place(hand(0.0, THUMB_OUT, 0.0), angle, 0.2, [0.5, 0.5])
    }

    fn run(p: &mut Point, masks: &[u8], angle: f32, t0: u64) -> Vec<Shot> {
        masks.iter().enumerate().filter_map(|(i, m)| p.update(*m, &pose(angle), Hand::default(), t0 + i as u64 * FRAME_NS)).collect()
    }

    fn frames(shots: &[Shot]) -> Vec<u64> {
        shots.iter().map(|s| s.t_ns / FRAME_NS).collect()
    }

    #[test]
    fn pointing_is_the_index_without_middle_ring_and_pinky() {
        assert!(pointing(POINT) && pointing(GUN));
        assert!(!pointing(FIST) && !pointing(OPEN) && !pointing(0b01100) && !pointing(0b10000));
    }

    #[test]
    fn a_point_shoots_once_along_the_index() {
        for angle in [0.0f32, 0.7, -1.4, 3.0] {
            let mut p = Point::with_hold(0);
            let shots = run(&mut p, &[FIST, FIST, POINT, POINT, POINT, GUN, POINT], angle, 0);
            assert_eq!(frames(&shots), vec![2], "angle {angle}");
            let d = shots[0].dir;
            let want = [angle.sin(), -angle.cos()];
            assert!(d[0] * want[0] + d[1] * want[1] > 0.99, "angle {angle}: {d:?}");
            let tip = pose(angle)[8];
            assert!((shots[0].at[0] - tip[0]).abs() < 1e-5 && (shots[0].at[1] - tip[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn the_point_has_to_hold() {
        let first = (1..).find(|k: &u64| (k - 1) * FRAME_NS >= HOLD_NS).unwrap_or(1);
        let mut p = Point::default();
        let shots = run(&mut p, &[FIST; 1].iter().chain([POINT; 20].iter()).copied().collect::<Vec<_>>(), 0.0, 0);
        assert_eq!(frames(&shots), vec![first]);
        if HOLD_NS > FRAME_NS {
            let mut p = Point::default();
            assert!(run(&mut p, &[FIST, POINT, POINT, FIST, FIST], 0.0, 0).is_empty(), "a point shorter than the hold");
        }
    }

    #[test]
    fn a_point_that_becomes_clear_and_still_later_shoots_then() {
        // Noa, 11-04: pointing slowly did not shoot; only the frame the mask
        // turned was checked, and the index was not straight yet there.
        let vague = Hand { vals: Some([0.2, 0.7, 0.1, 0.1, 0.1]), speed: Some(0.1) };
        let moving = Hand { vals: Some([0.2, 0.95, 0.1, 0.1, 0.1]), speed: Some(0.5) };
        let clear = Hand { vals: Some([0.2, 0.95, 0.1, 0.1, 0.1]), speed: Some(0.1) };
        let mut seq = vec![(FIST, clear), (POINT, vague), (POINT, vague), (POINT, moving), (POINT, moving), (POINT, vague)];
        seq.extend([(POINT, clear); 20]);
        let mut p = Point::default();
        let shots: Vec<u64> = seq.iter().enumerate().filter_map(|(i, (m, h))| p.update(*m, &pose(0.0), *h, i as u64 * FRAME_NS)).map(|s| s.t_ns / FRAME_NS).collect();
        let held = (1..).find(|k: &u64| (k - 1) * FRAME_NS >= HOLD_NS).unwrap_or(1);
        assert_eq!(shots, vec![held.max(6)]);
    }

    #[test]
    fn opening_the_hand_does_not_shoot_and_a_hand_back_pointing_does_after_the_hold() {
        let mut p = Point::default();
        assert!(run(&mut p, &[FIST, OPEN, OPEN, 0b01100, OPEN], 0.0, 0).is_empty());
        // gone 400 ms, back pointing: a new point
        let t0 = 4 * FRAME_NS + 400_000_000;
        let shots = run(&mut p, &[POINT; 20], 0.0, t0);
        assert_eq!(shots.len(), 1);
        assert!(shots[0].t_ns >= t0 + HOLD_NS);
    }

    #[test]
    fn pointing_again_shoots_again_after_the_refractory_time() {
        let mut p = Point::with_hold(0);
        // folded and shown again 100 ms later: dropped; 400 ms later: shoots
        let shots = run(&mut p, &[FIST, POINT, FIST, FIST, POINT, FIST, FIST, FIST, FIST, FIST, FIST, FIST, FIST, POINT], 0.0, 0);
        assert_eq!(frames(&shots), vec![1, 13]);
    }

    #[test]
    fn a_vague_or_moving_point_does_not_shoot() {
        let clear = Hand { vals: Some([0.2, 0.95, 0.1, 0.1, 0.1]), speed: Some(0.1) };
        let cases = [
            (Hand { vals: Some([0.2, 0.7, 0.1, 0.1, 0.1]), ..clear }, false),
            (Hand { vals: Some([0.2, 0.95, 0.4, 0.1, 0.1]), ..clear }, false),
            (Hand { speed: Some(0.5), ..clear }, false),
            (clear, true),
        ];
        for (hand, shoots) in cases {
            let mut p = Point::with_hold(0);
            assert!(p.update(FIST, &pose(0.0), hand, 0).is_none());
            assert_eq!(p.update(POINT, &pose(0.0), hand, FRAME_NS).is_some(), shoots, "{hand:?}");
        }
    }

    #[test]
    fn no_direction_no_shot() {
        let mut p = Point::with_hold(0);
        let into_lens = place(foreshortened(0.3), 0.0, 0.2, [0.5, 0.75]);
        assert!(p.update(FIST, &into_lens, Hand::default(), 0).is_none());
        assert!(p.update(POINT, &into_lens, Hand::default(), FRAME_NS).is_none());
        let mut bad = pose(0.0);
        bad[6][1] = f32::NAN;
        let mut p = Point::with_hold(0);
        p.update(FIST, &bad, Hand::default(), 0);
        assert!(p.update(POINT, &bad, Hand::default(), FRAME_NS).is_none());
        assert!(features(&place(hand(0.0, THUMB_OUT, 0.0), 0.0, 0.2, [0.5, 1.0])).is_none(), "wrist on the border");
    }
}
