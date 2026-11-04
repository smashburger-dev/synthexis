//! Pong runtime: a writer with a manual player, ticked by hand.

use std::collections::HashMap;

use super::arena::{Life, Side};
use super::run::Input;
use super::setup::{Action, How, Mode};
use super::shot::synth::{hand, place, THUMB_OUT};
use super::*;
use crate::engine::{Demand, SignalSubscriber};
use crate::midi;

const FRAME_NS: u64 = 33_333_333;
const TICK_NS: u64 = 2_000_000;
/// A point shoots this long after it shows (shot::HOLD_NS).
const HOLD_MS: u64 = shot::HOLD_NS / 1_000_000;
const FIST: u8 = 0b00000;
const POINT: u8 = 0b01000;
const PORTAL: &str = "fx:0:0:Portal";

#[derive(Default)]
struct Rec {
    notes: Mutex<Vec<(u32, u8, u8)>>,
}

impl midi::Out for Rec {
    fn notes(&self, ev: &[(u32, u8, u8)]) {
        self.notes.lock().extend_from_slice(ev);
    }
}

/// Parameter writes (key id, index, value).
#[derive(Default)]
struct Params {
    writes: Mutex<Vec<(String, u32, f32)>>,
}

impl Sink for Params {
    fn param(&self, key: &crate::fl::Key, index: u32, v: f32) {
        self.writes.lock().push((key.id(), index, v));
    }
}

fn rig() -> (Arc<Writer>, Arc<Rec>, Arc<Params>) {
    let rec = Arc::new(Rec::default());
    let sink = Arc::new(Params::default());
    let w = Writer::with_parts(Arc::new(crate::params::SynthexisParams::default()), midi::Player::manual(rec.clone()), sink.clone());
    (w, rec, sink)
}

/// A note part on channel 1 that starts at once.
fn note(store: &mut midi::Store, pitch: u8) -> u64 {
    store.add("wall", midi::Play::Note { pitch, vel: 100 }, Some(midi::Channel { index: 1, name: "Pad".into() }))
}

/// Pong on with `top` on the top wall.
fn on_with(w: &Writer, top: Action) {
    let mut s = Setup { on: true, ..Setup::default() };
    s.walls[Side::Top.index()] = top;
    w.pong.set_setup(s);
}

fn ramp() -> How {
    How { mode: Mode::Ramp, lo: 0.0, hi: 1.0, depth_from_speed: true, back_ms: 400 }
}

/// Ticks every 2 ms from `t0` for `ms`; the right hand points up from the
/// frame centre with `mask(frame)`, a new camera frame every 33 ms.
/// Returns the time of the last tick.
fn play(w: &Writer, t0: u64, ms: u64, live: bool, mask: impl Fn(u64) -> u8) -> u64 {
    let pose = place(hand(0.0, THUMB_OUT, 0.0), 0.0, 0.2, [0.5, 0.5]);
    let mut now = t0;
    for k in 0..ms * 1_000_000 / TICK_NS {
        now = t0 + k * TICK_NS;
        let frame = (now - t0) / FRAME_NS;
        let ft = t0 + frame * FRAME_NS;
        let input = Input { masks: [None, Some((mask(frame), ft))], skeleton: [None, Some((pose, ft))], live, ..Input::default() };
        w.pong.tick(now, &input);
        w.player.tick(now);
    }
    now
}

/// A fist for two frames, then pointing.
fn point(frame: u64) -> u8 {
    if frame < 2 {
        FIST
    } else {
        POINT
    }
}

/// No hand in view, Live on.
fn tick_empty(w: &Writer, now: u64) {
    w.pong.tick(now, &Input { live: true, ..Input::default() });
    w.player.tick(now);
}

#[test]
fn pointing_shoots_and_the_top_wall_plays_its_part() {
    let (w, rec, _) = rig();
    let mut store = midi::Store::default();
    let p = note(&mut store, 84);
    w.update_midi(store);
    on_with(&w, Action::Part { id: p });
    assert_eq!(w.player.store().walls, vec![p], "the store knows the wall's part");
    play(&w, midi::now_ns(), 400 + HOLD_MS, true, point);
    let v = w.pong.view();
    assert_eq!((v.fired, v.hits), (1, 1), "{v:?}");
    assert_eq!(v.shots.len(), 1, "it flies on to its next wall");
    assert!(rec.notes.lock().iter().any(|n| n.1 == 84 && n.2 > 0), "{:?}", rec.notes.lock());
}

#[test]
fn a_macro_wall_toggles_its_signal() {
    let (w, _, _) = rig();
    on_with(&w, Action::Macro { index: 4, how: How { mode: Mode::Toggle, lo: 0.2, hi: 0.9, ..How::default() } });
    let mut out = [0.0; crate::route::NUM_MACROS];
    play(&w, midi::now_ns(), 400 + HOLD_MS, true, point);
    w.read_snapshot(&mut out);
    assert_eq!(out[4], 0.9);
}

#[test]
fn a_parameter_ramp_jumps_by_the_shot_and_glides_back_to_rest() {
    let (w, _, sink) = rig();
    on_with(&w, Action::Param { key: PORTAL.into(), index: 12, name: "Mix".into(), how: ramp() });
    play(&w, midi::now_ns(), 800 + HOLD_MS, true, point);
    let writes: Vec<f32> = sink.writes.lock().iter().map(|(k, i, v)| if k == PORTAL && *i == 12 { *v } else { f32::NAN }).collect();
    assert!(writes.iter().all(|v| v.is_finite()), "{writes:?}");
    // a point's strength is 0.8
    assert!((writes[0] - shot::POINT_STRENGTH).abs() < 1e-6, "{writes:?}");
    assert!(writes.windows(2).all(|p| p[1] < p[0]), "glides down: {writes:?}");
    assert_eq!(writes.last().copied(), Some(0.0));
    // 400 ms in steps of at least 33 ms
    assert!(writes.len() >= 8 && writes.len() <= 14, "{}", writes.len());
}

#[test]
fn hits_on_one_side_inside_30_ms_fire_once() {
    let (w, _, sink) = rig();
    on_with(&w, Action::Param { key: PORTAL.into(), index: 1, name: String::new(), how: How { mode: Mode::Toggle, ..How::default() } });
    {
        let mut run = w.pong.run.lock();
        for _ in 0..3 {
            run.arena.shoot([0.5, 0.1], [0.0, -1.0], 1.0, 0.5, 16);
        }
        // a fourth that reaches the top 40 ms later
        run.arena.shoot([0.5, 0.14], [0.0, -1.0], 1.0, 0.5, 16);
    }
    let t0 = midi::now_ns();
    let mut now = t0;
    while now < t0 + 300_000_000 {
        tick_empty(&w, now);
        now += TICK_NS;
    }
    let v = w.pong.view();
    assert_eq!((v.hits, v.dropped_window), (2, 2), "{v:?}");
    // hi on the first hit, lo on the one 40 ms later
    assert_eq!(*sink.writes.lock(), vec![(PORTAL.to_string(), 1, 1.0), (PORTAL.to_string(), 1, 0.0)]);
}

#[test]
fn a_burst_of_64_bouncing_for_a_minute_keeps_the_notes_balanced_and_in_budget() {
    let (w, rec, _) = rig();
    let mut store = midi::Store::default();
    let (top, bottom) = (note(&mut store, 84), note(&mut store, 48));
    w.update_midi(store);
    let mut s = Setup { on: true, max: 64, life: Life { bounces: None, seconds: None }, ..Setup::default() };
    s.walls[Side::Top.index()] = Action::Part { id: top };
    s.walls[Side::Bottom.index()] = Action::Part { id: bottom };
    s.walls[Side::Left.index()] = Action::Macro { index: 0, how: ramp() };
    w.pong.set_setup(s.clone());
    w.pong.test_shots(64);
    let t0 = midi::now_ns();
    let mut now = t0;
    while now < t0 + 60_000_000_000 {
        tick_empty(&w, now);
        now += TICK_NS;
    }
    let v = w.pong.view();
    assert_eq!((v.fired, v.shots.len()), (64, 64), "never: all still fly");
    assert!(v.hits > 1000, "{v:?}");
    w.pong.set_setup(Setup { on: false, ..s });
    for k in 0..2000 {
        w.player.tick(now + k * TICK_NS);
    }
    let notes = rec.notes.lock();
    let mut on: HashMap<(u32, u8), bool> = HashMap::new();
    for (ch, pitch, vel) in notes.iter() {
        on.insert((*ch, *pitch), *vel > 0);
    }
    assert!(on.values().all(|v| !v), "a note hangs: {on:?}");
    let ons = notes.iter().filter(|n| n.2 > 0).count();
    assert!(ons > 0 && ons <= 8 + 40 * 61, "{ons} note-ons");
    assert!(w.pong.view().shots.is_empty(), "Pong off clears");
}

#[test]
fn live_off_clears_the_arena_writes_rest_and_shoots_nothing() {
    let (w, _, sink) = rig();
    on_with(&w, Action::Param { key: PORTAL.into(), index: 3, name: String::new(), how: How { back_ms: 5000, ..ramp() } });
    let now = play(&w, midi::now_ns(), 400 + HOLD_MS, true, point);
    assert_eq!(w.pong.view().hits, 1);
    let before = sink.writes.lock().len();
    let then = play(&w, now + TICK_NS, 300, false, |f| if f % 4 < 2 { FIST } else { POINT });
    let v = w.pong.view();
    assert!(v.shots.is_empty() && v.fired == 1, "{v:?}");
    let writes = sink.writes.lock();
    assert_eq!(writes.len(), before + 1, "lo once");
    assert_eq!(writes.last().map(|w| w.2), Some(0.0));
    drop(writes);
    // Live on again: points shoot again
    play(&w, then + TICK_NS, 400 + HOLD_MS, true, point);
    assert_eq!(w.pong.view().fired, 2);
}

#[test]
fn deactivate_writes_rest_before_the_next_tick() {
    // the instance may be unloaded right after deactivate: no tick follows
    let (w, _, sink) = rig();
    on_with(&w, Action::Param { key: PORTAL.into(), index: 3, name: String::new(), how: How { back_ms: 5000, ..ramp() } });
    play(&w, midi::now_ns(), 400 + HOLD_MS, true, point);
    assert_eq!(w.pong.view().hits, 1);
    w.pong.suspend();
    w.player.stop_all();
    assert_eq!(sink.writes.lock().last().map(|w| w.2), Some(0.0));
    assert!(w.pong.view().shots.is_empty());
}

#[test]
fn a_side_hit_with_a_new_target_ends_the_old_ramp() {
    // the setup swapped before set_setup's end_side: a tick in between
    // hits the side with its new target
    let (w, _, sink) = rig();
    let how = How { back_ms: 5000, ..ramp() };
    on_with(&w, Action::Param { key: PORTAL.into(), index: 3, name: String::new(), how });
    let now = play(&w, midi::now_ns(), 400 + HOLD_MS, true, point);
    let mut s = (*w.pong.setup()).clone();
    s.walls[Side::Top.index()] = Action::Param { key: PORTAL.into(), index: 4, name: String::new(), how };
    *w.pong.setup.lock() = Arc::new(s);
    w.pong.run.lock().arena.shoot([0.5, 0.1], [0.0, -1.0], 1.0, 0.5, 16);
    let mut t = now + TICK_NS;
    while t < now + 300_000_000 {
        tick_empty(&w, t);
        t += TICK_NS;
    }
    let old: Vec<f32> = sink.writes.lock().iter().filter(|(_, i, _)| *i == 3).map(|w| w.2).collect();
    assert_eq!(old.last().copied(), Some(0.0), "{old:?}");
    assert!(sink.writes.lock().iter().any(|(_, i, _)| *i == 4));
}

#[test]
fn deactivate_suspends_until_the_next_process() {
    let (w, _, _) = rig();
    on_with(&w, Action::Off);
    w.pong.test_shots(4);
    let t0 = midi::now_ns();
    tick_empty(&w, t0);
    assert_eq!(w.pong.view().shots.len(), 4);
    w.pong.suspend();
    tick_empty(&w, t0 + TICK_NS);
    w.pong.test_shots(4);
    assert!(w.pong.view().shots.is_empty(), "cleared, no new shots");
    w.pong.resume();
    w.pong.test_shots(1);
    assert_eq!(w.pong.view().shots.len(), 1);
}

#[test]
fn the_thread_ends_when_the_writer_goes() {
    let (w, _, _) = rig();
    let handle = Pong::spawn(&w.pong);
    assert!(handle.is_some(), "no thread");
    let Some(handle) = handle else { return };
    assert!(Pong::spawn(&w.pong).is_none(), "one thread per Pong");
    on_with(&w, Action::Off);
    std::thread::sleep(Duration::from_millis(20));
    assert!(!handle.is_finished());
    drop(w);
    let t = std::time::Instant::now();
    while !handle.is_finished() && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(handle.is_finished(), "the thread outlived its writer");
}

#[test]
fn needs_perception_while_pong_is_on() {
    let (w, _, _) = rig();
    assert_eq!(w.needs(), Demand::default());
    on_with(&w, Action::Off);
    assert_eq!(w.needs(), Demand { camera: true, perception: true });
    w.pong.set_setup(Setup::default());
    assert_eq!(w.needs(), Demand::default());
}

#[test]
fn the_setup_is_saved_with_the_project() {
    let (w, _, _) = rig();
    on_with(&w, Action::Macro { index: 2, how: How::default() });
    assert_eq!(Setup::load(&w.params.pong.read()), *w.pong.setup());
}
