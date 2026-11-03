//! The H2 write path: one coalesced main-queue dispatch per signal frame,
//! per-write gestures on macros that moved >= 1/1024. Plus the audio-thread
//! snapshot and the camera->parameter latency instrumentation.
//!
//! The dispatch block touches ObjC (dispatch2); everything else is safe.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

use dispatch2::DispatchQueue;
use nih_plug::prelude::*;

use crate::engine::SignalSubscriber;
use crate::log;
use crate::route::{apply_sample, default_routes, hands_used, Route, Target, Values, NUM_MACROS};
use crate::signal::Sample;

const WRITE_EPS: f32 = 1.0 / 1024.0;
const LOG_EVERY_S: u64 = 5;

/// Hands [L, R] each live instance's routes use, by instance number: an
/// instance's hand x routes split when another instance routes the other
/// hand (route::fit_hand_x). A leaf lock: nothing else is taken under it.
static HANDS_USED: Mutex<Vec<(u64, [bool; 2])>> = Mutex::new(Vec::new());
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Records the hands instance `me` uses; returns those the others use.
fn share_hands(me: u64, used: [bool; 2]) -> [bool; 2] {
    let mut all = HANDS_USED.lock();
    match all.iter_mut().find(|e| e.0 == me) {
        Some(e) => e.1 = used,
        None => all.push((me, used)),
    }
    all.iter().filter(|e| e.0 != me).fold([false; 2], |a, e| [a[0] || e.1[0], a[1] || e.1[1]])
}

/// Per-instance writer state. Registers with the engine as a subscriber.
pub struct Writer {
    pub params: Arc<crate::params::SynthexisParams>,
    host_ctx: Mutex<Option<Arc<dyn GuiContext>>>,
    /// Test routes plus the assigned ones (Route::id, 0 for tests).
    routes: Mutex<Vec<Route>>,
    next_id: AtomicU64,
    /// Plugin parameter values (bridge path) and the gates' last signal
    /// values; its macros are refreshed from `written` per sample.
    values: Mutex<Values>,
    /// Self-reference so subscriber calls can re-enter `submit_main`
    /// (the trait gives us `&self`, the dispatch block needs `Arc<Self>`).
    me: Mutex<std::sync::Weak<Self>>,
    /// Latest target values (f32 bits) + per-macro capture timestamps of the
    /// camera frame that produced them (0 for non-camera signals like
    /// test.triangle, whose t_capture_ns is dispatch time and would pollute
    /// the camera-to-parameter metric).
    latest: Box<[AtomicU32; NUM_MACROS]>,
    latest_ts: Box<[AtomicU64; NUM_MACROS]>,
    pending: AtomicBool,
    /// Last written values — main queue only.
    written: Mutex<[f32; NUM_MACROS]>,
    /// Snapshot the audio thread reads once per block.
    pub snapshot: Box<[AtomicU32; NUM_MACROS]>,
    /// Instrumentation.
    writes: AtomicU64,
    cam_to_param_ms: Mutex<std::collections::VecDeque<f64>>,
    last_log: Mutex<Option<Instant>>,
    /// Key in HANDS_USED.
    instance: u64,
}

impl Writer {
    pub fn new(params: Arc<crate::params::SynthexisParams>) -> Arc<Self> {
        let test_on = params.test_signal.value();
        let w = Arc::new(Self {
            me: Mutex::new(std::sync::Weak::new()),
            params,
            host_ctx: Mutex::new(None),
            routes: Mutex::new(default_routes(test_on)),
            next_id: AtomicU64::new(1),
            values: Mutex::new(Values::default()),
            latest: Box::new([const { AtomicU32::new(0) }; NUM_MACROS]),
            latest_ts: Box::new([const { AtomicU64::new(0) }; NUM_MACROS]),
            pending: AtomicBool::new(false),
            written: Mutex::new([0.0; NUM_MACROS]),
            snapshot: Box::new([const { AtomicU32::new(0) }; NUM_MACROS]),
            writes: AtomicU64::new(0),
            cam_to_param_ms: Mutex::new(std::collections::VecDeque::new()),
            last_log: Mutex::new(None),
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
        });
        *w.me.lock() = Arc::downgrade(&w);
        w
    }

    pub fn set_context(&self, ctx: Arc<dyn GuiContext>) {
        let api = format!("{:?}", ctx.plugin_api());
        *self.host_ctx.lock() = Some(ctx);
        log::log(&format!("host context available (api={api})"));
    }

    /// Adds assistant routes as pending (they run at once); returns their ids.
    pub fn propose(&self, routes: Vec<Route>) -> Vec<u64> {
        let mut table = self.routes.lock();
        routes
            .into_iter()
            .map(|mut r| {
                r.id = self.next_id.fetch_add(1, Ordering::Relaxed);
                r.pending = true;
                let id = r.id;
                table.push(r);
                id
            })
            .collect()
    }

    /// True when a route of `line` runs.
    pub fn has_line(&self, line: u64) -> bool {
        line != 0 && self.routes.lock().iter().any(|r| r.line == line)
    }

    /// (id, signal, target) of the routes of `line`.
    pub fn line_routes(&self, line: u64) -> Vec<(u64, String, Target)> {
        self.routes.lock().iter().filter(|r| line != 0 && r.line == line).map(|r| (r.id, r.signal.clone(), r.target.clone())).collect()
    }

    /// Keeps pending routes for good.
    pub fn accept(&self, ids: &[u64]) {
        for r in self.routes.lock().iter_mut().filter(|r| ids.contains(&r.id)) {
            r.pending = false;
        }
    }

    /// Removes routes; the targets keep their last value.
    pub fn remove(&self, ids: &[u64]) {
        self.routes.lock().retain(|r| !ids.contains(&r.id));
    }

    /// Flips the direction of routes (one click for a wrong INV).
    pub fn flip(&self, ids: &[u64]) {
        for r in self.routes.lock().iter_mut().filter(|r| ids.contains(&r.id)) {
            r.cond.range.invert = !r.cond.range.invert;
        }
    }

    /// (id, pending, inverted) of the assigned routes.
    #[cfg(test)]
    pub fn assigned(&self) -> Vec<(u64, bool, bool)> {
        self.routes.lock().iter().filter(|r| r.id > 0).map(|r| (r.id, r.pending, r.cond.range.invert)).collect()
    }

    /// Called by process() once per block (audio thread).
    pub fn read_snapshot(&self, out: &mut [f32; NUM_MACROS]) {
        for (i, a) in self.snapshot.iter().enumerate() {
            out[i] = f32::from_bits(a.load(Ordering::Relaxed));
        }
    }

    /// Queue a coalesced write onto the main queue.
    fn submit_main(&self) {
        let this = match self.me.lock().upgrade() {
            Some(t) => t,
            None => return,
        };
        if self.pending.swap(true, Ordering::AcqRel) {
            return; // a block is already queued; it will read the latest
        }
        DispatchQueue::main().exec_async(move || {
            let guard = &*this;
            let ctx = guard.host_ctx.lock().clone();
            let setter = match ctx.as_ref() {
                Some(c) => ParamSetter::new(c.as_ref()),
                None => {
                    guard.pending.store(false, Ordering::Release);
                    return;
                }
            };
            let sig_params = guard.params.signals();
            let mut written = guard.written.lock();
            let mut n_writes = 0u64;
            let mut max_cam_ms = 0.0f64;
            for (i, param) in sig_params.iter().enumerate() {
                let target = f32::from_bits(guard.latest[i].load(Ordering::Relaxed));
                if (target - written[i]).abs() >= WRITE_EPS {
                    setter.begin_set_parameter(*param);
                    setter.set_parameter(*param, target);
                    setter.end_set_parameter(*param);
                    written[i] = target;
                    n_writes += 1;
                    let ts = guard.latest_ts[i].load(Ordering::Relaxed);
                    if ts > 0 {
                        let done_ns = crate::engine::host_time_ns();
                        max_cam_ms = max_cam_ms
                            .max(done_ns.saturating_sub(ts) as f64 / 1e6);
                    }
                }
            }
            drop(written);
            guard.pending.store(false, Ordering::Release);
            if n_writes > 0 {
                guard.writes.fetch_add(n_writes, Ordering::Relaxed);
                if max_cam_ms > 0.0 {
                    let mut v = guard.cam_to_param_ms.lock();
                    v.push_back(max_cam_ms);
                    while v.len() > 300 {
                        v.pop_front();
                    }
                }
            }
        });
    }

    fn periodic_log(&self) {
        let mut last = self.last_log.lock();
        let now = Instant::now();
        if last.map(|t| t.elapsed().as_secs() >= LOG_EVERY_S).unwrap_or(true) {
            *last = Some(now);
            let v = self.cam_to_param_ms.lock();
            let (med, p95) = crate::engine::camera::median_p95(&v);
            log::log(&format!(
                "writes_total={} cam_to_param_ms med={med:.1} p95={p95:.1}",
                self.writes.load(Ordering::Relaxed)
            ));
        }
    }
}

/// What the enabled routes read; test routes count while Test signal is on.
fn routes_need(routes: &[Route], test_on: bool) -> crate::engine::Demand {
    routes
        .iter()
        .filter(|r| if r.test { test_on } else { r.enabled })
        .fold(crate::engine::Demand::default(), |d, r| d.or(crate::engine::Demand::of_signal(&r.signal)))
}

impl SignalSubscriber for Writer {
    fn needs(&self) -> crate::engine::Demand {
        routes_need(&self.routes.lock(), self.params.test_signal.value())
    }

    fn on_sample(&self, signal_id: &str, sample: &Sample) {
        let mut writes = Vec::new();
        let live = self.params.live.value();
        {
            let mut routes = self.routes.lock();
            let test_on = self.params.test_signal.value();
            for r in routes.iter_mut().filter(|r| r.test) {
                r.enabled = test_on;
            }
            let mut current = self.values.lock();
            if signal_id.starts_with("cam0.hand.") && signal_id.ends_with(".x") {
                current.hands_used = share_hands(self.instance, hands_used(&routes));
            }
            current.macros = *self.written.lock();
            let t_s = sample.t_capture_ns as f64 / 1e9;
            let changed = apply_sample(&mut routes, &mut current, signal_id, sample, t_s);
            if !live {
                // not sent, so not written: the first sample with Live on
                // sends it (macros catch up through `written` instead)
                for (t, _) in changed.iter().filter(|(t, _)| matches!(t, Target::Param { .. })) {
                    current.params.insert(t.clone(), f32::NAN);
                }
            }
            drop(current);
            if changed.is_empty() {
                self.periodic_log();
                return;
            }
            for (t, v) in &changed {
                match t {
                    Target::Macro(i) => {
                        self.latest[*i].store(v.to_bits(), Ordering::Relaxed);
                        self.snapshot[*i].store(v.to_bits(), Ordering::Relaxed);
                        self.latest_ts[*i].store(
                            if signal_id.starts_with("cam") { sample.t_capture_ns } else { 0 },
                            Ordering::Relaxed,
                        );
                    }
                    Target::Param { plugin, index } => writes.push((plugin.clone(), *index, *v)),
                }
            }
        }
        if !live {
            self.periodic_log();
            return;
        }
        // plugin parameters go through the FL script, not the host
        for (plugin, index, v) in writes {
            crate::fl::bridge().write(&plugin, index, v);
        }
        if self.host_ctx.lock().is_none() {
            self.periodic_log();
            return;
        }
        self.submit_main();
        self.periodic_log();
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        HANDS_USED.lock().retain(|e| e.0 != self.instance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Demand;

    #[test]
    fn instances_share_the_hands_their_routes_use() {
        // instance numbers far from NEXT_INSTANCE's, so live writers do not mix in
        let (a, b) = (u64::MAX - 1, u64::MAX - 2);
        assert_eq!(share_hands(a, [true, false]), [false, false]);
        assert_eq!(share_hands(b, [false, true]), [true, false]);
        assert_eq!(share_hands(a, [true, false]), [false, true]);
        HANDS_USED.lock().retain(|e| e.0 != b);
        assert_eq!(share_hands(a, [true, false]), [false, false]);
        HANDS_USED.lock().retain(|e| e.0 != a);
    }

    #[test]
    fn test_routes_need_camera_and_perception_only_while_test_signal_is_on() {
        assert_eq!(routes_need(&default_routes(false), false), Demand::default());
        assert_eq!(routes_need(&default_routes(true), true), Demand { camera: true, perception: true });
        let mut only_triangle = default_routes(true);
        only_triangle.retain(|r| r.signal == crate::signal::SIG_TRIANGLE);
        assert_eq!(routes_need(&only_triangle, true), Demand::default());
        // an assigned (non-test) route counts while enabled, whatever Test signal says
        let mut assigned = vec![Route::new("cam0.hand.R.pinch", 0, 1.0)];
        assert_eq!(routes_need(&assigned, false), Demand { camera: true, perception: true });
        assigned[0].enabled = false;
        assert_eq!(routes_need(&assigned, false), Demand::default());
    }
}
