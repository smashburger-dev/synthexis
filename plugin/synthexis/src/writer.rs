//! The H2 write path: one coalesced main-queue dispatch per signal frame,
//! per-write gestures on macros that moved >= 1/1024. Plus the audio-thread
//! snapshot and the camera->parameter latency instrumentation. Routes to
//! MIDI parts hand their trigger edges to the instance's player (midi.rs).
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
use crate::midi;
use crate::route::{apply_sample, default_routes, hand_of, hands_used, Route, Target, Values, NUM_MACROS};
use crate::signal::Sample;

const WRITE_EPS: f32 = 1.0 / 1024.0;
/// Line of the routes made from the MIDI store's binds (what each finger
/// plays); no sentence line gets it.
pub const BIND_LINE: u64 = u64::MAX;
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
    /// Plays this instance's MIDI parts.
    pub player: Arc<midi::Player>,
    /// Live as the last sample saw it (the player starts with Live on).
    was_live: AtomicBool,
    /// The FL bridge whose channel rack the parts follow; set in
    /// initialize, so a plugin scan opens no bridge.
    fl: std::sync::OnceLock<&'static crate::fl::Bridge>,
    /// The bridge view's gen the parts' channels were last checked at.
    fl_gen: AtomicU64,
    /// Pong mode of this instance.
    pub pong: Arc<crate::pong::Pong>,
    /// Bumped when routes, parts and Pong were replaced as a whole (a
    /// preset, a project load): the window reloads what it shows.
    state_gen: AtomicU64,
}

impl Writer {
    pub fn new(params: Arc<crate::params::SynthexisParams>) -> Arc<Self> {
        let w = Self::with_parts(params, midi::Player::new(Arc::new(midi::Bridge)), Arc::new(crate::pong::FlSink));
        crate::pong::Pong::spawn(&w.pong);
        w
    }

    #[cfg(test)]
    fn with_player(params: Arc<crate::params::SynthexisParams>, player: Arc<midi::Player>) -> Arc<Self> {
        Self::with_parts(params, player, Arc::new(crate::pong::FlSink))
    }

    /// A writer whose Pong has no thread yet and writes parameters to `sink`.
    pub(crate) fn with_parts(
        params: Arc<crate::params::SynthexisParams>,
        player: Arc<midi::Player>,
        sink: Arc<dyn crate::pong::Sink>,
    ) -> Arc<Self> {
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
            player,
            was_live: AtomicBool::new(true),
            fl: std::sync::OnceLock::new(),
            fl_gen: AtomicU64::new(0),
            pong: crate::pong::Pong::new(sink),
            state_gen: AtomicU64::new(0),
        });
        *w.me.lock() = Arc::downgrade(&w);
        w.pong.attach(&w);
        w
    }

    /// From now on the parts follow their channels through `bridge`'s
    /// rack changes (follow_rack).
    pub fn follow_channels(&self, bridge: &'static crate::fl::Bridge) {
        let _ = self.fl.set(bridge);
    }

    /// When FL's view changed (an inventory with channels inserted, moved
    /// or deleted), the parts find their channels again
    /// (Store::rebind_channels). Polled on the test triangle, which runs
    /// with the window closed too; the rebind itself runs on the main
    /// queue, where the window and the sentence field make their edits of
    /// the store, so it never overwrites one of them.
    fn follow_rack(&self) {
        let Some(&bridge) = self.fl.get() else { return };
        let gen = bridge.view().gen;
        if self.fl_gen.swap(gen, Ordering::AcqRel) == gen {
            return;
        }
        let Some(this) = self.me.lock().upgrade() else { return };
        DispatchQueue::main().exec_async(move || {
            let (rows, focus) = {
                let v = bridge.view();
                (v.channels.clone(), v.focus.clone())
            };
            this.rack_changed(&rows, focus.as_ref());
            crate::preset::follow(&this, bridge);
        });
    }

    /// FL's channels now: the parts find theirs again, and the Pong parts
    /// waiting for one (Setup.auto_channel) get the focused generator, else
    /// the first channel, and leave the list. Nothing while the list is
    /// empty (no inventory yet).
    fn rack_changed(&self, rows: &[crate::fl::ChannelRow], focus: Option<&crate::fl::Key>) {
        let mut store = self.player.store();
        let mut changed = store.rebind_channels(rows);
        let setup = self.pong.setup();
        let focused = focus.and_then(|k| match k.at {
            crate::fl::At::Gen { ch } => rows.iter().find(|r| r.index == ch),
            crate::fl::At::Fx { .. } => None,
        });
        let row = focused.or(rows.first());
        if let Some(row) = row.filter(|_| !setup.auto_channel.is_empty()) {
            for id in &setup.auto_channel {
                if let Some(p) = store.get_mut(*id).filter(|p| p.channel.is_none()) {
                    let c = midi::Channel { index: row.index, name: row.label() };
                    midi::fit_to_channel(&mut p.play, &c.name);
                    p.channel = Some(c);
                    changed = true;
                }
            }
        }
        if changed {
            self.update_midi(store);
        }
        if row.is_some() && !setup.auto_channel.is_empty() {
            self.pong.set_setup(crate::pong::setup::Setup { auto_channel: Vec::new(), ..(*setup).clone() });
        }
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

    /// Removes routes; the targets keep their last value, a MIDI part a
    /// removed route held ends its notes.
    pub fn remove(&self, ids: &[u64]) {
        let mut parts = Vec::new();
        self.routes.lock().retain(|r| {
            let gone = ids.contains(&r.id);
            if gone && matches!(r.target, Target::Part { .. }) {
                parts.push(r.id);
            }
            !gone
        });
        for id in parts {
            self.player.release_route(id);
        }
    }

    /// Parts, key and binds as the window or a load has them: the player
    /// gets the parts, the binds become routes (BIND_LINE), the project
    /// keeps the JSON. A bind that stays keeps its route and trigger state,
    /// so a finger held while another bind changes plays on. The walls are
    /// the Pong setup's.
    pub fn set_midi(&self, mut store: midi::Store) {
        store.walls = self.pong.setup().wall_parts();
        let mut want: Vec<(String, Target)> = Vec::new();
        for b in store.binds.iter().filter(|b| store.get(b.part).is_some()) {
            let k = (b.signal.clone(), Target::Part { id: b.part, trigger: b.trigger });
            if !want.contains(&k) {
                want.push(k);
            }
        }
        let mut gone = Vec::new();
        for r in self.routes.lock().iter().filter(|r| r.line == BIND_LINE) {
            match want.iter().position(|(s, t)| *s == r.signal && *t == r.target) {
                Some(i) => {
                    want.swap_remove(i);
                }
                None => gone.push(r.id),
            }
        }
        self.remove(&gone);
        let routes: Vec<Route> = want
            .into_iter()
            .map(|(s, t)| {
                let mut r = Route::to(&s, t, 1.0);
                r.line = BIND_LINE;
                r
            })
            .collect();
        *self.params.midi.write() = store.save();
        self.player.set_store(store);
        let ids = self.propose(routes);
        self.accept(&ids);
    }

    /// Keep on a MIDI line: its routes `ids` become the bind routes
    /// `store` has for them, with their trigger state and flip, so the
    /// finger held while Keep is clicked plays on; the rest as set_midi.
    pub fn keep_midi(&self, ids: &[u64], store: midi::Store) {
        let mut gone = Vec::new();
        {
            let mut table = self.routes.lock();
            let mut bound: Vec<(String, Target)> =
                table.iter().filter(|r| r.line == BIND_LINE).map(|r| (r.signal.clone(), r.target.clone())).collect();
            for r in table.iter_mut().filter(|r| ids.contains(&r.id)) {
                let k = (r.signal.clone(), r.target.clone());
                let binds = store.binds.iter().any(|b| b.signal == k.0 && k.1 == Target::Part { id: b.part, trigger: b.trigger });
                if binds && !bound.contains(&k) {
                    (r.line, r.pending) = (BIND_LINE, false);
                    bound.push(k);
                } else {
                    gone.push(r.id);
                }
            }
        }
        self.remove(&gone);
        self.set_midi(store);
    }

    /// An edit in the window: the player and the project get the new
    /// parts at once; the bind routes are rebuilt only when the binds
    /// changed, so painting steps never restarts a held finger.
    pub fn update_midi(&self, mut store: midi::Store) {
        store.walls = self.pong.setup().wall_parts();
        if self.player.store().binds != store.binds {
            self.set_midi(store);
            return;
        }
        *self.params.midi.write() = store.save();
        self.player.set_store(store);
    }

    /// Trigger state of the bind routes now: (signal, trigger) that are on.
    pub fn binds_on(&self) -> Vec<(String, crate::route::Trigger)> {
        self.routes
            .lock()
            .iter()
            .filter_map(|r| match r.target {
                Target::Part { trigger, .. } if r.edge.on => Some((r.signal.clone(), trigger)),
                _ => None,
            })
            .collect()
    }

    /// Removes every assigned route (pending and kept sentence lines, the
    /// binds' routes); test routes stay. Held parts end.
    pub fn clear_assigned(&self) {
        let ids: Vec<u64> = self.routes.lock().iter().filter(|r| !r.test).map(|r| r.id).collect();
        self.remove(&ids);
    }

    /// The highest sentence line in use (0: none).
    pub fn max_line(&self) -> u64 {
        self.routes.lock().iter().map(|r| r.line).filter(|l| *l != BIND_LINE).max().unwrap_or(0)
    }

    pub fn bump_state(&self) {
        self.state_gen.fetch_add(1, Ordering::AcqRel);
    }

    pub fn state_gen(&self) -> u64 {
        self.state_gen.load(Ordering::Acquire)
    }

    /// The next inventory finds the parts' channels again (a preset loaded
    /// before FL's channel list was there).
    pub fn reset_fl_gen(&self) {
        self.fl_gen.store(0, Ordering::Release);
    }

    /// Sets Signal `index + 1` from any thread (a Pong wall): the audio
    /// snapshot at once, the host through the coalesced main-queue write.
    /// A route on the same macro overwrites it with its next sample.
    pub fn set_macro(&self, index: usize, v: f32) {
        let (Some(latest), Some(snap), Some(ts)) = (self.latest.get(index), self.snapshot.get(index), self.latest_ts.get(index)) else {
            return;
        };
        let v = v.clamp(0.0, 1.0);
        latest.store(v.to_bits(), Ordering::Relaxed);
        snap.store(v.to_bits(), Ordering::Relaxed);
        ts.store(0, Ordering::Relaxed);
        if self.host_ctx.lock().is_some() {
            self.submit_main();
        }
    }

    /// Pong pauses the fingers of its shooting hands (`paused`, [L, R]).
    /// A hand that pauses ends every part its routes play, whatever their
    /// mode, unless a route that plays on holds it: then only its own
    /// routes let go. One that plays again re-sends the on edge of its
    /// routes whose trigger is held, as Live on does. Under the routes
    /// lock, like the edges of take_sample.
    pub fn set_paused(&self, paused: [bool; 2]) {
        let routes = self.routes.lock();
        let old = std::mem::replace(&mut self.values.lock().paused, paused);
        let held: Vec<u64> = routes
            .iter()
            .filter(|r| r.enabled && r.edge.on && hand_of(&r.signal).is_none_or(|h| !paused[h]))
            .filter_map(|r| if let Target::Part { id, .. } = r.target { Some(id) } else { None })
            .collect();
        let mut ended = Vec::new();
        let mut edges = Vec::new();
        for r in routes.iter().filter(|r| r.enabled) {
            let (Target::Part { id, .. }, Some(h)) = (&r.target, hand_of(&r.signal)) else { continue };
            if paused[h] && !old[h] {
                if !held.contains(id) {
                    if !ended.contains(id) {
                        ended.push(*id);
                    }
                } else if r.edge.on {
                    edges.push((r.id, *id, false));
                }
            } else if !paused[h] && old[h] && r.edge.on {
                edges.push((r.id, *id, true));
            }
        }
        if !ended.is_empty() {
            self.player.end_parts(&ended);
        }
        for (route, part, on) in edges {
            self.player.trigger(route, part, on);
        }
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
        let d = routes_need(&self.routes.lock(), self.params.test_signal.value());
        // Pong shoots with the window closed too
        if self.pong.is_on() {
            d.or(crate::engine::Demand { camera: true, perception: true })
        } else {
            d
        }
    }

    fn on_sample(&self, signal_id: &str, sample: &Sample) {
        self.take_sample(signal_id, sample, || self.params.live.value());
        if signal_id == crate::signal::SIG_TRIANGLE {
            self.follow_rack();
        }
        self.periodic_log();
    }
}

impl Writer {
    /// on_sample with Live from `is_live` (tests switch it).
    fn take_sample(&self, signal_id: &str, sample: &Sample, is_live: impl Fn() -> bool) {
        let mut writes = Vec::new();
        let live;
        {
            let mut routes = self.routes.lock();
            // read and passed on under the routes lock, so the engine's
            // threads see Live change in one order
            live = is_live();
            self.player.set_live(live);
            let woke = !self.was_live.swap(live, Ordering::AcqRel) && live;
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
            let mut edges = Vec::new();
            if woke {
                // Live came on: a trigger held since Live off starts its
                // part now (the player dropped its on edge); a paused
                // hand's parts stay silent
                let paused = current.paused;
                edges.extend(routes.iter().filter_map(|r| match r.target {
                    Target::Part { id, .. } if r.enabled && r.edge.on && !hand_of(&r.signal).is_some_and(|h| paused[h]) => Some((r.id, id, true)),
                    _ => None,
                }));
            }
            edges.append(&mut current.events);
            if !live {
                // not sent, so not written: the first sample with Live on
                // sends it (macros catch up through `written` instead)
                for (t, _) in changed.iter().filter(|(t, _)| matches!(t, Target::Param { .. })) {
                    current.params.insert(t.clone(), f32::NAN);
                }
            }
            drop(current);
            // still under the routes lock: a route that `remove` takes out
            // now is either gone before its edge or released after it
            for (route, part, on) in edges {
                self.player.trigger(route, part, on);
            }
            drop(routes);
            if changed.is_empty() {
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
                    Target::Part { .. } => {}
                }
            }
        }
        if !live {
            return;
        }
        // plugin parameters go through the FL script, not the host
        for (plugin, index, v) in writes {
            crate::fl::bridge().write(&plugin, index, v);
        }
        if self.host_ctx.lock().is_none() {
            return;
        }
        self.submit_main();
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

    use crate::route::{finger_bit, shape_value, Trigger};

    /// Notes out; `hook` runs once, on the first notes.
    #[derive(Default)]
    struct Rec {
        notes: Mutex<Vec<(u32, u8, u8)>>,
        hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl midi::Out for Rec {
        fn notes(&self, ev: &[(u32, u8, u8)]) {
            self.notes.lock().extend_from_slice(ev);
            let hook = self.hook.lock().take();
            if let Some(f) = hook {
                f();
            }
        }
    }

    const SHAPE_R: &str = "cam0.hand.R.shape";

    fn setup() -> (Arc<Writer>, Arc<Rec>) {
        let rec = Arc::new(Rec::default());
        let w = Writer::with_player(Arc::new(crate::params::SynthexisParams::default()), midi::Player::manual(rec.clone()));
        (w, rec)
    }

    /// A note part on channel 1 that starts at once.
    fn note(store: &mut midi::Store, pitch: u8, mode: midi::Mode) -> u64 {
        let id = store.add("n", midi::Play::Note { pitch, vel: 100 }, Some(midi::Channel { index: 1, name: "Pad".into() }));
        if let Some(p) = store.get_mut(id) {
            p.mode = mode;
        }
        id
    }

    fn finger(f: usize) -> Trigger {
        Trigger::Shape { mask: finger_bit(f), care: finger_bit(f) }
    }

    /// The right hand with `mask` shown at `t` s.
    fn show(w: &Writer, mask: u8, t: f64, live: bool) {
        let s = Sample { value: shape_value(mask), confidence: 1.0, t_capture_ns: (t * 1e9) as u64, present: true };
        w.take_sample(SHAPE_R, &s, || live);
    }

    #[test]
    fn edges_reach_the_player_while_the_routes_are_locked() {
        // A remove (Keep, a bind edit) takes the routes lock, then releases
        // what the route holds: with the edge delivered under that lock it
        // runs before the edge (no route, no edge) or after it
        // (release_route finds the holder), never in between, where the
        // edge would leave a holder no off edge can reach.
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let a = note(&mut store, 60, midi::Mode::Gate);
        w.update_midi(store);
        w.propose(vec![Route::to(SHAPE_R, Target::Part { id: a, trigger: finger(1) }, 1.0)]);
        let locked = Arc::new(AtomicBool::new(false));
        let (seen, weak) = (locked.clone(), Arc::downgrade(&w));
        // the first note goes out from inside player.trigger
        *rec.hook.lock() = Some(Box::new(move || {
            if let Some(w) = weak.upgrade() {
                seen.store(w.routes.try_lock().is_none(), Ordering::SeqCst);
            }
        }));
        show(&w, finger_bit(1), 0.0, true);
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100)]);
        assert!(locked.load(Ordering::SeqCst), "the routes were free while the edge went out");
    }

    #[test]
    fn a_bind_edit_leaves_a_held_finger_alone() {
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Latch);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store.clone());
        show(&w, finger_bit(1), 0.0, true);
        assert_eq!(w.player.playing(), vec![p]);
        // the thumb gets a part while the index is up
        let q = note(&mut store, 64, midi::Mode::Gate);
        store.bind(SHAPE_R, finger(0), q);
        w.update_midi(store);
        show(&w, finger_bit(1), 0.033, true);
        show(&w, finger_bit(1), 0.066, true);
        assert_eq!(w.player.playing(), vec![p], "latched, not toggled off");
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100)]);
    }

    #[test]
    fn keep_turns_the_held_line_into_its_bind() {
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Gate);
        w.update_midi(store.clone());
        let ids = w.propose(vec![Route::to(SHAPE_R, Target::Part { id: p, trigger: finger(1) }, 1.0)]);
        show(&w, finger_bit(1), 0.0, true);
        w.accept(&ids);
        store.bind(SHAPE_R, finger(1), p);
        w.keep_midi(&ids, store);
        show(&w, finger_bit(1), 0.033, true);
        assert_eq!(w.player.playing(), vec![p]);
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100)], "no note off and on again");
        assert_eq!(w.binds_on(), vec![(SHAPE_R.to_string(), finger(1))], "one route, the bind");
    }

    #[test]
    fn a_finger_held_through_live_off_starts_when_live_comes_on() {
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Gate);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store);
        show(&w, finger_bit(1), 0.0, false);
        show(&w, finger_bit(1), 0.033, false);
        assert!(w.player.playing().is_empty() && rec.notes.lock().is_empty(), "nothing while Live is off");
        show(&w, finger_bit(1), 0.066, true);
        assert_eq!(w.player.playing(), vec![p]);
        // off and on again with the finger still up
        show(&w, finger_bit(1), 0.1, false);
        show(&w, finger_bit(1), 0.133, true);
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100), (1, 60, 0), (1, 60, 100)]);
    }

    use crate::pong::setup::{Action, Setup};

    #[test]
    fn a_paused_hand_ends_its_parts_whatever_their_mode() {
        for mode in [midi::Mode::Gate, midi::Mode::Latch, midi::Mode::Once] {
            let (w, rec) = setup();
            let mut store = midi::Store::default();
            let p = note(&mut store, 60, mode);
            store.bind(SHAPE_R, finger(1), p);
            w.set_midi(store);
            show(&w, finger_bit(1), 0.0, true);
            assert_eq!(w.player.playing(), vec![p], "{mode:?}");
            // Pong on: the right hand shoots, its fingers pause
            w.pong.set_setup(Setup { on: true, ..Setup::default() });
            assert_eq!(w.pong.paused(), [false, true]);
            assert!(w.player.playing().is_empty(), "{mode:?}");
            assert_eq!(*rec.notes.lock(), vec![(1, 60, 100), (1, 60, 0)], "{mode:?}");
            show(&w, 0, 0.033, true);
            show(&w, finger_bit(1), 0.066, true);
            assert!(w.player.playing().is_empty(), "{mode:?}: a paused finger plays nothing");
        }
    }

    #[test]
    fn pausing_one_hand_keeps_the_part_the_other_hand_holds() {
        const SHAPE_L: &str = "cam0.hand.L.shape";
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Gate);
        store.bind(SHAPE_L, finger(1), p);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store);
        let left = |mask: u8, t: f64| {
            let s = Sample { value: shape_value(mask), confidence: 1.0, t_capture_ns: (t * 1e9) as u64, present: true };
            w.take_sample(SHAPE_L, &s, || true);
        };
        left(finger_bit(1), 0.0);
        show(&w, finger_bit(1), 0.0, true);
        assert_eq!(w.player.playing(), vec![p]);
        // the right hand shoots and pauses; the left one holds the part on
        w.pong.set_setup(Setup { on: true, ..Setup::default() });
        assert_eq!(w.pong.paused(), [false, true]);
        assert_eq!(w.player.playing(), vec![p]);
        left(finger_bit(1), 0.033);
        show(&w, finger_bit(1), 0.033, true);
        assert_eq!(w.player.playing(), vec![p], "the left finger still holds it");
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100)]);
        // the left finger goes down: the paused right one holds nothing
        left(0, 0.066);
        assert!(w.player.playing().is_empty());
        assert_eq!(*rec.notes.lock(), vec![(1, 60, 100), (1, 60, 0)]);
    }

    #[test]
    fn an_unpaused_hand_plays_the_finger_it_holds() {
        let (w, rec) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Latch);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store);
        show(&w, finger_bit(1), 0.0, true);
        w.pong.set_setup(Setup { on: true, ..Setup::default() });
        // off again before another frame: the held finger's edge is sent again
        w.pong.set_setup(Setup::default());
        assert_eq!(w.player.playing(), vec![p]);
        // paused across frames: the next frame starts it
        w.pong.set_setup(Setup { on: true, ..Setup::default() });
        show(&w, finger_bit(1), 0.033, true);
        w.pong.set_setup(Setup::default());
        assert!(w.player.playing().is_empty());
        show(&w, finger_bit(1), 0.066, true);
        assert_eq!(w.player.playing(), vec![p]);
        assert_eq!(rec.notes.lock().iter().filter(|n| n.2 > 0).count(), 3);
    }

    #[test]
    fn live_on_skips_a_paused_hand() {
        let (w, _) = setup();
        let mut store = midi::Store::default();
        // a latch: an off edge after the on would not end it
        let p = note(&mut store, 60, midi::Mode::Latch);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store);
        show(&w, finger_bit(1), 0.0, false);
        w.pong.set_setup(Setup { on: true, ..Setup::default() });
        show(&w, finger_bit(1), 0.033, true);
        assert!(w.player.playing().is_empty());
    }

    #[test]
    fn a_pong_part_waiting_for_a_channel_gets_the_focused_generator_else_the_first() {
        use crate::fl::{At, ChannelRow, Key};
        let (w, _) = setup();
        let mut store = midi::Store::default();
        let a = store.add("Pong top", midi::Play::Note { pitch: 84, vel: 100 }, None);
        let b = store.add("Pong bottom", midi::Play::Note { pitch: 48, vel: 100 }, None);
        let c = note(&mut store, 60, midi::Mode::Gate);
        w.update_midi(store);
        let mut s = Setup { auto_channel: vec![a, c], ..Setup::default() };
        s.walls[0] = Action::Part { id: a };
        w.pong.set_setup(s.clone());
        assert_eq!(w.player.store().walls, vec![a]);
        let rows = vec![
            ChannelRow { index: 0, name: "Kick".into(), plugin: String::new() },
            ChannelRow { index: 1, name: "Pad".into(), plugin: String::new() },
            ChannelRow { index: 3, name: "Lead".into(), plugin: "Serum 2".into() },
        ];
        // no inventory yet: everything waits
        w.rack_changed(&[], None);
        assert_eq!(w.pong.setup().auto_channel, vec![a, c]);
        w.rack_changed(&rows, Some(&Key { at: At::Gen { ch: 3 }, plugin: "Serum 2".into() }));
        let store = w.player.store();
        assert_eq!(store.get(a).and_then(|p| p.channel.clone()), Some(midi::Channel { index: 3, name: "Serum 2 · Lead".into() }));
        assert_eq!(store.get(c).and_then(|p| p.channel.clone()), Some(midi::Channel { index: 1, name: "Pad".into() }), "a part with a channel keeps it");
        assert!(w.pong.setup().auto_channel.is_empty());
        assert_eq!(store.walls, vec![a]);
        // a mixer effect in focus: the first channel
        w.pong.set_setup(Setup { auto_channel: vec![b], ..s });
        w.rack_changed(&rows, Some(&Key { at: At::Fx { track: 0, slot: 0 }, plugin: "Portal".into() }));
        assert_eq!(w.player.store().get(b).and_then(|p| p.channel.clone()), Some(midi::Channel { index: 0, name: "Kick".into() }));
    }

    #[test]
    fn clear_assigned_keeps_the_test_routes_and_max_line_skips_the_binds() {
        let (w, _) = setup();
        let mut store = midi::Store::default();
        let p = note(&mut store, 60, midi::Mode::Gate);
        store.bind(SHAPE_R, finger(1), p);
        w.set_midi(store);
        let mut r = Route::new("cam0.hand.R.y", 3, 1.0);
        r.line = 7;
        let ids = w.propose(vec![r]);
        w.accept(&ids);
        assert_eq!(w.max_line(), 7);
        let tests = w.routes.lock().iter().filter(|r| r.test).count();
        w.clear_assigned();
        assert!(w.assigned().is_empty());
        assert_eq!(w.routes.lock().len(), tests);
        assert_eq!(w.max_line(), 0);
        let g = w.state_gen();
        w.bump_state();
        assert_eq!(w.state_gen(), g + 1);
    }
}
