//! Signal -> target routing. Many-to-many; each target (one of the 32
//! Signal macros, or a parameter of a plugin in the FL project) sums its
//! routes' conditioned values * depth and clamps to 0..1.

use std::collections::HashMap;

use crate::cond::Chain;
use crate::fl;
use crate::signal::{Sample, SIG_BRIGHTNESS, SIG_MOTION, SIG_TRIANGLE};

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

/// Apply one incoming sample to the route table; returns the changed
/// targets with their new values (after summing/clamping).
pub fn apply_sample(
    routes: &mut [Route],
    current: &mut Values,
    signal: &str,
    sample: &Sample,
    t_s: f64,
) -> Vec<(Target, f32)> {
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
    // Recompute touched targets as the clamped sum of all enabled routes.
    let mut changed = Vec::new();
    for t in touched {
        let new: f32 = routes
            .iter()
            .filter(|r| r.enabled && r.target == t)
            .map(Route::out)
            .sum::<f32>()
            .clamp(0.0, 1.0);
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
    fn negative_depth_drives_down() {
        let mut routes = vec![Route::new("x", 0, -1.0)];
        let mut cur = Values { macros: [0.5; NUM_MACROS], ..Values::default() };
        for i in 0..120 {
            apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur.macros[0], 0.0);
    }
}
