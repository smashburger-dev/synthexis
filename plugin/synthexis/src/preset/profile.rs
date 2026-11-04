//! Profiles (ui-redesign.md, Profiles): three working setups the top bar's
//! 1 2 3 switches between. A profile is a preset snapshot (kept lines, the
//! MIDI store, the Pong setup, the picker pin) plus the window's view (open
//! and folded sections, the picked preset). The active profile is the live
//! state itself; the other two wait in params `profiles`, saved with the
//! project. A profile never visited starts empty and keeps the picked
//! plugin and the MIDI key. An older project has no `profiles`: it opens
//! on profile 1, which holds its setup unchanged.

use serde::{Deserialize, Serialize};

use super::top::View;
use super::{apply_on, capture, check, Snapshot};
use crate::fl;
use crate::log::log;
use crate::midi;
use crate::params::SynthexisParams;
use crate::writer::Writer;

/// How many profiles the top bar offers.
pub const COUNT: usize = 3;
/// The version written into params `profiles`.
const VERSION: u32 = 1;

/// A waiting profile: its state and its view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    pub snapshot: Snapshot,
    pub view: View,
}

/// params `profiles`: `{"version":1,"active":0,"slots":[null,{..},null]}`.
/// The active profile's slot stays null (the live state holds it); a null
/// slot of another profile was never visited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profiles {
    pub version: u32,
    pub active: usize,
    pub slots: [Option<Slot>; COUNT],
}

impl Default for Profiles {
    fn default() -> Self {
        Profiles { version: VERSION, active: 0, slots: [None, None, None] }
    }
}

/// What the top bar shows: the active profile and which hold a setup
/// (the active one always does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub active: usize,
    pub filled: [bool; COUNT],
}

impl Profiles {
    /// The profiles saved with the project; empty (older projects) or
    /// unreadable gives profile 1 active and the others never visited.
    pub fn read(params: &SynthexisParams) -> Profiles {
        let json = params.profiles.read().clone();
        if json.trim().is_empty() {
            return Profiles::default();
        }
        match serde_json::from_str::<Profiles>(&json) {
            Ok(mut p) => {
                if p.active >= COUNT {
                    log(&format!("profiles: active {} out of range, profile 1 taken", p.active));
                    p.active = 0;
                }
                if let Some(s) = p.slots.get_mut(p.active) {
                    *s = None;
                }
                p
            }
            Err(e) => {
                log(&format!("profiles: not read ({e})"));
                Profiles::default()
            }
        }
    }

    pub fn write(&self, params: &SynthexisParams) {
        *params.profiles.write() = serde_json::to_string(self).unwrap_or_default();
    }

    pub fn summary(&self) -> Summary {
        let mut filled = [false; COUNT];
        for (i, f) in filled.iter_mut().enumerate() {
            *f = i == self.active || self.slots.get(i).is_some_and(Option::is_some);
        }
        Summary { active: self.active, filled }
    }
}

/// A profile never visited: no kept lines, no parts or binds, Pong off,
/// no section open; the picked plugin and the MIDI key of `now` stay.
pub fn fresh(now: &Snapshot) -> Slot {
    let snapshot = Snapshot { pin: now.pin.clone(), midi: midi::Store { key: now.midi.key, ..midi::Store::default() }, ..Snapshot::default() };
    Slot { snapshot, view: View::default() }
}

/// What a switch did.
#[derive(Clone, Debug, PartialEq)]
pub struct Switched {
    /// The view of the profile now active, already written to params.
    pub view: View,
    /// The profile was never visited and started empty.
    pub fresh: bool,
}

/// Switches the instance to profile `to` (0-based) with FL's inventory
/// from `bridge`: the state now and `view` go into the active profile,
/// `to`'s state is applied (the old parts' notes and Pong's ramps stop as
/// on a preset load) and its view written. `discard` drops the chat
/// field's pending lines right before the state changes; nothing changes
/// when `to`'s state is refused.
pub(crate) fn switch_on(writer: &Writer, view: &View, to: usize, bridge: &fl::Bridge, discard: &mut dyn FnMut()) -> Result<Switched, String> {
    if to >= COUNT {
        return Err(format!("no profile {}", to + 1));
    }
    let params = &writer.params;
    let mut all = Profiles::read(params);
    if to == all.active {
        return Ok(Switched { view: view.clone(), fresh: false });
    }
    let now = capture(writer);
    let waiting = all.slots.get_mut(to).and_then(Option::take);
    let fresh_one = waiting.is_none();
    let next = waiting.unwrap_or_else(|| fresh(&now));
    check(&next.snapshot)?;
    discard();
    apply_on(writer, &next.snapshot, bridge)?;
    if let Some(s) = all.slots.get_mut(all.active) {
        *s = Some(Slot { snapshot: now, view: view.clone() });
    }
    all.active = to;
    all.write(params);
    let new_view = next.view.with_content(&capture(writer));
    new_view.write(params);
    Ok(Switched { view: new_view, fresh: fresh_one })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nih_plug::prelude::Params;
    use parking_lot::Mutex;

    use super::super::tests::{line, topos, writer};
    use super::super::Section;
    use super::*;
    use crate::pong::arena::Side;
    use crate::pong::setup::{Action, Setup};
    use crate::saved;

    /// A detached bridge per call: no FL inventory, plugin ids stay.
    fn bridge() -> &'static fl::Bridge {
        fl::Bridge::detached()
    }

    fn go(w: &Writer, view: &View, to: usize) -> Switched {
        let r = switch_on(w, view, to, bridge(), &mut || {});
        assert!(r.is_ok(), "{r:?}");
        r.unwrap_or(Switched { view: View::default(), fresh: false })
    }

    /// Kept lines without their line and batch ids (apply renumbers them).
    fn alike(a: &Snapshot, b: &Snapshot) -> bool {
        let plain = |s: &Snapshot| s.routes.iter().map(|l| saved::Saved { line: 0, batch: 0, ..l.clone() }).collect::<Vec<_>>();
        plain(a) == plain(b) && a.midi == b.midi && a.pong == b.pong && a.pin == b.pin
    }

    /// Profile 1's setup: two kept lines, a kick on the top wall, a hat on
    /// the right pinch, Pong on, Topos 0/0 picked, F minor.
    fn setup_one(w: &Writer) -> View {
        let lines = vec![line(&topos(0, 0), 4), line(&topos(1, 2), 9)];
        *w.params.routes.write() = saved::store(&lines);
        saved::run(&lines, w);
        let mut store = midi::Store { key: midi::Key { root: 5, scale: midi::Scale::Minor }, ..midi::Store::default() };
        let kick = store.add("Kick", midi::Play::Note { pitch: 36, vel: 110 }, None);
        let hat = store.add("Hat", midi::Play::Note { pitch: 42, vel: 90 }, None);
        store.binds.push(midi::Bind { signal: "cam0.hand.R.pinch".into(), trigger: crate::route::Trigger::Above, part: hat });
        w.set_midi(store);
        let mut setup = Setup { on: true, max: 8, ..Setup::default() };
        setup.walls[Side::Top.index()] = Action::Part { id: kick };
        w.pong.set_setup(setup);
        *w.params.target_pin.write() = topos(0, 0).id();
        let mut view = View { sections: vec![Section::Hands, Section::Pong], preset: Some("Night set".into()), folded: Vec::new() };
        view.set_folded(Section::Pong, true);
        view.write(&w.params);
        view
    }

    /// Profile 2's setup: one kept line on Topos 3/1, a snare on the mouth,
    /// Topos 3/1 picked; Head opened first, then Hands by the line.
    fn setup_two(w: &Writer) -> View {
        let lines = vec![line(&topos(3, 1), 1)];
        *w.params.routes.write() = saved::store(&lines);
        saved::run(&lines, w);
        let mut store = w.player.store();
        let snare = store.add("Snare", midi::Play::Note { pitch: 38, vel: 100 }, None);
        store.binds.push(midi::Bind { signal: "cam0.mouth.open".into(), trigger: crate::route::Trigger::Above, part: snare });
        w.set_midi(store);
        *w.params.target_pin.write() = topos(3, 1).id();
        let view = View { sections: vec![Section::Head, Section::Hands], ..View::default() };
        view.write(&w.params);
        view
    }

    #[test]
    fn each_profile_round_trips_through_capture_and_apply() {
        let w = writer();
        let v1 = setup_one(&w);
        let one = capture(&w);
        let to2 = go(&w, &v1, 1);
        assert!(to2.fresh);
        let v2 = setup_two(&w);
        let two = capture(&w);
        let to3 = go(&w, &v2, 2);
        assert!(to3.fresh);
        let back1 = go(&w, &to3.view, 0);
        assert!(!back1.fresh);
        assert!(alike(&capture(&w), &one), "profile 1: {:?}", capture(&w));
        assert_eq!(back1.view, v1);
        assert_eq!(View::read(&w.params), v1);
        let back2 = go(&w, &back1.view, 1);
        assert!(alike(&capture(&w), &two), "profile 2: {:?}", capture(&w));
        assert_eq!(back2.view, v2);
        // every kept line runs again
        let got = capture(&w);
        assert!(got.routes.iter().all(|l| w.has_line(l.line)));
        assert_eq!(Profiles::read(&w.params).summary(), Summary { active: 1, filled: [true, true, true] });
    }

    #[test]
    fn an_unvisited_profile_is_empty_and_keeps_the_picked_plugin_and_the_key() {
        let w = writer();
        let v1 = setup_one(&w);
        let key = w.player.store().key;
        assert_eq!(Profiles::read(&w.params).summary(), Summary { active: 0, filled: [true, false, false] });
        let s = go(&w, &v1, 2);
        assert!(s.fresh);
        let got = capture(&w);
        assert!(got.routes.is_empty() && w.params.routes.read().is_empty());
        assert!(got.midi.parts.is_empty() && got.midi.binds.is_empty() && got.midi.walls.is_empty());
        assert_eq!(got.midi.key, key);
        assert!(!got.pong.on);
        assert_eq!(got.pong, Setup::default());
        assert_eq!(got.pin, Some(topos(0, 0).id()));
        assert_eq!(s.view, View::default());
        assert!(View::read(&w.params).sections.is_empty());
        assert!(!w.has_line(1) && w.max_line() == 0);
        assert_eq!(Profiles::read(&w.params).summary(), Summary { active: 2, filled: [true, false, true] });
    }

    #[test]
    fn switching_back_restores_exactly() {
        let w = writer();
        let v1 = setup_one(&w);
        let before = capture(&w);
        let routes_before = w.line_routes(4).len() + w.line_routes(9).len();
        let s = go(&w, &v1, 1);
        // the same profile again changes nothing
        let same = go(&w, &s.view, 1);
        assert!(!same.fresh && same.view == s.view);
        let back = go(&w, &s.view, 0);
        let after = capture(&w);
        assert!(alike(&after, &before), "{before:?}\n{after:?}");
        assert_eq!(back.view, v1);
        let ids: Vec<u64> = after.routes.iter().map(|l| l.line).collect();
        assert_eq!(ids.iter().map(|l| w.line_routes(*l).len()).sum::<usize>(), routes_before);
        // and again: a second round trip changes nothing more
        let s = go(&w, &back.view, 1);
        go(&w, &s.view, 0);
        assert_eq!(capture(&w), after);
    }

    #[test]
    fn a_refused_profile_changes_nothing_and_keeps_the_pending_lines() {
        let w = writer();
        let v1 = setup_one(&w);
        let before = capture(&w);
        // a hand-edited project: profile 2 holds two parts with one id
        let mut bad = Profiles::default();
        let mut snap = Snapshot::default();
        snap.midi.add("x", midi::Play::Note { pitch: 60, vel: 100 }, None);
        let twin = snap.midi.parts.clone();
        snap.midi.parts.extend(twin);
        bad.slots[1] = Some(Slot { snapshot: snap, view: View::default() });
        bad.write(&w.params);
        let mut discards = 0;
        let r = switch_on(&w, &v1, 1, bridge(), &mut || discards += 1);
        assert!(r.is_err());
        assert_eq!(discards, 0);
        assert_eq!(capture(&w), before);
        assert_eq!(Profiles::read(&w.params).active, 0);
        assert!(switch_on(&w, &v1, 3, bridge(), &mut || discards += 1).is_err());
        assert_eq!(discards, 0);
    }

    #[test]
    fn an_old_project_loads_on_profile_1_unchanged() {
        // a project saved before profiles: its state has no "profiles" field
        let w = writer();
        let v1 = setup_one(&w);
        let mut fields = w.params.serialize_fields();
        assert!(fields.contains_key("profiles"));
        fields.remove("profiles");
        let params = Arc::new(SynthexisParams::default());
        params.deserialize_fields(&fields);
        assert!(params.profiles.read().is_empty());
        for key in ["routes", "midi", "pong", "view", "target-pin"] {
            assert_eq!(params.serialize_fields().get(key), w.params.serialize_fields().get(key), "{key}");
        }
        let p = Profiles::read(&params);
        assert_eq!(p, Profiles::default());
        assert_eq!(p.summary(), Summary { active: 0, filled: [true, false, false] });
        assert_eq!(View::read(&params), v1);
        // a field written by hand reads with its defaults; a stray slot for
        // the active profile and an index out of range are dropped
        *params.profiles.write() = r#"{"active":1,"slots":[null,{"view":{"sections":["pong"]}},null]}"#.into();
        let p = Profiles::read(&params);
        assert_eq!((p.version, p.active, p.slots[1].clone()), (VERSION, 1, None));
        *params.profiles.write() = r#"{"active":7}"#.into();
        assert_eq!(Profiles::read(&params).active, 0);
        *params.profiles.write() = "{not json".into();
        assert_eq!(Profiles::read(&params), Profiles::default());
    }

    #[test]
    fn the_profiles_round_trip_through_the_project() {
        let w = writer();
        let v1 = setup_one(&w);
        let s = go(&w, &v1, 1);
        setup_two(&w);
        let now = capture(&w);
        let fields = w.params.serialize_fields();
        let params = Arc::new(SynthexisParams::default());
        params.deserialize_fields(&fields);
        assert_eq!(Profiles::read(&params), Profiles::read(&w.params));
        assert_eq!(Profiles::read(&params).summary(), Summary { active: 1, filled: [true, true, false] });
        let waiting = Profiles::read(&params).slots[0].clone().map(|s| s.view);
        assert_eq!(waiting, Some(v1));
        assert_ne!(s.view, View::read(&params));
        assert!(alike(&now, &capture(&w)));
    }

    /// Notes out, kept in order.
    #[derive(Default)]
    struct Rec(Mutex<Vec<(u32, u8, u8)>>);

    impl midi::Out for Rec {
        fn notes(&self, ev: &[(u32, u8, u8)]) {
            self.0.lock().extend_from_slice(ev);
        }
    }

    impl crate::pong::Sink for Rec {
        fn param(&self, _: &fl::Key, _: u32, _: f32) {}
    }

    /// (channel, pitch) pairs still sounding after `ev`.
    fn hanging(ev: &[(u32, u8, u8)]) -> Vec<(u32, u8)> {
        let mut on: Vec<(u32, u8)> = Vec::new();
        for (ch, pitch, vel) in ev {
            if *vel > 0 {
                on.push((*ch, *pitch));
            } else {
                on.retain(|n| *n != (*ch, *pitch));
            }
        }
        on
    }

    #[test]
    fn a_switch_leaves_no_note_hanging_and_no_shot_flying() {
        let rec = Arc::new(Rec::default());
        let w = Writer::with_parts(Arc::new(SynthexisParams::default()), midi::Player::manual(rec.clone()), rec.clone());
        let mut store = midi::Store::default();
        let pad = store.add("Pad", midi::Play::Note { pitch: 60, vel: 100 }, Some(midi::Channel { index: 0, name: "Pad".into() }));
        if let Some(p) = store.get_mut(pad) {
            (p.mode, p.start) = (midi::Mode::Latch, midi::Start::Now);
        }
        store.binds.push(midi::Bind { signal: "cam0.hand.R.pinch".into(), trigger: crate::route::Trigger::Above, part: pad });
        w.set_midi(store);
        w.pong.set_setup(Setup { on: true, max: 8, ..Setup::default() });
        // the pinch holds the pad, Pong shoots
        for (route, _, t) in w.line_routes(crate::writer::BIND_LINE) {
            if let crate::route::Target::Part { id, .. } = t {
                w.player.trigger(route, id, true);
            }
        }
        w.player.tick(1);
        w.pong.test_shots(4);
        assert_eq!(w.player.playing(), vec![pad]);
        assert_eq!(hanging(&rec.0.lock()), [(0, 60)]);
        let view = View { sections: vec![Section::Hands, Section::Pong], ..View::default() };
        let s = switch_on(&w, &view, 1, bridge(), &mut || {});
        assert!(s.is_ok());
        assert!(w.player.playing().is_empty(), "stuck after the switch");
        assert!(hanging(&rec.0.lock()).is_empty(), "{:?}", rec.0.lock());
        assert!(w.pong.view().shots.is_empty(), "shots of profile 1");
        assert!(!w.pong.setup().on);
        // back on profile 1 nothing plays until the pinch comes again
        let back = switch_on(&w, &View::default(), 0, bridge(), &mut || {});
        assert!(back.is_ok());
        assert!(w.player.playing().is_empty());
        assert!(hanging(&rec.0.lock()).is_empty());
        assert!(w.pong.setup().on);
    }
}
