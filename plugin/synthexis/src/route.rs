//! Signal -> macro routing. Many-to-many; each macro sums its routes'
//! conditioned values * depth and clamps to 0..1.

use crate::cond::Chain;
use crate::signal::{Sample, SIG_BRIGHTNESS, SIG_MOTION, SIG_TRIANGLE};

pub const NUM_MACROS: usize = 32;

#[derive(Debug)]
pub struct Route {
    pub signal: &'static str,
    /// Signal-macro index 0..31 (Signal N = index N-1).
    pub macro_index: usize,
    /// -1..1; negative depths drive the macro down.
    pub depth: f32,
    pub cond: Chain,
    /// Whether this route is currently active.
    pub enabled: bool,
    /// Built-in test route: active only while the test_signal param is on.
    pub test: bool,
    /// Last conditioned output; a macro's value is the clamped sum of all
    /// enabled routes' last_v*depth (many-to-many fan-in).
    pub last_v: f32,
}

impl Route {
    pub fn new(signal: &'static str, macro_index: usize, depth: f32) -> Self {
        Self {
            signal,
            macro_index,
            depth,
            cond: Chain::default(),
            enabled: true,
            test: false,
            last_v: 0.0,
        }
    }
}

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

/// Apply one incoming sample to the route table; returns the changed macro
/// values (indices + new values after summing/clamping).
pub fn apply_sample(
    routes: &mut [Route],
    current: &mut [f32; NUM_MACROS],
    signal: &str,
    sample: &Sample,
    t_s: f64,
) -> Vec<(usize, f32)> {
    let mut touched = [false; NUM_MACROS];
    for r in routes.iter_mut() {
        if !r.enabled || r.signal != signal {
            continue;
        }
        r.last_v = r.cond.step(sample, t_s);
        touched[r.macro_index] = true;
    }
    // Recompute touched macros as the clamped sum of all enabled routes.
    let mut changed = Vec::new();
    for (i, t) in touched.iter().enumerate() {
        if !t {
            continue;
        }
        let new: f32 = routes
            .iter()
            .filter(|r| r.enabled && r.macro_index == i)
            .map(|r| r.last_v * r.depth)
            .sum::<f32>()
            .clamp(0.0, 1.0);
        if (new - current[i]).abs() > f32::EPSILON {
            current[i] = new;
            changed.push((i, new));
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
        let mut cur = [0.0f32; NUM_MACROS];
        let ids = [SIG_BRIGHTNESS, SIG_MOTION, SIG_TRIANGLE].into_iter().chain(PERCEPTION_DEFAULTS.iter().map(|d| d.0));
        for id in ids {
            for i in 0..60 {
                assert!(apply_sample(&mut routes, &mut cur, id, &s(0.8), i as f64 / 30.0).is_empty(), "{id}");
            }
        }
        assert!(cur.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn brightness_routes_to_signal_1() {
        let mut routes = default_routes(true);
        let mut cur = [0.0f32; NUM_MACROS];
        // Feed constant brightness until the one-euro settles.
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, SIG_BRIGHTNESS, &s(0.8), i as f64 / 30.0);
        }
        assert!((cur[0] - 0.8).abs() < 0.05, "sig1 {}", cur[0]);
        assert!((cur[1] - 0.0).abs() < 0.05);
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
        let mut cur = [0.0f32; NUM_MACROS];
        for i in 0..60 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur[31], 0.0);
        let mut routes = default_routes(true);
        let mut cur = [0.0f32; NUM_MACROS];
        for i in 0..90 {
            apply_sample(&mut routes, &mut cur, SIG_TRIANGLE, &s(1.0), i as f64 / 30.0);
        }
        assert!(cur[31] > 0.9);
    }

    #[test]
    fn many_to_many_sum_clamped() {
        let mut routes = vec![
            Route::new("x", 0, 0.7),
            Route::new("x", 0, 0.7),
        ];
        let mut cur = [0.0f32; NUM_MACROS];
        for i in 0..120 {
            apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur[0], 1.0);
    }

    #[test]
    fn negative_depth_drives_down() {
        let mut routes = vec![Route::new("x", 0, -1.0)];
        let mut cur = [0.5f32; NUM_MACROS];
        for i in 0..120 {
            apply_sample(&mut routes, &mut cur, "x", &s(1.0), i as f64 / 30.0);
        }
        assert_eq!(cur[0], 0.0);
    }
}
