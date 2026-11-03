//! Signal -> target routing. Many-to-many; each target (one of the 32
//! Signal macros, or a parameter of a plugin in the FL project) sums its
//! lines and clamps to 0..1. A line is what one sentence made (Route::line);
//! its routes on one target average ("both hands" is one line of two
//! routes and stays inside its range), a route of line 0 counts alone.

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

/// Hand x signals and the side of the person each moves on while both
/// are routed on x: -1 left of the centre, 1 right of it. x is from the
/// person's view (geom.rs: 0 = their left).
const HAND_X: [(&str, f32); 2] = [("cam0.hand.L.x", -1.0), ("cam0.hand.R.x", 1.0)];
/// How far each hand's range reaches from the person's centre, in frame
/// widths: half the frame, so a person in the middle splits it at 0.5.
pub const HAND_X_REACH: f32 = 0.5;
/// Time constant of the centre's smoothing (s): head moves while playing
/// shift the split slowly instead of shaking both x routes.
const CENTRE_TAU_S: f64 = 1.0;

/// Input range of the routes on `signal` when it is a hand x: while both
/// hands' x have an enabled route (Noa, FL test 11-03), each hand gets its
/// own side of the person's centre, from the centre out to HAND_X_REACH,
/// cut at the frame edge, so one hand cannot cross the other; else the
/// whole frame. y has no such rule.
fn fit_hand_x(routes: &mut [Route], centre: f32, signal: &str) {
    let Some(&(_, dir)) = HAND_X.iter().find(|h| h.0 == signal) else { return };
    let both = HAND_X.iter().all(|h| routes.iter().any(|r| r.enabled && r.signal == h.0));
    let (lo, hi) = match (both, dir < 0.0) {
        (false, _) => (0.0, 1.0),
        (true, true) => ((centre - HAND_X_REACH).max(0.0), centre),
        (true, false) => (centre, (centre + HAND_X_REACH).min(1.0)),
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
    fit_hand_x(routes, current.centre_x(), signal);
    let seen = if sample.present && sample.confidence >= GATE_MIN_CONF { sample.value } else { 0.0 };
    match current.last.get_mut(signal) {
        Some(v) => *v = seen,
        None => {
            current.last.insert(signal.to_string(), seen);
        }
    }
    let mut touched: Vec<Target> = Vec::new();
    for r in routes.iter_mut() {
        if !r.enabled || r.signal != signal {
            continue;
        }
        if r.gates.iter().any(|g| current.last.get(g).is_none_or(|v| *v <= GATE_OPEN)) {
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
            Target::Param { .. } => current.params.entry(t.clone()).or_insert(f32::NAN),
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

    #[test]
    fn two_hands_on_x_each_get_their_half() {
        let (l, r) = ("cam0.hand.L.x", "cam0.hand.R.x");
        let mut routes = vec![Route::new(l, 0, 1.0), Route::new(r, 1, 1.0), Route::new("cam0.hand.L.y", 2, 1.0)];
        let mut cur = Values::default();
        let feed = |routes: &mut Vec<Route>, cur: &mut Values, t0: usize, vl: f32, vr: f32| {
            for i in t0..t0 + 90 {
                let t = i as f64 / 30.0;
                apply_sample(routes, cur, l, &s(vl), t);
                apply_sample(routes, cur, r, &s(vr), t);
                apply_sample(routes, cur, "cam0.hand.L.y", &s(0.25), t);
            }
        };
        // left hand at the centre and right hand at the frame edge: both at their top;
        // a hand a quarter in from its outer edge sits at the middle of its route
        feed(&mut routes, &mut cur, 0, 0.5, 1.0);
        assert!(cur.macros[0] > 0.98 && cur.macros[1] > 0.98, "{:?}", &cur.macros[..2]);
        feed(&mut routes, &mut cur, 90, 0.25, 0.75);
        assert!((cur.macros[0] - 0.5).abs() < 0.02 && (cur.macros[1] - 0.5).abs() < 0.02, "{:?}", &cur.macros[..2]);
        // a hand on the other side holds at its inner end; y keeps the whole frame
        feed(&mut routes, &mut cur, 180, 0.7, 0.3);
        assert!(cur.macros[0] > 0.98 && cur.macros[1] < 0.02, "{:?}", &cur.macros[..2]);
        assert!((cur.macros[2] - 0.25).abs() < 0.02, "y {}", cur.macros[2]);
        // the right hand's route goes off: the left hand has the whole frame again
        routes[1].enabled = false;
        feed(&mut routes, &mut cur, 270, 0.25, 0.75);
        assert!((cur.macros[0] - 0.25).abs() < 0.02, "{}", cur.macros[0]);
    }

    #[test]
    fn two_hands_on_x_split_at_the_persons_centre() {
        let (l, r) = ("cam0.hand.L.x", "cam0.hand.R.x");
        let mut routes = vec![Route::new(l, 0, 1.0), Route::new(r, 1, 1.0)];
        let mut cur = Values::default();
        // the person stands at 0.3: left hand 0..0.3, right hand 0.3..0.8
        for i in 0..300 {
            let t = i as f64 / 30.0;
            apply_sample(&mut routes, &mut cur, SIG_BODY_X, &s(0.3), t);
            apply_sample(&mut routes, &mut cur, l, &s(0.15), t);
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
    }

    #[test]
    fn one_hand_on_x_keeps_the_whole_frame() {
        let mut routes = vec![Route::new("cam0.hand.R.x", 0, 1.0), Route::new("cam0.hand.L.y", 1, 1.0)];
        let mut cur = Values::default();
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, "cam0.hand.R.x", &s(0.3), i as f64 / 30.0);
        }
        assert!((cur.macros[0] - 0.3).abs() < 0.02, "{}", cur.macros[0]);
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
