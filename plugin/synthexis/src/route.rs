//! Signal -> target routing. Many-to-many; each target (one of the 32
//! Signal macros, or a parameter of a plugin in the FL project) sums its
//! lines and clamps to 0..1. A line is what one sentence made (Route::line);
//! its routes on one target average ("both hands" is one line of two
//! routes and stays inside its range), a route of line 0 counts alone.
//! A route to a MIDI part (midi.rs) has no value: its trigger goes on and
//! off, after hysteresis and a minimum dwell (spec cond.hysteresis), and
//! each edge lands in `Values::events` for the player.

use std::collections::HashMap;

use crate::cond::Chain;
use crate::fl;
use crate::signal::{Sample, SIG_BODY_X, SIG_BRIGHTNESS, SIG_MOTION, SIG_TRIANGLE};

pub const NUM_MACROS: usize = 32;

/// Where a route writes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// Signal-macro index 0..31 (Signal N = index N-1), linked in FL by
    /// the user.
    Macro(usize),
    /// A parameter of a plugin in the project, written through the FL
    /// script (bridge.fl_script, REC events).
    Param { plugin: fl::Key, index: u32 },
    /// A synthexis MIDI part, started and stopped by `trigger`.
    Part { id: u64, trigger: Trigger },
}

/// When a route to a MIDI part is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The source reads above 0.55; off again below 0.45 (pinch, mouth).
    #[default]
    Above,
    /// The hand's finger mask (signal hand.shape) has the bits of `mask`
    /// on the fingers in `care`: one finger shown is care = mask = its bit,
    /// a named shape cares about all five.
    Shape { mask: u8, care: u8 },
}

/// hand.shape carries the 5-bit finger mask (thumb = 0b10000 .. pinky =
/// 0b00001, the ontology's "shapes" order) as mask / 31.
pub fn shape_value(mask: u8) -> f32 {
    f32::from(mask & 31) / 31.0
}

pub fn shape_mask(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 31.0).round() as u8
}

/// The bit of finger `f` (0 thumb .. 4 pinky) in a shape mask.
pub fn finger_bit(f: usize) -> u8 {
    1 << (4 - f.min(4))
}

/// A value trigger must hold its new state this long (spec
/// cond.hysteresis min_dwell); shapes are debounced by the engine.
const DWELL_S: f64 = 0.060;
/// A trigger whose source is lost stays on this long (cond.rs occlusion
/// hold), then goes off; also when its source sends nothing at all.
const LOST_S: f64 = 0.6;

/// Trigger state of one route.
#[derive(Clone, Copy, Debug, Default)]
pub struct Edge {
    pub on: bool,
    raw: bool,
    since: f64,
    lost: Option<f64>,
    /// Time of the last sample of the route's own signal.
    heard: f64,
}

impl Edge {
    /// The new state when it changed. `invert` (INV, the flip button):
    /// a value trigger is on below its threshold, a shape trigger while
    /// the shape is not shown.
    fn step(&mut self, trigger: Trigger, s: &Sample, t: f64, open: bool, invert: bool) -> Option<bool> {
        self.heard = t;
        if !s.present || s.confidence < GATE_MIN_CONF {
            let lost = *self.lost.get_or_insert(t);
            if self.on && t - lost >= LOST_S {
                (self.on, self.raw) = (false, false);
                return Some(false);
            }
            return None;
        }
        self.lost = None;
        let (raw, dwell) = match trigger {
            Trigger::Above => {
                let v = if invert { 1.0 - s.value } else { s.value };
                (v > if self.raw { 0.45 } else { 0.55 }, DWELL_S)
            }
            Trigger::Shape { mask, care } => ((shape_mask(s.value) & care == mask & care) != invert, 0.0),
        };
        let raw = raw && open;
        if raw != self.raw {
            (self.raw, self.since) = (raw, t);
        }
        (self.raw != self.on && t - self.since >= dwell).then(|| {
            self.on = self.raw;
            self.on
        })
    }

    /// Off when the route's own signal has sent nothing for LOST_S (the
    /// camera stopped, perception gives no percept): the occlusion hold
    /// above needs samples to end. `t` is another signal's sample time.
    fn quiet(&mut self, t: f64) -> Option<bool> {
        (self.on && t - self.heard >= LOST_S).then(|| {
            (self.on, self.raw) = (false, false);
            false
        })
    }
}

#[derive(Debug)]
pub struct Route {
    /// Set by the writer for assigned routes; 0 for the test routes.
    pub id: u64,
    /// The line (sentence field row) the route belongs to; 0: none.
    pub line: u64,
    pub signal: String,
    pub target: Target,
    /// -1..1; negative depths drive the target down.
    pub depth: f32,
    /// Output range: the conditioned 0..1 maps onto lo..hi ("nur bis 80
    /// Prozent" is hi 0.8).
    pub lo: f32,
    pub hi: f32,
    /// Signals that must all read above GATE_OPEN for the route to follow
    /// its source ("only while I pinch"); while one is closed the target
    /// holds. A gate signal that is absent or unsure counts as closed.
    pub gates: Vec<String>,
    pub cond: Chain,
    /// Whether this route is currently active.
    pub enabled: bool,
    /// Built-in test route: active only while the test_signal param is on.
    pub test: bool,
    /// Made by the assistant and not accepted yet (ui.assistant: pending
    /// rows run at once so the move can be checked by ear).
    pub pending: bool,
    /// Last conditioned output; a macro's value is the clamped sum of all
    /// enabled routes' last_v*depth (many-to-many fan-in).
    pub last_v: f32,
    /// Trigger state of a route to a MIDI part.
    pub edge: Edge,
}

impl Route {
    pub fn new(signal: &str, macro_index: usize, depth: f32) -> Self {
        Self::to(signal, Target::Macro(macro_index), depth)
    }

    pub fn to(signal: &str, target: Target, depth: f32) -> Self {
        Self {
            id: 0,
            line: 0,
            signal: signal.to_string(),
            target,
            depth,
            lo: 0.0,
            hi: 1.0,
            gates: Vec::new(),
            cond: Chain::default(),
            enabled: true,
            test: false,
            pending: false,
            last_v: 0.0,
            edge: Edge::default(),
        }
    }

    fn out(&self) -> f32 {
        (self.lo + (self.hi - self.lo) * self.last_v) * self.depth
    }
}

/// Current target values: the macros, and the plugin parameters routes
/// have written.
#[derive(Debug, Default)]
pub struct Values {
    pub macros: [f32; NUM_MACROS],
    pub params: HashMap<Target, f32>,
    /// Last value of every signal for the gates; 0 while the signal is
    /// absent or below GATE_MIN_CONF.
    pub last: HashMap<String, f32>,
    /// The person's centre on x (SIG_BODY_X, smoothed) and when it was
    /// last seen; None until a face was seen, then the frame's middle is used.
    pub centre: Option<(f32, f64)>,
    /// Hands [L, R] the routes of the other plugin instances use (writer.rs
    /// fills it before a hand x sample); `fit_hand_x` adds this table's own.
    pub hands_used: [bool; 2],
    /// Trigger edges of routes to MIDI parts since the writer last took
    /// them: (route id, part id, on).
    pub events: Vec<(u64, u64, bool)>,
    /// Hands [L, R] that shoot in Pong mode with their fingers paused: the
    /// gate of every route to a MIDI part on that hand is closed.
    pub paused: [bool; 2],
}

impl Values {
    fn centre_x(&self) -> f32 {
        self.centre.map_or(0.5, |c| c.0)
    }

    fn see_centre(&mut self, sample: &Sample, t_s: f64) {
        if !sample.present || sample.confidence < GATE_MIN_CONF {
            return;
        }
        let x = sample.value.clamp(0.0, 1.0);
        self.centre = Some(match self.centre {
            Some((c, t)) => (c + (x - c) * (1.0 - (-(t_s - t).max(0.0) / CENTRE_TAU_S).exp()) as f32, t_s),
            None => (x, t_s),
        });
    }
}

/// A gate signal above this lets its route move.
pub const GATE_OPEN: f32 = 0.5;
/// Below this confidence a gate signal counts as absent (cond.rs
/// Occlusion's c_min).
pub const GATE_MIN_CONF: f32 = 0.2;

/// Perception test routes on Signal 3..10 (probes/plugin-perception
/// fl_live.py): (signal id, macro index).
pub const PERCEPTION_DEFAULTS: &[(&str, usize)] = &[
    ("cam0.hand.R.openness", 2),
    ("cam0.hand.L.openness", 3),
    ("cam0.mouth.open", 4),
    ("cam0.hand.R.y", 5),
    ("cam0.hand.R.x", 6),
    ("cam0.hand.R.pinch", 7),
    ("cam0.eye.L.open", 8),
    ("cam0.head.yaw", 9),
];

/// Default routing table: only built-in test routes, all gated by the
/// test_signal param. With it off no macro moves, so with Live on FL
/// records only what a profile, a click or the assistant assigns (Noa's
/// rule; docs/ux-plugin.md compiles those into this table).
pub fn default_routes(test_signal_on: bool) -> Vec<Route> {
    let mut routes = vec![
        Route::new(SIG_BRIGHTNESS, 0, 1.0),
        Route::new(SIG_MOTION, 1, 1.0),
        Route::new(SIG_TRIANGLE, 31, 1.0),
    ];
    routes.extend(PERCEPTION_DEFAULTS.iter().map(|&(id, m)| Route::new(id, m, 1.0)));
    for r in &mut routes {
        r.test = true;
        r.enabled = test_signal_on;
    }
    routes
}

/// Hand x signals, [L, R]. x is from the person's view (geom.rs: 0 =
/// their left).
const HAND_X: [&str; 2] = ["cam0.hand.L.x", "cam0.hand.R.x"];
/// How far each hand's range reaches from the person's centre, in frame
/// widths: half the frame, so a person in the middle splits it at 0.5
/// (stretched arms reach 0.4 to 0.55 from the face centre in H_stretch, S2
/// to S4).
pub const HAND_X_REACH: f32 = 0.5;
/// Dead band on each side of the centre, in frame widths: a hand held right
/// next to the other one at the centre reads its inner end (palm centres
/// of hands close together sit 0.03 to 0.11 apart in G_free_hands, S2 to
/// S4, and the face is not always above the point where they meet).
pub const HAND_X_GAP: f32 = 0.08;
/// Time constant of the centre's smoothing (s): head moves while playing
/// shift the split slowly instead of shaking both x routes.
const CENTRE_TAU_S: f64 = 1.0;

/// The hand (0 = L, 1 = R) a signal belongs to.
pub fn hand_of(signal: &str) -> Option<usize> {
    if signal.starts_with("cam0.hand.L.") {
        Some(0)
    } else if signal.starts_with("cam0.hand.R.") {
        Some(1)
    } else {
        None
    }
}

/// Hands [L, R] the enabled assigned routes use, as source or gate. Test
/// routes do not count.
pub fn hands_used(routes: &[Route]) -> [bool; 2] {
    let mut used = [false; 2];
    for r in routes.iter().filter(|r| r.enabled && !r.test) {
        for s in std::iter::once(&r.signal).chain(&r.gates) {
            used[0] |= s.starts_with("cam0.hand.L.");
            used[1] |= s.starts_with("cam0.hand.R.");
        }
    }
    used
}

/// Input range of the routes on `signal` when it is a hand x. While the
/// other hand also has a route (here or in another instance), the hands
/// cannot cross (Noa, FL tests 11-03), so each hand's x runs on its own
/// side of the person's centre: from the frame edge side (HAND_X_REACH out,
/// cut at the edge) to HAND_X_GAP short of the centre, the left hand from
/// 0 far left to 1 at the centre, the right hand from 0 at the centre to 1
/// far right. One hand alone keeps the whole frame. y has no such rule.
fn fit_hand_x(routes: &mut [Route], current: &Values, signal: &str) {
    let Some(hand) = HAND_X.iter().position(|h| *h == signal) else { return };
    let other = 1 - hand;
    let split = hands_used(routes)[other] || current.hands_used[other];
    let c = current.centre_x();
    let (lo, hi) = match (split, hand) {
        (false, _) => (0.0, 1.0),
        // a face at the frame edge still leaves the hand a range to move in
        (true, 0) => ((c - HAND_X_REACH).max(0.0), (c - HAND_X_GAP).max(HAND_X_GAP)),
        (true, _) => ((c + HAND_X_GAP).min(1.0 - HAND_X_GAP), (c + HAND_X_REACH).min(1.0)),
    };
    for r in routes.iter_mut().filter(|r| r.signal == signal) {
        r.cond.range.in_min = lo;
        r.cond.range.in_max = hi;
    }
}

/// Apply one incoming sample to the route table; returns the changed
/// targets with their new values (after summing/clamping).
pub fn apply_sample(
    routes: &mut [Route],
    current: &mut Values,
    signal: &str,
    sample: &Sample,
    t_s: f64,
) -> Vec<(Target, f32)> {
    if signal == SIG_BODY_X {
        current.see_centre(sample, t_s);
        return Vec::new();
    }
    fit_hand_x(routes, current, signal);
    let seen = if sample.present && sample.confidence >= GATE_MIN_CONF { sample.value } else { 0.0 };
    match current.last.get_mut(signal) {
        Some(v) => *v = seen,
        None => {
            current.last.insert(signal.to_string(), seen);
        }
    }
    let mut touched: Vec<Target> = Vec::new();
    for r in routes.iter_mut() {
        if !r.enabled {
            continue;
        }
        if r.signal != signal {
            // the test triangle keeps coming at 30 Hz when the camera stops
            if let (Target::Part { id, .. }, Some(on)) = (&r.target, r.edge.quiet(t_s)) {
                current.events.push((r.id, *id, on));
            }
            continue;
        }
        let open = !r.gates.iter().any(|g| current.last.get(g).is_none_or(|v| *v <= GATE_OPEN));
        if let Target::Part { id, trigger } = r.target {
            // a closed gate ends the note instead of holding a value; a
            // paused hand (Pong) plays no part
            let open = open && !hand_of(signal).is_some_and(|h| current.paused[h]);
            if let Some(on) = r.edge.step(trigger, sample, t_s, open, r.cond.range.invert) {
                current.events.push((r.id, id, on));
            }
            continue;
        }
        if !open {
            continue;
        }
        r.last_v = r.cond.step(sample, t_s);
        if !touched.contains(&r.target) {
            touched.push(r.target.clone());
        }
    }
    // Recompute touched targets: the clamped sum of their lines, each line
    // the mean of its enabled routes on the target.
    let mut changed = Vec::new();
    for t in touched {
        let mut lines: Vec<(u64, f32, u32)> = Vec::new();
        for r in routes.iter().filter(|r| r.enabled && r.target == t) {
            match lines.iter_mut().find(|l| l.0 == r.line && r.line != 0) {
                Some(l) => {
                    l.1 += r.out();
                    l.2 += 1;
                }
                None => lines.push((r.line, r.out(), 1)),
            }
        }
        let new: f32 = lines.iter().map(|l| l.1 / l.2 as f32).sum::<f32>().clamp(0.0, 1.0);
        let old = match &t {
            Target::Macro(i) => &mut current.macros[*i],
            Target::Param { .. } | Target::Part { .. } => current.params.entry(t.clone()).or_insert(f32::NAN),
        };
        if old.is_nan() || (new - *old).abs() > f32::EPSILON {
            *old = new;
            changed.push((t, new));
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: f32) -> Sample {
        Sample {
            value: v,
            confidence: 1.0,
            t_capture_ns: 0,
            present: true,
        }
    }

    #[test]
    fn no_default_route_moves_a_macro_while_test_signal_is_off() {
        let mut routes = default_routes(false);
        let mut cur = Values::default();
        let ids = [SIG_BRIGHTNESS, SIG_MOTION, SIG_TRIANGLE].into_iter().chain(PERCEPTION_DEFAULTS.iter().map(|d| d.0));
        for id in ids {
            for i in 0..60 {
                assert!(apply_sample(&mut routes, &mut cur, id, &s(0.8), i as f64 / 30.0).is_empty(), "{id}");
            }
        }
        assert!(cur.macros.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn brightness_routes_to_signal_1() {
        let mut routes = default_routes(true);
        let mut cur = Values::default();
        // Feed constant brightness until the one-euro settles.
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, SIG_BRIGHTNESS, &s(0.8), i as f64 / 30.0);
        }
        assert!((cur.macros[0] - 0.8).abs() < 0.05, "sig1 {}", cur.macros[0]);
        assert!((cur.macros[1] - 0.0).abs() < 0.05);
    }

    #[test]
    fn perception_defaults_are_engine_ids() {
        let ids: Vec<&str> = crate::signal::perception_ids().collect();
        for (id, _) in PERCEPTION_DEFAULTS {
            assert!(ids.contains(id), "{id}");
        }
    }

    #[test]
    fn triangle_disabled_when_test_signal_off() {
        let mut routes = default_routes(false);
        let mut cur = Values::default();
        for i in 0..60 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur.macros[31], 0.0);
        let mut routes = default_routes(true);
        let mut cur = Values::default();
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(1.0), i as f64 / 30.0);
        }
        assert!(cur.macros[31] > 0.9);
    }

    #[test]
    fn many_to_many_sum_clamped() {
        let mut routes = vec![
            Route::new("x", 0, 0.7),
            Route::new("x", 0, 0.7),
        ];
        let mut cur = Values::default();
        for i in 0..120 {
            apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur.macros[0], 1.0);
    }

    #[test]
    fn param_route_maps_onto_its_range_and_waits_for_its_gate() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let target = Target::Param { plugin: key, index: 12 };
        let mut r = Route::to("x", target.clone(), 1.0);
        r.hi = 0.8;
        r.gates = vec!["g".into()];
        let mut routes = vec![r];
        let mut cur = Values::default();
        // gate closed: nothing moves
        for i in 0..30 {
            assert!(apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0).is_empty());
        }
        apply_sample(&mut routes, &mut cur, "g", &s(1.0), 1.0);
        let mut last = Vec::new();
        for i in 30..120 {
            last = apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        let v = cur.params[&target];
        assert!((v - 0.8).abs() < 0.02, "{v} {last:?}");
        // the gate hand leaves the frame: the gate closes, the target holds
        let gone = Sample { present: false, ..s(1.0) };
        apply_sample(&mut routes, &mut cur, "g", &gone, 4.0);
        for i in 120..150 {
            assert!(apply_sample(&mut routes, &mut cur, "x", &s(0.0), i as f64 / 30.0).is_empty());
        }
        assert!((cur.params[&target] - v).abs() < 1e-6);
    }

    #[test]
    fn a_lines_routes_average_and_lines_sum() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let t = Target::Param { plugin: key, index: 1 };
        let mk = |sig: &str, line| {
            let mut r = Route::to(sig, t.clone(), 1.0);
            r.line = line;
            r.hi = 0.8;
            r
        };
        // both hands, one line: at 1.0 and 0.0 the line gives 0.4, never above 0.8
        let mut routes = vec![mk("l", 7), mk("r", 7)];
        let mut cur = Values::default();
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, "l", &s(1.0), i as f64 / 30.0);
            apply_sample(&mut routes, &mut cur, "r", &s(0.0), i as f64 / 30.0);
        }
        assert!((cur.params[&t] - 0.4).abs() < 0.02, "{}", cur.params[&t]);
        // one hand left of the line: it reaches the line's hi alone
        routes.pop();
        for i in 90..180 {
            apply_sample(&mut routes, &mut cur, "l", &s(1.0), i as f64 / 30.0);
        }
        assert!((cur.params[&t] - 0.8).abs() < 0.02, "{}", cur.params[&t]);
    }

    /// Two instances of one plugin with the same parameter (Noa's two
    /// Serum 2 on tracks 3 and 10, both on Filter 1 Freq) are two targets:
    /// each route drives only its own instance and never sums into the other.
    #[test]
    fn two_instances_of_one_plugin_are_two_targets() {
        let serum = |ch| fl::Key { at: fl::At::Gen { ch }, plugin: "Serum 2".into() };
        let (a, b) = (Target::Param { plugin: serum(3), index: 30 }, Target::Param { plugin: serum(10), index: 30 });
        assert_ne!(a, b);
        let mk = |sig: &str, t: &Target, line| {
            let mut r = Route::to(sig, t.clone(), 1.0);
            r.line = line;
            r
        };
        let mut routes = vec![mk("thumb", &a, 1), mk("x", &b, 2)];
        let mut cur = Values::default();
        let mut moved = Vec::new();
        for i in 0..90 {
            moved.extend(apply_sample(&mut routes, &mut cur, "thumb", &s(1.0), i as f64 / 30.0).into_iter().map(|(t, _)| t));
            moved.extend(apply_sample(&mut routes, &mut cur, "x", &s(0.0), i as f64 / 30.0).into_iter().map(|(t, _)| t));
        }
        assert!((cur.params[&a] - 1.0).abs() < 0.02, "{:?}", cur.params);
        assert!(cur.params[&b].abs() < 0.02, "{:?}", cur.params);
        assert!(moved.iter().all(|t| *t == a || *t == b) && moved.contains(&a) && moved.contains(&b));
    }

    /// Feeds `vals` [(signal, value)] for 3 s from `t0` (s).
    fn feed(routes: &mut [Route], cur: &mut Values, t0: f64, vals: &[(&str, f32)]) {
        for i in 0..90 {
            for (sig, v) in vals {
                apply_sample(routes, cur, sig, &s(*v), t0 + i as f64 / 30.0);
            }
        }
    }

    #[test]
    fn two_hands_on_x_each_get_their_side_of_the_centre() {
        let (l, r) = ("cam0.hand.L.x", "cam0.hand.R.x");
        let mut routes = vec![Route::new(l, 0, 1.0), Route::new(r, 1, 1.0), Route::new("cam0.hand.L.y", 2, 1.0)];
        let mut cur = Values::default();
        // no face yet: the centre is 0.5, left 0..0.42, right 0.58..1
        feed(&mut routes, &mut cur, 0.0, &[(l, 0.42), (r, 1.0), ("cam0.hand.L.y", 0.25)]);
        assert!(cur.macros[0] > 0.98 && cur.macros[1] > 0.98, "{:?}", &cur.macros[..2]);
        feed(&mut routes, &mut cur, 3.0, &[(l, 0.21), (r, 0.79)]);
        assert!((cur.macros[0] - 0.5).abs() < 0.02 && (cur.macros[1] - 0.5).abs() < 0.02, "{:?}", &cur.macros[..2]);
        // the right hand right next to the left one at the centre reads 0;
        // the left hand there reads 1; y keeps the whole frame
        feed(&mut routes, &mut cur, 6.0, &[(l, 0.45), (r, 0.55)]);
        assert!(cur.macros[0] > 0.98 && cur.macros[1] < 0.02, "{:?}", &cur.macros[..2]);
        assert!((cur.macros[2] - 0.25).abs() < 0.02, "y {}", cur.macros[2]);
    }

    #[test]
    fn any_route_on_the_other_hand_splits_x() {
        // Noa's FL test: left hand openness and right hand x
        let r = "cam0.hand.R.x";
        let mut routes = vec![Route::new("cam0.hand.L.openness", 0, 1.0), Route::new(r, 1, 1.0)];
        let mut cur = Values::default();
        feed(&mut routes, &mut cur, 0.0, &[(r, 0.56)]);
        assert!(cur.macros[1] < 0.02, "{}", cur.macros[1]);
        feed(&mut routes, &mut cur, 3.0, &[(r, 1.0)]);
        assert!(cur.macros[1] > 0.98, "{}", cur.macros[1]);
        // the left hand's route goes off: the right hand has the whole frame again
        routes[0].enabled = false;
        feed(&mut routes, &mut cur, 6.0, &[(r, 0.3)]);
        assert!((cur.macros[1] - 0.3).abs() < 0.02, "{}", cur.macros[1]);
        // a left-hand gate counts as using the left hand
        let mut gated = Route::new(r, 1, 1.0);
        gated.gates = vec!["cam0.hand.L.pinch".into()];
        let mut routes = vec![gated];
        let mut cur = Values::default();
        feed(&mut routes, &mut cur, 0.0, &[("cam0.hand.L.pinch", 1.0), (r, 0.79)]);
        assert!((cur.macros[1] - 0.5).abs() < 0.02, "{}", cur.macros[1]);
    }

    #[test]
    fn another_instances_hand_splits_x_and_test_routes_do_not() {
        let r = "cam0.hand.R.x";
        let mut routes = vec![Route::new(r, 0, 1.0)];
        let mut cur = Values { hands_used: [true, false], ..Values::default() };
        feed(&mut routes, &mut cur, 0.0, &[(r, 0.79)]);
        assert!((cur.macros[0] - 0.5).abs() < 0.02, "{}", cur.macros[0]);
        // Test signal on: its left openness route leaves its right x whole
        let mut routes = default_routes(true);
        let mut cur = Values::default();
        feed(&mut routes, &mut cur, 0.0, &[(r, 0.3)]);
        assert!((cur.macros[6] - 0.3).abs() < 0.02, "{}", cur.macros[6]);
    }

    #[test]
    fn one_hand_alone_keeps_the_whole_frame() {
        let r = "cam0.hand.R.x";
        let mut routes = vec![Route::new(r, 0, 1.0), Route::new("cam0.hand.R.y", 1, 1.0)];
        let mut cur = Values::default();
        feed(&mut routes, &mut cur, 0.0, &[(r, 0.3)]);
        assert!((cur.macros[0] - 0.3).abs() < 0.02, "{}", cur.macros[0]);
    }

    #[test]
    fn two_hands_on_x_split_at_the_persons_centre() {
        let (l, r) = ("cam0.hand.L.x", "cam0.hand.R.x");
        let mut routes = vec![Route::new(l, 0, 1.0), Route::new(r, 1, 1.0)];
        let mut cur = Values::default();
        // the person stands at 0.3: left hand 0..0.22, right hand 0.38..0.8
        for i in 0..300 {
            let t = i as f64 / 30.0;
            apply_sample(&mut routes, &mut cur, SIG_BODY_X, &s(0.3), t);
            apply_sample(&mut routes, &mut cur, l, &s(0.11), t);
            apply_sample(&mut routes, &mut cur, r, &s(0.8), t);
        }
        assert!((cur.centre_x() - 0.3).abs() < 1e-3, "{}", cur.centre_x());
        assert!((cur.macros[0] - 0.5).abs() < 0.02 && cur.macros[1] > 0.98, "{:?}", &cur.macros[..2]);
        // a face seen once does not move the split at once
        let mut cur2 = Values { centre: Some((0.5, 0.0)), ..Values::default() };
        apply_sample(&mut routes, &mut cur2, SIG_BODY_X, &s(0.1), 1.0 / 30.0);
        assert!(cur2.centre_x() > 0.48, "{}", cur2.centre_x());
        // an unsure face is ignored
        let unsure = Sample { confidence: 0.1, ..s(0.9) };
        apply_sample(&mut routes, &mut cur, SIG_BODY_X, &unsure, 11.0);
        assert!((cur.centre_x() - 0.3).abs() < 1e-3);
        // a face at the frame edge still leaves the left hand a range
        let mut cur3 = Values { centre: Some((0.02, 0.0)), ..Values::default() };
        fit_hand_x(&mut routes, &cur3, l);
        let rg = routes[0].cond.range;
        assert!(rg.in_max - rg.in_min > 0.05, "{rg:?}");
        cur3.centre = Some((0.98, 0.0));
        fit_hand_x(&mut routes, &cur3, r);
        let rg = routes[1].cond.range;
        assert!(rg.in_max - rg.in_min > 0.05, "{rg:?}");
    }

    #[test]
    fn a_finger_shown_starts_and_ends_its_part_once() {
        let index = finger_bit(1);
        let mut r = Route::to("cam0.hand.R.shape", Target::Part { id: 9, trigger: Trigger::Shape { mask: index, care: index } }, 1.0);
        r.id = 4;
        let mut routes = vec![r];
        let mut cur = Values::default();
        let shape = |m: u8| s(shape_value(m));
        // index + middle shown: the index part is on, once
        for i in 0..5 {
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &shape(0b01100), i as f64 / 30.0);
        }
        assert_eq!(cur.events, vec![(4, 9, true)]);
        // the index folds: off; a lost hand holds 600 ms, then nothing more
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &shape(0b00100), 0.2);
        assert_eq!(cur.events, vec![(4, 9, true), (4, 9, false)]);
        cur.events.clear();
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &shape(0b01000), 0.3);
        let gone = Sample { present: false, ..shape(0b01000) };
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &gone, 0.4);
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &gone, 0.8);
        assert_eq!(cur.events, vec![(4, 9, true)], "held through a short loss");
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &gone, 1.01);
        assert_eq!(cur.events, vec![(4, 9, true), (4, 9, false)]);
        assert!(cur.macros.iter().all(|v| *v == 0.0) && cur.params.is_empty(), "a part target has no value");
    }

    #[test]
    fn a_value_trigger_does_not_chatter_around_its_threshold() {
        let mut r = Route::to("cam0.hand.R.pinch", Target::Part { id: 1, trigger: Trigger::Above }, 1.0);
        r.id = 2;
        let mut routes = vec![r];
        let mut cur = Values::default();
        // 0.5 +- 0.04 noise never crosses 0.55 / 0.45
        for i in 0..60 {
            let v = if i % 2 == 0 { 0.54 } else { 0.46 };
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.pinch", &s(v), i as f64 / 30.0);
        }
        assert!(cur.events.is_empty());
        // a pinch held for one frame only is not a note; held 60 ms it is
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.pinch", &s(0.9), 2.0);
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.pinch", &s(0.3), 2.033);
        assert!(cur.events.is_empty());
        for i in 0..4 {
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.pinch", &s(0.9), 3.0 + i as f64 / 30.0);
        }
        assert_eq!(cur.events, vec![(2, 1, true)]);
    }

    #[test]
    fn an_inverted_trigger_plays_on_the_other_side() {
        // "closing my mouth plays the snare": S:mouth.open INV
        let mut r = Route::to("cam0.mouth.open", Target::Part { id: 1, trigger: Trigger::Above }, 1.0);
        (r.id, r.cond.range.invert) = (2, true);
        let mut routes = vec![r];
        let mut cur = Values::default();
        let mut feed = |v: f32, t0: f64, cur: &mut Values| {
            for i in 0..4 {
                apply_sample(&mut routes, cur, "cam0.mouth.open", &s(v), t0 + i as f64 / 30.0);
            }
        };
        feed(0.9, 0.0, &mut cur);
        assert!(cur.events.is_empty(), "open mouth: nothing");
        feed(0.1, 1.0, &mut cur);
        assert_eq!(cur.events, vec![(2, 1, true)]);
        // the hysteresis turns round with it: 0.5 holds, 0.6 ends it
        feed(0.5, 2.0, &mut cur);
        assert_eq!(cur.events, vec![(2, 1, true)]);
        feed(0.6, 3.0, &mut cur);
        assert_eq!(cur.events, vec![(2, 1, true), (2, 1, false)]);
        // a flipped finger line plays while the finger is not shown
        let index = finger_bit(1);
        let mut r = Route::to("cam0.hand.R.shape", Target::Part { id: 9, trigger: Trigger::Shape { mask: index, care: index } }, 1.0);
        (r.id, r.cond.range.invert) = (4, true);
        let mut routes = vec![r];
        let mut cur = Values::default();
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(index)), 0.0);
        assert!(cur.events.is_empty());
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(0)), 0.1);
        assert_eq!(cur.events, vec![(4, 9, true)]);
    }

    #[test]
    fn a_trigger_ends_when_its_source_sends_nothing_more() {
        let index = finger_bit(1);
        let mut r = Route::to("cam0.hand.R.shape", Target::Part { id: 9, trigger: Trigger::Shape { mask: index, care: index } }, 1.0);
        r.id = 4;
        let mut routes = vec![r, Route::new(SIG_TRIANGLE, 31, 1.0)];
        let mut cur = Values::default();
        for i in 0..3 {
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(index)), i as f64 / 30.0);
        }
        assert_eq!(cur.events, vec![(4, 9, true)]);
        // the camera stops: no hand sample comes, present or not; the
        // triangle still does
        for i in 0..30 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(0.5), 0.1 + i as f64 / 60.0);
        }
        assert_eq!(cur.events, vec![(4, 9, true)], "held for {LOST_S} s");
        for i in 0..10 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(0.5), 0.7 + i as f64 / 30.0);
        }
        assert_eq!(cur.events, vec![(4, 9, true), (4, 9, false)]);
        // frames come back with the finger still up: it plays again
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(index)), 2.0);
        assert_eq!(cur.events, vec![(4, 9, true), (4, 9, false), (4, 9, true)]);
    }

    #[test]
    fn a_paused_hand_plays_no_part_and_keeps_its_parameter_routes() {
        let index = finger_bit(1);
        let mut part = Route::to("cam0.hand.R.shape", Target::Part { id: 9, trigger: Trigger::Shape { mask: index, care: index } }, 1.0);
        part.id = 4;
        let mut left = Route::to("cam0.hand.L.shape", Target::Part { id: 8, trigger: Trigger::Shape { mask: index, care: index } }, 1.0);
        left.id = 5;
        let mut routes = vec![part, left, Route::new("cam0.hand.R.pinch", 0, 1.0)];
        let mut cur = Values { paused: [false, true], ..Values::default() };
        for i in 0..30 {
            let t = i as f64 / 30.0;
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(index)), t);
            apply_sample(&mut routes, &mut cur, "cam0.hand.L.shape", &s(shape_value(index)), t);
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.pinch", &s(0.8), t);
        }
        assert_eq!(cur.events, vec![(5, 8, true)], "only the left hand plays");
        assert!(cur.macros[0] > 0.7, "the right pinch still moves its macro: {}", cur.macros[0]);
        cur.paused = [false, false];
        apply_sample(&mut routes, &mut cur, "cam0.hand.R.shape", &s(shape_value(index)), 1.1);
        assert_eq!(cur.events, vec![(5, 8, true), (4, 9, true)]);
    }

    #[test]
    fn negative_depth_drives_down() {
        let mut routes = vec![Route::new("x", 0, -1.0)];
        let mut cur = Values { macros: [0.5; NUM_MACROS], ..Values::default() };
        for i in 0..120 {
            apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur.macros[0], 0.0);
    }
}
