//! The Pong runtime behind Pong's lock: the arena, the shot detectors per
//! hand, the hit window and the ramps per side, the counters. Time and
//! input come in as arguments, so tests drive it with their own clock; the
//! only way out is `Out` (the player, the macros, the parameter sink).

use crate::fl;
use crate::midi::HitReq;
use crate::writer::Writer;

use super::arena::{Arena, Hit};
use super::setup::{Action, How, Mode, Setup};
use super::shot::{self, Point, Shot};
use super::Sink;

/// One step moves the projectiles at most this far (a stalled thread).
pub const MAX_STEP_NS: u64 = 50_000_000;
/// A shot from an older camera frame is moved on at most this much.
pub const MAX_LATE_NS: u64 = 200_000_000;
/// Per side the first hit in this window fires, later ones are dropped.
pub const HIT_WINDOW_NS: u64 = 30_000_000;
/// A glide back writes its target at most this often.
pub const GLIDE_EVERY_NS: u64 = 33_000_000;
/// Smaller changes are not written.
const WRITE_EPS: f32 = 1.0 / 1024.0;
/// Test shots fly from the frame centre, each this angle (rad) after the
/// last, so a burst covers every direction without a pattern.
const GOLDEN: f32 = 2.399_963;
pub const TEST_STRENGTH: f32 = 0.7;

/// What one tick reads: the engine's last frame per hand (L, R) with
/// capture times and Live.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub masks: [Option<(u8, u64)>; 2],
    pub skeleton: [Option<([[f32; 2]; 21], u64)>; 2],
    pub fingers: [Option<([f32; 5], u64)>; 2],
    pub speed: [Option<(f32, u64)>; 2],
    pub live: bool,
}

impl Input {
    /// The engine statics now, each locked alone and copied out.
    pub fn read(live: bool) -> Input {
        Input {
            masks: *crate::engine::MASKS.lock(),
            skeleton: *crate::engine::SKELETON.lock(),
            fingers: *crate::engine::FINGER_VALUES.lock(),
            speed: *crate::engine::HAND_SPEED.lock(),
            live,
        }
    }
}

/// Where a wall's hits go.
#[derive(Clone, Debug, PartialEq)]
pub enum Tgt {
    Param(fl::Key, u32),
    Macro(usize),
}

/// The parameter or macro a wall moves, and how.
fn target(a: &Action) -> Option<(Tgt, How)> {
    match a {
        Action::Param { key, index, how, .. } => Some((Tgt::Param(fl::Key::parse(key)?, *index), *how)),
        Action::Macro { index, how } => Some((Tgt::Macro(*index), *how)),
        Action::Off | Action::Part { .. } => None,
    }
}

/// The writer and the parameter sink, for the length of one call.
pub struct Out<'a> {
    pub writer: &'a Writer,
    pub sink: &'a dyn Sink,
}

impl Out<'_> {
    fn write(&self, t: &Tgt, v: f32) {
        match t {
            Tgt::Param(key, index) => self.sink.param(key, *index, v),
            Tgt::Macro(i) => self.writer.set_macro(*i, v),
        }
    }
}

/// One side between ticks.
#[derive(Debug, Default)]
struct SideRun {
    /// When its last hit fired.
    fired: Option<u64>,
    /// What its ramps, toggle and last write belong to.
    target: Option<(Tgt, How)>,
    /// Running ramps: peak and start.
    ramps: Vec<(f32, u64)>,
    /// Last value written and when.
    written: Option<(f32, u64)>,
    toggled: bool,
}

impl SideRun {
    fn hit(&mut self, tgt: Tgt, how: How, hit: &Hit, now: u64, out: &Out) {
        if self.target.as_ref().is_none_or(|(t, h)| *t != tgt || *h != how) {
            // the old target's ramp writes lo before it is dropped
            self.end_ramp(now, out);
            *self = SideRun { fired: self.fired, target: Some((tgt, how)), ..SideRun::default() };
        }
        let v = match how.mode {
            Mode::Ramp => {
                let depth = if how.depth_from_speed { hit.strength } else { 1.0 };
                self.ramps.push((how.lo + (how.hi - how.lo) * depth, now));
                self.ramp(now)
            }
            Mode::Position => how.lo + (how.hi - how.lo) * hit.pos,
            Mode::Toggle => {
                self.toggled = !self.toggled;
                if self.toggled {
                    how.hi
                } else {
                    how.lo
                }
            }
        };
        // a hit writes at once: the hit window keeps it to one per 30 ms
        self.write(v, now, out);
    }

    /// The ramps' value at `now`: each glides from its peak back to lo
    /// over back_ms, the one farthest from lo counts. Finished ones go.
    fn ramp(&mut self, now: u64) -> f32 {
        let Some((_, how)) = &self.target else { return 0.0 };
        let back = u64::from(how.back_ms.max(1)) * 1_000_000;
        self.ramps.retain(|r| now.saturating_sub(r.1) < back);
        let at = |&(peak, t0): &(f32, u64)| peak - (peak - how.lo) * (now.saturating_sub(t0) as f32 / back as f32);
        self.ramps.iter().map(at).fold(how.lo, |a, v| if (v - how.lo).abs() > (a - how.lo).abs() { v } else { a })
    }

    /// Writes `v` when it moved at least WRITE_EPS (the first always).
    fn write(&mut self, v: f32, now: u64, out: &Out) {
        if self.written.is_some_and(|(w, _)| (v - w).abs() < WRITE_EPS) {
            return;
        }
        let Some((t, _)) = &self.target else { return };
        out.write(t, v);
        self.written = Some((v, now));
    }

    /// The glide between hits, every GLIDE_EVERY_NS; its end writes lo.
    fn glide(&mut self, now: u64, out: &Out) {
        if self.ramps.is_empty() {
            return;
        }
        let v = self.ramp(now);
        if self.ramps.is_empty() {
            self.write_lo(now, out);
        } else if self.written.is_none_or(|(_, at)| now.saturating_sub(at) >= GLIDE_EVERY_NS) {
            self.write(v, now, out);
        }
    }

    /// A running ramp writes lo once and stops.
    fn end_ramp(&mut self, now: u64, out: &Out) {
        if !self.ramps.is_empty() {
            self.ramps.clear();
            self.write_lo(now, out);
        }
    }

    fn write_lo(&mut self, now: u64, out: &Out) {
        let Some((t, how)) = &self.target else { return };
        if self.written.is_none_or(|(w, _)| w != how.lo) {
            out.write(t, how.lo);
            self.written = Some((how.lo, now));
        }
    }
}

#[derive(Debug, Default)]
pub struct Run {
    pub arena: Arena,
    point: [Point; 2],
    /// Capture time of the last frame each hand's detector read.
    seen: [Option<u64>; 2],
    sides: [SideRun; 4],
    /// Time of the last step; None after a pause (the next step is 0 s).
    last_ns: Option<u64>,
    running: bool,
    /// Test shots so far (their angle).
    tests: u32,
    pub fired: u64,
    pub hits: u64,
    pub dropped_window: u64,
}

/// A shot's speed: the setup's, 0.6x..1.4x by strength when asked.
fn speed(setup: &Setup, strength: f32) -> f32 {
    setup.speed * if setup.speed_from_flick { 0.6 + 0.8 * strength } else { 1.0 }
}

impl Run {
    /// One tick at `now` (host clock): detectors, arena step, hits out,
    /// glides. While not `running` nothing moves; the tick it stops
    /// clears the arena and writes lo to running ramps.
    pub fn tick(&mut self, now: u64, input: &Input, setup: &Setup, running: bool, out: &Out) {
        if !running {
            if self.running {
                self.stop(now, out);
            }
            return;
        }
        if !self.running {
            self.running = true;
            self.last_ns = None;
        }
        let dt = self.last_ns.map_or(0, |l| now.saturating_sub(l).min(MAX_STEP_NS));
        self.last_ns = Some(now);
        let mut hits = Vec::new();
        self.arena.step(dt as f32 / 1e9, setup.life, &mut hits);
        for h in 0..2 {
            if let Some(s) = self.detect(h, now, input, setup) {
                self.spawn(&s, now, setup, &mut hits);
            }
        }
        self.dispatch(now, &hits, setup, out);
        for s in &mut self.sides {
            s.glide(now, out);
        }
    }

    /// The next step starts from 0 s (Pong on, a pause).
    pub fn restart(&mut self) {
        self.last_ns = None;
    }

    /// Projectiles gone, detectors start over, running ramps at lo.
    pub fn stop(&mut self, now: u64, out: &Out) {
        self.running = false;
        self.last_ns = None;
        self.arena.clear();
        self.lost(0);
        self.lost(1);
        self.end_ramps(now, out);
    }

    fn lost(&mut self, h: usize) {
        self.point[h].lost();
    }

    pub fn end_ramps(&mut self, now: u64, out: &Out) {
        for s in &mut self.sides {
            s.end_ramp(now, out);
        }
    }

    /// Side `i` got another action: its ramp writes lo, the rest starts over.
    pub fn end_side(&mut self, i: usize, now: u64, out: &Out) {
        if let Some(s) = self.sides.get_mut(i) {
            s.end_ramp(now, out);
            *s = SideRun { fired: s.fired, ..SideRun::default() };
        }
    }

    /// Hand `h`'s detector on its newest frame. A hand that does not
    /// shoot, has no frame or none for shot::LOST_NS is lost. A point
    /// waits for the mask of the skeleton's frame (perception writes them
    /// one after the other; a frame whose shape has not settled has none).
    fn detect(&mut self, h: usize, now: u64, input: &Input, setup: &Setup) -> Option<Shot> {
        let frame = input.skeleton[h].filter(|f| setup.hands.has(h) && now.saturating_sub(f.1) <= shot::LOST_NS);
        let Some((joints, t)) = frame else {
            self.lost(h);
            return None;
        };
        if self.seen[h].is_some_and(|s| t <= s) {
            return None;
        }
        let (mask, _) = input.masks[h].filter(|m| m.1 == t)?;
        self.seen[h] = Some(t);
        let hand = shot::Hand { vals: input.fingers[h].filter(|f| f.1 == t).map(|f| f.0), speed: input.speed[h].filter(|s| s.1 == t).map(|s| s.0) };
        self.point[h].update(mask, &joints, hand, t)
    }

    /// The shot flies from where it would be now: moved on by the time
    /// since its frame was captured (perception latency).
    fn spawn(&mut self, s: &Shot, now: u64, setup: &Setup, hits: &mut Vec<Hit>) {
        if self.arena.shoot(s.at, s.dir, speed(setup, s.strength), s.strength, setup.max as usize).is_none() {
            return;
        }
        self.fired += 1;
        let late = now.saturating_sub(s.t_ns).min(MAX_LATE_NS);
        self.arena.advance_newest(late as f32 / 1e9, setup.life, hits);
    }

    /// `n` shots from the frame centre at golden-angle directions.
    pub fn test_shots(&mut self, n: usize, setup: &Setup) {
        for _ in 0..n {
            let a = self.tests as f32 * GOLDEN;
            self.tests = self.tests.wrapping_add(1);
            if self.arena.shoot([0.5, 0.5], [a.cos(), a.sin()], speed(setup, TEST_STRENGTH), TEST_STRENGTH, setup.max as usize).is_some() {
                self.fired += 1;
            }
        }
    }

    /// The hits of one tick: per side the first in HIT_WINDOW_NS fires;
    /// the part walls' hits go to the player in one call.
    fn dispatch(&mut self, now: u64, hits: &[Hit], setup: &Setup, out: &Out) {
        let mut parts = Vec::new();
        for hit in hits {
            let action = setup.wall(hit.side);
            if *action == Action::Off {
                continue;
            }
            let side = &mut self.sides[hit.side.index()];
            if side.fired.is_some_and(|f| now.saturating_sub(f) < HIT_WINDOW_NS) {
                self.dropped_window += 1;
                continue;
            }
            side.fired = Some(now);
            self.hits += 1;
            if let Action::Part { id } = action {
                parts.push(HitReq { part: *id, strength: hit.strength, at_ns: now });
            } else if let Some((t, how)) = target(action) {
                side.hit(t, how, hit, now, out);
            }
        }
        if !parts.is_empty() {
            out.writer.player.hits(&parts, setup.vel_from_speed, f64::from(setup.hit_len));
        }
    }

    /// Seconds since each side's last hit fired.
    pub fn since_hit(&self, now: u64) -> [Option<f32>; 4] {
        std::array::from_fn(|i| self.sides[i].fired.map(|f| now.saturating_sub(f) as f32 / 1e9))
    }
}
