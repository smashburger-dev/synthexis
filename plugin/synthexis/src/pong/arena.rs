//! The Pong arena: projectiles in the camera frame that bounce off its four
//! sides like the old DVD logo (no gravity, no friction). Pure and
//! deterministic; the Pong thread steps it, tests drive it by hand.
//!
//! Coordinates: preview coordinates, x 0..1 in the person's view (mirrored
//! like the preview and `engine::SKELETON`), y 0..1 from the top. Physics
//! runs with x scaled by ASPECT, so the frame is 16/9 wide and 1 high and an
//! angle in the picture is the angle a projectile flies. Speeds are in frame
//! heights per second.

use serde::{Deserialize, Serialize};

/// Width of the frame in physics units (its height is 1).
pub const ASPECT: f32 = 16.0 / 9.0;
/// Wall hits one step handles per projectile; past that the rest of the
/// step is dropped (only at absurd speeds, the settings stop at 4 + 40 %).
const MAX_HITS_PER_STEP: u32 = 16;
/// Both walls this close (units) at a hit: a corner, one hit per side.
const CORNER: f32 = 1e-5;
/// A shot starts at least this far inside the frame.
const MARGIN: f32 = 0.01;

/// A side of the frame, clockwise from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A preview point in physics units.
pub fn to_world(p: [f32; 2]) -> [f32; 2] {
    [p[0] * ASPECT, p[1]]
}

/// A physics point in preview coordinates.
pub fn to_preview(p: [f32; 2]) -> [f32; 2] {
    [p[0] / ASPECT, p[1]]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projectile {
    pub id: u64,
    /// Physics units.
    pub pos: [f32; 2],
    /// Frame heights per second.
    pub vel: [f32; 2],
    pub bounces: u32,
    /// Seconds since the shot.
    pub age: f32,
    /// 0..1, how hard the shot was (depth and velocity of its hits).
    pub strength: f32,
}

/// One wall hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub side: Side,
    /// 0..1 along the side: top and bottom from the left to the right of
    /// the picture, left and right from the bottom up (like hand y).
    pub pos: f32,
    /// The projectile's speed, frame heights per second.
    pub speed: f32,
    /// The shot's strength, 0..1.
    pub strength: f32,
    pub projectile: u64,
}

/// When a projectile is gone: after this many wall hits (the last one
/// still fires) or this many seconds; None is never.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Life {
    pub bounces: Option<u32>,
    pub seconds: Option<f32>,
}

#[derive(Debug, Default)]
pub struct Arena {
    shots: Vec<Projectile>,
    next_id: u64,
}

impl Arena {
    /// Fires a projectile from preview point `at` along `dir` (physics
    /// units, any length) at `speed` heights per second. Over `max` at once
    /// the oldest goes. None for a shot that is not a number.
    pub fn shoot(&mut self, at: [f32; 2], dir: [f32; 2], speed: f32, strength: f32, max: usize) -> Option<u64> {
        if !(at.iter().chain(&dir).all(|v| v.is_finite()) && speed.is_finite() && strength.is_finite()) {
            return None;
        }
        let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
        // no direction: straight up
        let d = if len > 1e-6 { [dir[0] / len, dir[1] / len] } else { [0.0, -1.0] };
        let p = to_world(at);
        let pos = [p[0].clamp(MARGIN, ASPECT - MARGIN), p[1].clamp(MARGIN, 1.0 - MARGIN)];
        let speed = speed.max(0.0);
        self.next_id += 1;
        let id = self.next_id;
        self.shots.push(Projectile { id, pos, vel: [d[0] * speed, d[1] * speed], bounces: 0, age: 0.0, strength: strength.clamp(0.0, 1.0) });
        let over = self.shots.len().saturating_sub(max.max(1));
        self.shots.drain(..over);
        Some(id)
    }

    /// Moves every projectile by `dt` seconds; wall hits go to `hits` in
    /// time order per projectile. A corner gives one hit per side and
    /// counts as one bounce. A projectile that is not a number is dropped.
    pub fn step(&mut self, dt: f32, life: Life, hits: &mut Vec<Hit>) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        self.shots.retain_mut(|p| p.pos.iter().chain(&p.vel).all(|v| v.is_finite()) && fly(p, dt, life, hits));
    }

    /// Moves the newest projectile on by `dt` seconds without hits counted
    /// twice: a shot from an older camera frame starts where it would be
    /// now. Its hits go to `hits` like a step's.
    pub fn advance_newest(&mut self, dt: f32, life: Life, hits: &mut Vec<Hit>) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let Some(p) = self.shots.last_mut() else { return };
        if !fly(p, dt, life, hits) {
            self.shots.pop();
        }
    }

    pub fn clear(&mut self) {
        self.shots.clear();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.shots.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }

    pub fn shots(&self) -> &[Projectile] {
        &self.shots
    }
}

/// Time to the wall `v` runs towards from `x` in 0..hi; None when still.
fn wall_time(x: f32, v: f32, hi: f32) -> Option<f32> {
    if v > 0.0 {
        Some(((hi - x) / v).max(0.0))
    } else if v < 0.0 {
        Some(((0.0 - x) / v).max(0.0))
    } else {
        None
    }
}

/// One projectile through `dt`; false when it is gone.
fn fly(p: &mut Projectile, dt: f32, life: Life, hits: &mut Vec<Hit>) -> bool {
    let mut left = dt;
    let speed = (p.vel[0] * p.vel[0] + p.vel[1] * p.vel[1]).sqrt();
    for _ in 0..MAX_HITS_PER_STEP {
        let tx = wall_time(p.pos[0], p.vel[0], ASPECT);
        let ty = wall_time(p.pos[1], p.vel[1], 1.0);
        let t = match (tx, ty) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => f32::INFINITY,
        };
        if t > left {
            p.pos = [p.pos[0] + p.vel[0] * left, p.pos[1] + p.vel[1] * left];
            break;
        }
        p.pos = [p.pos[0] + p.vel[0] * t, p.pos[1] + p.vel[1] * t];
        left -= t;
        let x_hit = tx.is_some_and(|a| a <= t) || (p.vel[0] > 0.0 && ASPECT - p.pos[0] <= CORNER) || (p.vel[0] < 0.0 && p.pos[0] <= CORNER);
        let y_hit = ty.is_some_and(|b| b <= t) || (p.vel[1] > 0.0 && 1.0 - p.pos[1] <= CORNER) || (p.vel[1] < 0.0 && p.pos[1] <= CORNER);
        if x_hit {
            let side = if p.vel[0] > 0.0 { Side::Right } else { Side::Left };
            p.pos[0] = if side == Side::Right { ASPECT } else { 0.0 };
            p.vel[0] = -p.vel[0];
            hits.push(Hit { side, pos: (1.0 - p.pos[1]).clamp(0.0, 1.0), speed, strength: p.strength, projectile: p.id });
        }
        if y_hit {
            let side = if p.vel[1] > 0.0 { Side::Bottom } else { Side::Top };
            p.pos[1] = if side == Side::Bottom { 1.0 } else { 0.0 };
            p.vel[1] = -p.vel[1];
            hits.push(Hit { side, pos: (p.pos[0] / ASPECT).clamp(0.0, 1.0), speed, strength: p.strength, projectile: p.id });
        }
        p.bounces += 1;
        if life.bounces.is_some_and(|b| p.bounces >= b) {
            return false;
        }
    }
    p.age += dt;
    life.seconds.is_none_or(|s| p.age < s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOREVER: Life = Life { bounces: None, seconds: None };

    fn sides(hits: &[Hit]) -> Vec<Side> {
        hits.iter().map(|h| h.side).collect()
    }

    #[test]
    fn a_wall_reflects_at_the_incoming_angle() {
        let mut a = Arena::default();
        // from the middle, 45 degrees up and right
        a.shoot([0.5, 0.5], [1.0, -1.0], 1.0, 0.5, 8);
        let mut hits = Vec::new();
        a.step(1.0, FOREVER, &mut hits);
        assert_eq!(sides(&hits), vec![Side::Top]);
        let p = a.shots()[0];
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((p.vel[0] - s).abs() < 1e-5 && (p.vel[1] - s).abs() < 1e-5, "vy flipped, vx kept: {:?}", p.vel);
        // the hit is where the line meets the top: 0.5 units right of the start
        assert!((hits[0].pos - (to_world([0.5, 0.5])[0] + 0.5) / ASPECT).abs() < 1e-4);
        assert!((hits[0].speed - 1.0).abs() < 1e-5);
    }

    #[test]
    fn no_tunnelling_at_any_speed() {
        let mut a = Arena::default();
        a.shoot([0.5, 0.5], [1.0, 0.0], 1000.0, 0.5, 8);
        let mut hits = Vec::new();
        // 1000 heights/s for 4 ms is 4 heights: two widths of 1.78, so two
        // walls at least, alternating
        a.step(0.004, FOREVER, &mut hits);
        assert!(hits.len() >= 2, "{hits:?}");
        assert!(hits.windows(2).all(|w| w[0].side != w[1].side));
        let x = a.shots()[0].pos[0];
        assert!((0.0..=ASPECT).contains(&x));
    }

    #[test]
    fn a_corner_is_two_sides_and_one_bounce() {
        let mut a = Arena::default();
        // aimed exactly at the top-right corner
        let at = [0.5, 0.5];
        let w = to_world(at);
        a.shoot(at, [ASPECT - w[0], -w[1]], 1.0, 0.5, 8);
        let mut hits = Vec::new();
        a.step(2.0, Life { bounces: Some(2), seconds: None }, &mut hits);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(sides(&hits).contains(&Side::Top) && sides(&hits).contains(&Side::Right));
        assert_eq!(a.shots()[0].bounces, 1);
        // and it flies back the way it came
        let v = a.shots()[0].vel;
        assert!(v[0] < 0.0 && v[1] > 0.0);
    }

    #[test]
    fn it_is_gone_after_its_last_bounce_or_its_time() {
        let mut a = Arena::default();
        a.shoot([0.5, 0.5], [1.0, 0.0], 2.0, 0.5, 8);
        let mut hits = Vec::new();
        let three = Life { bounces: Some(3), seconds: None };
        for _ in 0..1000 {
            a.step(0.01, three, &mut hits);
        }
        assert_eq!(hits.len(), 3, "the third hit fires, then it is gone");
        assert!(a.is_empty());
        a.shoot([0.5, 0.5], [0.0, 1.0], 0.1, 0.5, 8);
        let two_s = Life { bounces: None, seconds: Some(2.0) };
        for _ in 0..199 {
            a.step(0.01, two_s, &mut hits);
        }
        assert_eq!(a.len(), 1);
        a.step(0.02, two_s, &mut hits);
        assert!(a.is_empty());
    }

    #[test]
    fn never_keeps_it_bouncing() {
        let mut a = Arena::default();
        a.shoot([0.3, 0.6], [0.7, 0.4], 3.0, 0.5, 8);
        let mut hits = Vec::new();
        for _ in 0..60_000 {
            a.step(0.004, FOREVER, &mut hits);
        }
        assert_eq!(a.len(), 1);
        let p = a.shots()[0];
        assert!((0.0..=ASPECT).contains(&p.pos[0]) && (0.0..=1.0).contains(&p.pos[1]));
        let speed = (p.vel[0].powi(2) + p.vel[1].powi(2)).sqrt();
        assert!((speed - 3.0).abs() < 1e-3, "speed kept: {speed}");
        assert!(hits.len() > 100);
    }

    #[test]
    fn over_the_cap_the_oldest_goes() {
        let mut a = Arena::default();
        let ids: Vec<u64> = (0..5).filter_map(|_| a.shoot([0.5, 0.5], [0.0, -1.0], 1.0, 0.5, 3)).collect();
        assert_eq!(a.shots().iter().map(|p| p.id).collect::<Vec<_>>(), ids[2..].to_vec());
    }

    #[test]
    fn side_positions_and_strength() {
        let mut a = Arena::default();
        // straight left from low in the picture: the left wall near its bottom
        a.shoot([0.5, 0.9], [-1.0, 0.0], 1.0, 0.25, 8);
        let mut hits = Vec::new();
        a.step(2.0, FOREVER, &mut hits);
        assert_eq!(hits[0].side, Side::Left);
        assert!((hits[0].pos - 0.1).abs() < 1e-5, "{}", hits[0].pos);
        assert_eq!(hits[0].strength, 0.25);
    }

    #[test]
    fn a_late_shot_starts_where_it_would_be() {
        let mut a = Arena::default();
        a.shoot([0.5, 0.5], [0.0, -1.0], 1.0, 0.5, 8);
        let mut hits = Vec::new();
        a.advance_newest(0.1, FOREVER, &mut hits);
        assert!((a.shots()[0].pos[1] - 0.4).abs() < 1e-5);
        a.advance_newest(0.5, FOREVER, &mut hits);
        assert_eq!(sides(&hits), vec![Side::Top]);
    }

    #[test]
    fn bad_shots_are_refused_and_still_ones_fly_up() {
        let mut a = Arena::default();
        assert!(a.shoot([f32::NAN, 0.5], [1.0, 0.0], 1.0, 0.5, 8).is_none());
        assert!(a.shoot([0.5, 0.5], [1.0, 0.0], f32::INFINITY, 0.5, 8).is_none());
        a.shoot([2.0, -1.0], [0.0, 0.0], 1.0, 0.5, 8);
        let p = a.shots()[0];
        assert!(p.pos[0] <= ASPECT && p.pos[1] >= 0.0, "started inside: {:?}", p.pos);
        assert!(p.vel[1] < 0.0 && p.vel[0] == 0.0);
        let mut hits = Vec::new();
        a.step(f32::NAN, FOREVER, &mut hits);
        a.step(-1.0, FOREVER, &mut hits);
        assert!(hits.is_empty());
    }
}
