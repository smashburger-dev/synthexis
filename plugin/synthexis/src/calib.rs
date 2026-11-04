//! Finger calibration (probes/finger-shapes/RESULT.md): seven poses, both
//! hands at once, about 3 s each: fist, open, then each finger alone. Per
//! hand, pose and finger it takes the median trigger value
//! (geom::trigger_fingers); each finger's threshold goes halfway between
//! the level of its own pose and the highest pose that has it folded, so
//! every single finger and every combination is judged against this
//! person's own hand and camera. Kept per user in settings.json, shared by
//! every instance and project.
//!
//! What a threshold cannot do (Noa, FL 11-04): when the hand student reads
//! a pose as another finger (middle alone as index: index 0.93 to 1.00,
//! middle 0.03 to 0.10 on both hands), no threshold turns it back. Such a
//! pose is left out of that finger's threshold and reported in
//! `Outcome::misread`. When the student's skeletons of the misread pose
//! and of the pose it reads as still differ on held-out frames
//! (`MIN_APART`), the person's mean skeletons become templates
//! (geom::MaskTemplates) that the engine uses to tell the two apart
//! (docs/session-2026-11-04-fingers.md).

use std::time::Instant;

use crate::engine::geom::{bones, bones_dist, Bones, MaskTemplates};

/// The poses in order, as masks thumb (0b10000) .. pinky (0b00001).
pub const POSES: [u8; 7] = [0b00000, 0b11111, 0b10000, 0b01000, 0b00100, 0b00010, 0b00001];
/// Seconds per pose before collecting starts, and collecting.
const SETTLE_S: f32 = 0.9;
const HOLD_S: f32 = 2.0;
/// A pose also waits for this many frames of one hand, up to
/// `MAX_HOLD_S`: a stalled perception thread (11-04: Apple Vision loading
/// for 8 s) left fist and open with 0 and 1 frames.
const POSE_FRAMES: usize = 20;
const MAX_HOLD_S: f32 = 8.0;
/// Levels closer than this tell nothing: that finger keeps 0.5.
const MIN_GAP: f32 = 0.12;
const MIN_SAMPLES: usize = 8;

/// A pose joins the skeleton templates (geom::MaskTemplates) only when
/// the second half of its frames falls nearer its own mean skeleton than
/// the read pose's (both from the first halves) this often.
pub const MIN_APART: f32 = 0.9;

#[derive(Default)]
struct PoseData {
    /// [side]: trigger values per frame.
    vals: [Vec<[f32; 5]>; 2],
    /// [side]: the student's skeleton per frame, for the misread log.
    bones: [Vec<Bones>; 2],
    /// [side]: the student's and the geometric values per frame, for the log.
    raw: [Vec<[[f32; 5]; 2]>; 2],
}

pub struct Calib {
    pose: usize,
    since: Instant,
    poses: Vec<PoseData>,
    last: [u64; 2],
    raw_last: [u64; 2],
}

/// What a finished run found.
#[derive(Debug)]
pub struct Outcome {
    pub thresholds: [[f32; 5]; 2],
    /// Hands with no samples keep their old thresholds.
    pub seen: [bool; 2],
    /// (side, finger) that neither its own pose nor the open hand reads
    /// above the poses with it folded: it keeps 0.5.
    pub hard: Vec<(usize, usize)>,
    /// (side, pose mask, mask read with the new thresholds) for poses the
    /// camera reads as another mask.
    pub misread: Vec<(usize, u8, u8)>,
    /// Per hand the skeleton templates for misread poses its skeletons
    /// tell apart (`MIN_APART`); the engine uses them right away.
    pub templates: [MaskTemplates; 2],
}

fn median(v: impl Iterator<Item = f32>) -> Option<f32> {
    let mut s: Vec<f32> = v.filter(|x| x.is_finite()).collect();
    if s.len() < MIN_SAMPLES {
        return None;
    }
    s.sort_by(f32::total_cmp);
    s.get(s.len() / 2).copied()
}

fn mean(v: &[Bones]) -> Bones {
    let mut c = [0.0f32; 42];
    for x in v {
        c.iter_mut().zip(x.iter()).for_each(|(c, x)| *c += x / v.len() as f32);
    }
    c
}

/// Held-out check: mean skeletons from the first half of `a` and of `b`,
/// the share of second-half frames nearer their own mean. Halves in time,
/// not alternate frames: neighbouring frames are near copies and would
/// flatter the check. None with too few frames.
fn apart(a: &[Bones], b: &[Bones]) -> Option<f32> {
    if a.len() < 2 * MIN_SAMPLES || b.len() < 2 * MIN_SAMPLES {
        return None;
    }
    let (a0, a1) = a.split_at(a.len() / 2);
    let (b0, b1) = b.split_at(b.len() / 2);
    let (ca, cb) = (mean(a0), mean(b0));
    let near = |v: &[Bones], own: &Bones, other: &Bones| v.iter().filter(|x| bones_dist(x, own) < bones_dist(x, other)).count();
    Some((near(a1, &ca, &cb) + near(b1, &cb, &ca)) as f32 / (a1.len() + b1.len()) as f32)
}

impl Default for Calib {
    fn default() -> Self {
        Calib::new(Instant::now())
    }
}

impl Calib {
    fn new(now: Instant) -> Self {
        Calib { pose: 0, since: now, poses: (0..POSES.len()).map(|_| PoseData::default()).collect(), last: [0; 2], raw_last: [0; 2] }
    }

    /// Frames of the hand with the most in the current pose.
    fn frames(&self) -> usize {
        self.poses.get(self.pose).map_or(0, |p| p.vals.iter().map(Vec::len).max().unwrap_or(0))
    }

    /// Stops the run (Reset while it runs): what it collected goes, the
    /// thresholds and templates stay as they were. The caller drops its
    /// Calib with this, so no further tick can finish it.
    pub fn cancel(self) {
        crate::log::log(&format!("calib: cancelled at pose {} of {}", self.pose + 1, POSES.len()));
    }

    /// The pose to show now and how far it is (0..1).
    pub fn now(&self) -> (usize, f32) {
        let t = self.since.elapsed().as_secs_f32() / (SETTLE_S + HOLD_S);
        let frames = (SETTLE_S + HOLD_S * self.frames() as f32 / POSE_FRAMES as f32) / (SETTLE_S + HOLD_S);
        (self.pose, t.min(frames).min(1.0))
    }

    /// Takes the newest finger values of both hands; Some when the last
    /// pose is done.
    pub fn tick(&mut self, values: [Option<([f32; 5], u64)>; 2], old: [[f32; 5]; 2]) -> Option<Outcome> {
        self.tick_at(values, old, Instant::now())
    }

    fn tick_at(&mut self, values: [Option<([f32; 5], u64)>; 2], old: [[f32; 5]; 2], now: Instant) -> Option<Outcome> {
        let t = now.saturating_duration_since(self.since).as_secs_f32();
        if t >= SETTLE_S {
            self.collect(values);
        }
        let held = t >= SETTLE_S + HOLD_S && (self.frames() >= POSE_FRAMES || t >= SETTLE_S + MAX_HOLD_S);
        if !held {
            return None;
        }
        self.pose += 1;
        self.since = now;
        (self.pose == POSES.len()).then(|| {
            let out = self.outcome(old);
            self.log(&out);
            for (side, t) in out.templates.iter().enumerate() {
                if out.seen[side] {
                    crate::engine::MASK_TEMPLATES.lock()[side] = t.clone();
                }
            }
            out
        })
    }

    fn collect(&mut self, values: [Option<([f32; 5], u64)>; 2]) {
        let raw = *crate::engine::FINGER_RAW.lock();
        let skeleton = *crate::engine::SKELETON.lock();
        let Some(p) = self.poses.get_mut(self.pose) else { return };
        for (side, r) in raw.iter().enumerate() {
            if let Some((v, at)) = r.filter(|(_, at)| *at != self.raw_last[side]) {
                self.raw_last[side] = at;
                p.raw[side].push(v);
            }
        }
        for (side, v) in values.iter().enumerate() {
            let Some((vals, at)) = v else { continue };
            if *at == self.last[side] {
                continue;
            }
            self.last[side] = *at;
            p.vals[side].push(*vals);
            if let Some((j, _)) = skeleton[side].filter(|(_, t)| t == at) {
                p.bones[side].push(bones(&j));
            }
        }
    }

    /// Median of finger `f` in pose `p` for `side`.
    fn med(&self, p: usize, side: usize, f: usize) -> Option<f32> {
        median(self.poses.get(p)?.vals[side].iter().map(|v| v[f]))
    }

    pub fn outcome(&self, old: [[f32; 5]; 2]) -> Outcome {
        let mut out = Outcome {
            thresholds: old,
            seen: [false; 2],
            hard: Vec::new(),
            misread: Vec::new(),
            templates: [MaskTemplates::EMPTY, MaskTemplates::EMPTY],
        };
        for side in 0..2 {
            out.seen[side] = (0..POSES.len()).any(|p| self.poses[p].vals[side].len() >= MIN_SAMPLES);
            if !out.seen[side] {
                continue;
            }
            for f in 0..5 {
                out.thresholds[side][f] = self.threshold(side, f).unwrap_or_else(|| {
                    out.hard.push((side, f));
                    0.5
                });
            }
            for (p, &mask) in POSES.iter().enumerate() {
                let Some(m) = (0..5).map(|f| self.med(p, side, f)).collect::<Option<Vec<f32>>>() else { continue };
                // read as the gate does: lit only above th + band, off
                // only below th - band; a pose in between is a misread too
                let band = crate::engine::geom::SHAPE_BAND;
                let read_at = |off: f32| {
                    m.iter()
                        .zip(out.thresholds[side])
                        .enumerate()
                        .filter(|(_, (v, th))| **v > *th + off)
                        .fold(0u8, |r, (f, _)| r | crate::route::finger_bit(f))
                };
                let (lit, not_off) = (read_at(band), read_at(-band));
                if lit != mask {
                    out.misread.push((side, mask, lit));
                } else if not_off != mask {
                    out.misread.push((side, mask, not_off));
                }
            }
            out.templates[side] = self.templates(side, &out.misread);
        }
        out
    }

    /// The skeleton frames of the pose with mask `m` for `side`.
    fn bones_of(&self, m: u8, side: usize) -> Option<&[Bones]> {
        POSES.iter().position(|p| *p == m).map(|p| self.poses[p].bones[side].as_slice())
    }

    /// Per read mask of a misread pose: the read pose's and every misread
    /// pose's mean skeleton, for the misread poses that pass `apart`.
    fn templates(&self, side: usize, misread: &[(usize, u8, u8)]) -> MaskTemplates {
        let mut t = MaskTemplates::EMPTY;
        for &(s, mask, read) in misread {
            let (Some(a), Some(b)) = (self.bones_of(mask, side), self.bones_of(read, side)) else { continue };
            if s != side || apart(a, b).is_none_or(|x| x < MIN_APART) {
                continue;
            }
            match t.sets.iter_mut().find(|(r, _)| *r == read) {
                Some((_, cands)) => cands.push((mask, mean(a))),
                None => t.sets.push((read, vec![(read, mean(b)), (mask, mean(a))])),
            }
        }
        t
    }

    /// The threshold of finger `f`: halfway between its shown level (its
    /// own pose, and the open hand when that also reads it shown) and the
    /// highest pose with it folded, when the two lie at least twice the
    /// gate's band (geom::SHAPE_BAND) apart. A folded pose that reads the finger as
    /// shown (above 0.5 and nearer the shown level than the lowest folded
    /// pose) is a misread and left out: on 11-04 the middle and ring poses
    /// read index 0.85 to 0.98, which put the right index threshold at
    /// 0.85, so the index itself barely lit. None when nothing reads the
    /// finger shown.
    fn threshold(&self, side: usize, f: usize) -> Option<f32> {
        let bit = crate::route::finger_bit(f);
        let folded: Vec<f32> = (0..POSES.len()).filter(|&p| POSES[p] & bit == 0).filter_map(|p| self.med(p, side, f)).collect();
        let low = folded.iter().copied().reduce(f32::min)?;
        let own = self.med(2 + f, side, f).filter(|v| v - low >= MIN_GAP);
        let open = self.med(1, side, f).filter(|v| v - low >= MIN_GAP);
        let shown = match (own, open) {
            (Some(a), Some(b)) => a.min(b),
            (a, b) => a.or(b)?,
        };
        let mid = ((shown + low) / 2.0).max(0.5);
        let high = folded.iter().copied().filter(|v| *v < mid).fold(low, f32::max);
        // both levels must clear the gate's hysteresis band, else the
        // finger never lights from its own pose or never goes off again
        let band = crate::engine::geom::SHAPE_BAND;
        (shown - high >= 2.0 * band).then(|| ((shown + high) / 2.0).clamp(0.15, 0.85).max(high + band).min(shown - band))
    }

    /// One log line per pose and hand: frames seen, the median of the
    /// student's and of the geometric value per finger thumb..pinky; one
    /// per misread pose, with how often the student's skeletons of the
    /// pose and of the pose it reads as fall nearer their own (split half).
    fn log(&self, out: &Outcome) {
        let med = |v: &[[[f32; 5]; 2]], src: usize, f: usize| {
            let mut x: Vec<f32> = v.iter().map(|r| r[src][f]).filter(|x| x.is_finite()).collect();
            x.sort_by(f32::total_cmp);
            x.get(x.len() / 2).copied().unwrap_or(f32::NAN)
        };
        for (pose, data) in self.poses.iter().enumerate() {
            for (side, v) in data.raw.iter().enumerate() {
                let fmt = |src: usize| (0..5).map(|f| format!("{:.2}", med(v, src, f))).collect::<Vec<_>>().join(" ");
                crate::log::log(&format!(
                    "calib: pose {:05b} hand {} frames {} student [{}] geometry [{}]",
                    POSES[pose],
                    ["L", "R"][side],
                    v.len(),
                    fmt(0),
                    fmt(1)
                ));
            }
        }
        for &(side, mask, read) in &out.misread {
            let sep = self.bones_of(mask, side).zip(self.bones_of(read, side)).and_then(|(a, b)| apart(a, b));
            let used = out.templates[side].sets.iter().any(|(r, c)| *r == read && c.iter().any(|(m, _)| *m == mask));
            crate::log::log(&format!(
                "calib: hand {} pose {mask:05b} reads as {read:05b}, skeletons apart {}, template {}",
                ["L", "R"][side],
                sep.map_or("n/a".into(), |s| format!("{s:.2}")),
                if used { "on" } else { "off" }
            ));
        }
        crate::log::log(&format!(
            "calib: seen L {} R {}, thresholds {:?}, hard {:?}",
            out.seen[0], out.seen[1], out.thresholds, out.hard
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A run with `frames` frames per pose of the values `at(pose, side)`.
    fn run(frames: usize, at: impl Fn(usize, usize) -> Option<[f32; 5]>) -> Calib {
        let mut c = Calib::default();
        for (p, data) in c.poses.iter_mut().enumerate() {
            for side in 0..2 {
                if let Some(v) = at(p, side) {
                    data.vals[side] = vec![v; frames];
                }
            }
        }
        c
    }

    #[test]
    fn thresholds_sit_between_folded_and_shown() {
        // right hand: shown 0.8, folded 0.4; ring barely moves
        let c = run(10, |p, side| {
            (side == 1).then(|| {
                let mut v = [0.0; 5];
                for (f, x) in v.iter_mut().enumerate() {
                    let on = POSES[p] & crate::route::finger_bit(f) != 0;
                    *x = match (f, on) {
                        (3, true) => 0.52,
                        (3, false) => 0.48,
                        (_, true) => 0.8,
                        (_, false) => 0.4,
                    };
                }
                v
            })
        });
        let out = c.outcome([[0.5; 5]; 2]);
        assert_eq!(out.seen, [false, true]);
        assert!((out.thresholds[1][1] - 0.6).abs() < 1e-6 && (out.thresholds[1][0] - 0.6).abs() < 1e-6);
        assert!((out.thresholds[1][3] - 0.5).abs() < 1e-6);
        assert_eq!(out.hard, vec![(1, 3)]);
        // the ring sits inside the gate's band at 0.5 in every pose, so
        // every pose is reported
        assert_eq!(out.misread.len(), POSES.len(), "{:?}", out.misread);
        assert!(out.misread.iter().all(|m| m.0 == 1));
        // the unseen left hand keeps what it had
        let out = c.outcome([[0.4; 5], [0.5; 5]]);
        assert_eq!(out.thresholds[0], [0.4; 5]);
    }

    /// Noa's run of 11-04 (log 1793797094): per pose the mean of the
    /// student's and the geometric median, thumb..pinky. Fist and open got
    /// 0 and 1 frames (perception stalled) and are left out.
    fn noa_11_04(p: usize, side: usize) -> Option<[f32; 5]> {
        let mean = |s: [f32; 5], g: [f32; 5]| {
            let mut v = [0.0; 5];
            for f in 0..5 {
                v[f] = (s[f] + g[f]) / 2.0;
            }
            v
        };
        let l = [
            ([1.00, 0.00, 0.00, 0.00, 0.00], [0.37, 0.00, 0.00, 0.00, 0.00]),
            ([0.00, 1.00, 0.00, 0.00, 0.00], [0.03, 0.94, 0.03, 0.00, 0.00]),
            ([0.00, 0.98, 0.05, 0.00, 0.00], [0.00, 0.93, 0.10, 0.00, 0.00]),
            ([0.00, 0.96, 0.00, 0.00, 0.18], [0.20, 0.88, 0.14, 0.12, 0.32]),
            ([0.00, 0.14, 0.00, 0.00, 0.59], [0.21, 0.57, 0.12, 0.04, 0.29]),
        ];
        let r = [
            ([1.00, 0.01, 0.00, 0.00, 0.00], [0.62, 0.00, 0.00, 0.00, 0.00]),
            ([0.00, 1.00, 0.00, 0.00, 0.00], [0.50, 1.00, 0.00, 0.00, 0.00]),
            ([0.00, 0.96, 0.01, 0.00, 0.00], [0.00, 1.00, 0.03, 0.03, 0.00]),
            ([0.00, 0.70, 0.00, 0.00, 0.02], [0.28, 0.99, 0.10, 0.13, 0.27]),
            ([0.02, 0.00, 0.00, 0.00, 1.00], [0.44, 0.12, 0.03, 0.03, 0.98]),
        ];
        let (s, g) = [l, r][side].get(p.checked_sub(2)?).copied()?;
        Some(mean(s, g))
    }

    #[test]
    fn a_pose_read_as_another_finger_is_reported_and_left_out() {
        let out = run(30, noa_11_04).outcome([[0.5; 5]; 2]);
        assert_eq!(out.seen, [true, true]);
        let [l, r] = out.thresholds;
        // the index keeps a threshold its own pose clears by far (was 0.85
        // on the right, so the index needed 0.95 to light)
        assert!(r[1] < 0.6 && l[1] < 0.7, "{l:?} {r:?}");
        // the left pinky pose (0.44) lies only 0.19 above the ring pose
        // (0.25): inside the gate's band, so no threshold of its own (was
        // 0.22, which kept the pinky lit in the ring pose) and the pose
        // is reported
        assert!(out.hard.contains(&(0, 4)) && (l[4] - 0.5).abs() < 1e-6, "{l:?}");
        assert!(out.misread.iter().any(|m| m.0 == 0 && m.1 == 0b00001), "{:?}", out.misread);
        // middle and ring alone read as index on both hands; their own
        // fingers get no threshold
        for side in 0..2 {
            assert!(out.misread.contains(&(side, 0b00100, 0b01000)), "{:?}", out.misread);
            assert!(out.misread.contains(&(side, 0b00010, 0b01000)), "{:?}", out.misread);
            assert!(out.hard.contains(&(side, 2)) && out.hard.contains(&(side, 3)), "{:?}", out.hard);
        }
        // thumb, index and pinky alone read right on the right hand
        assert!(!out.misread.iter().any(|m| m.0 == 1 && [0b10000, 0b01000, 0b00001].contains(&m.1)), "{:?}", out.misread);
    }

    #[test]
    fn a_pose_waits_for_frames_when_perception_stalls() {
        let t0 = Instant::now();
        let mut c = Calib::new(t0);
        let at = |s: f32| t0 + Duration::from_secs_f32(s);
        let none = [None, None];
        // no frames for 4 s: still the fist
        assert!(c.tick_at(none, [[0.5; 5]; 2], at(4.0)).is_none());
        assert_eq!(c.pose, 0);
        // frames come in: the pose ends once it has enough
        for k in 0..POSE_FRAMES as u64 {
            let v = [Some(([0.1; 5], k + 1)), None];
            assert!(c.tick_at(v, [[0.5; 5]; 2], at(4.0 + k as f32 * 0.03)).is_none());
        }
        assert_eq!(c.poses[0].vals[0].len(), POSE_FRAMES);
        assert!(c.tick_at(none, [[0.5; 5]; 2], at(5.0)).is_none());
        assert_eq!(c.pose, 1);
        // with no hand at all a pose gives up after MAX_HOLD_S
        assert!(c.tick_at(none, [[0.5; 5]; 2], at(5.0 + SETTLE_S + MAX_HOLD_S)).is_none());
        assert_eq!(c.pose, 2);
    }

    /// A skeleton: wrist, middle base straight up, the index tip leaning
    /// `lean` palm lengths sideways.
    fn hand(lean: f32) -> [[f32; 2]; 21] {
        let mut j = [[0.5f32, 0.5]; 21];
        j[9] = [0.5, 0.4];
        j[8] = [0.5 + lean * 0.1, 0.3];
        j
    }

    #[test]
    fn middle_alone_gets_a_template_only_when_its_skeleton_differs() {
        let mut c = run(30, noa_11_04);
        // right hand: middle alone has its own skeleton, ring alone the
        // index pose's one
        for (p, lean) in [(3, 0.0), (4, 0.4), (5, 0.0)] {
            c.poses[p].bones[1] = (0..30).map(|k| bones(&hand(lean + 0.01 * (k % 3) as f32))).collect();
        }
        let out = c.outcome([[0.5; 5]; 2]);
        let sets = &out.templates[1].sets;
        assert_eq!(sets.len(), 1, "{sets:?}");
        let poses: Vec<u8> = sets[0].1.iter().map(|(m, _)| *m).collect();
        assert_eq!((sets[0].0, poses), (0b01000, vec![0b01000, 0b00100]));
        // the left hand has no skeletons: no templates
        assert!(out.templates[0].sets.is_empty());
        // the templates turn a skeleton like the middle pose into middle,
        // one like the index pose stays a point
        let t = &out.templates[1];
        assert_eq!(t.pick(0b01000, &bones(&hand(0.4)), None), 0b00100);
        assert_eq!(t.pick(0b01000, &bones(&hand(0.0)), None), 0b01000);
        // halfway it keeps the last pick; other masks pass through
        assert_eq!(t.pick(0b01000, &bones(&hand(0.2)), Some(0b00100)), 0b00100);
        assert_eq!(t.pick(0b01000, &bones(&hand(0.2)), None), 0b01000);
        assert_eq!(t.pick(0b01100, &bones(&hand(0.4)), None), 0b01100);
    }

    #[test]
    fn skeletons_of_two_poses_are_told_apart() {
        let mut hand = [[0.5f32, 0.5]; 21];
        hand[9] = [0.5, 0.4];
        let mut moved = hand;
        moved[8] = [0.55, 0.3];
        let b = bones(&hand);
        // wrist at 0, middle base straight up one palm length
        assert!(b[0].abs() < 1e-6 && b[1].abs() < 1e-6 && b[18].abs() < 1e-6 && (b[19] + 1.0).abs() < 1e-6, "{b:?}");
        let a = vec![bones(&hand); 20];
        let m = vec![bones(&moved); 20];
        assert_eq!(apart(&a, &m), Some(1.0));
        assert_eq!(apart(&a[..4], &m), None);
    }
}
