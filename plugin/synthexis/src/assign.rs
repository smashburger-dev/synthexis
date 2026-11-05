//! Resolved routes (synthexis_parser::resolve) -> engine routes (route.rs).
//!
//! Source: `cam0.` + the source id, with L or R after its first segment for
//! a sided source (signal.rs); a sided source without a side gets no route
//! (Noa, 11-03: no default hand), "both" gets one per hand. Target: the
//! bound parameter of the picked plugin, or a Signal macro for
//! `T:macro:N`. Direction, range, amount, curve, smoothing and gates go
//! into the route; what the engine cannot run yet (steps, toggles, holds,
//! ramps, motions) is named in the note and left out. A MIDI target
//! (`T:midi.pattern`, `T:midi.chord`, `T:midi.note`) becomes a synthexis
//! part in the MIDI store and a route to it (`midi_routes`): a finger or a
//! shape triggers it while shown, any other movement while it reads high
//! (a fist and a closed eye with INV, see `triggers`). The MIDI store
//! binds one part per trigger, without gates or INV: a MIDI line with a
//! gate, another INV or a second part on one trigger says so in its note
//! and plays nothing of that. A route with MIDI and other targets is two
//! lines (`split_midi`).

use synthexis_parser::resolve;

use crate::cond::Curve;
use crate::fl;
use crate::i18n::{fill, Text, EN};
use crate::midi::{self, Channel, Play, Start, Store};
use crate::pong::setup::{Action, How, Mode};
use crate::route::{finger_bit, Route, Target, Trigger, NUM_MACROS};
use crate::signal::perception_ids;

/// Engine signal ids for a source and side; Err names why there is none,
/// in the language of `t`.
pub fn signals_in(source: &str, side: Option<&str>, t: &Text) -> Result<Vec<String>, String> {
    if source.is_empty() {
        return Err(t.n_which_move.into());
    }
    let ids: Vec<&str> = perception_ids().collect();
    let (head, rest) = source.split_once('.').unwrap_or((source, ""));
    let sided = |s: &str| format!("cam0.{head}.{s}.{rest}");
    if !rest.is_empty() && ids.contains(&sided("L").as_str()) {
        return match side {
            Some("left") => Ok(vec![sided("L")]),
            Some("right") => Ok(vec![sided("R")]),
            Some("both") => Ok(vec![sided("L"), sided("R")]),
            _ if head == "eye" => Err(t.n_which_eye.into()),
            _ => Err(t.n_which_hand.into()),
        };
    }
    let id = format!("cam0.{source}");
    if ids.contains(&id.as_str()) {
        Ok(vec![id])
    } else {
        Err(unmeasured(source, t))
    }
}

/// How a stand-in takes the route's side.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sides {
    /// The route's own side ("raise my left arm": the left hand).
    Same,
    /// Both hands (the orb between the palms).
    Both,
    /// A source without a side.
    Unsided,
}

/// Ontology sources the camera does not measure (tools/mapping/
/// ontology.yaml) with the live source that obviously stands in for each.
/// Every other unmeasured source gets three live movements to try
/// (`unmeasured`).
const STAND_INS: &[(&str, &str, Sides)] = &[
    ("arm.raise", "hand.y", Sides::Same),
    ("orb.size", "hands.distance", Sides::Unsided),
    ("orb.x", "hand.x", Sides::Both),
    ("orb.y", "hand.y", Sides::Both),
    ("motion.grab", "hand.fist", Sides::Same),
    ("motion.release", "hand.openness", Sides::Same),
];

/// The source and side a route reads: `source` itself, or the live
/// source standing in for it (STAND_INS) with its side, and true then.
pub fn live_source<'a>(source: &'a str, side: Option<&'a str>) -> (&'a str, Option<&'a str>, bool) {
    match STAND_INS.iter().find(|s| s.0 == source) {
        Some((_, live, Sides::Same)) => (live, side, true),
        Some((_, live, Sides::Both)) => (live, Some("both"), true),
        Some((_, live, Sides::Unsided)) => (live, None, true),
        None => (source, side, false),
    }
}

/// What a stand-in route reads, for the head of its line: "hand height
/// (both hands)", "hand height (left)", "distance between the hands";
/// None when `source` is measured itself.
pub fn stand_in_label(source: &str, side: Option<&str>, t: &Text) -> Option<String> {
    let (live, side, stood) = live_source(source, side);
    if !stood {
        return None;
    }
    let what = movement(live, t);
    Some(match side {
        Some("both") => format!("{what} ({})", t.r_both),
        Some("left") => format!("{what} ({})", t.r_left),
        Some("right") => format!("{what} ({})", t.r_right),
        _ => what.to_string(),
    })
}

/// A live movement in plain words, for the notes.
fn movement(source: &str, t: &Text) -> &'static str {
    match source {
        "hand.y" => t.mv_hand_y,
        "hand.x" => t.mv_hand_x,
        "hands.distance" => t.mv_hands_distance,
        "hand.fist" => t.mv_fist,
        "hand.openness" => t.mv_openness,
        "hand.pinch" => t.mv_pinch,
        "hand.roll" => t.mv_roll,
        "mouth.open" => t.mv_mouth,
        "head.roll" => t.mv_head_tilt,
        _ => t.mv_head_turn,
    }
}

/// The note for a source the camera does not measure: it names the
/// source and three live movements to try instead, by body part.
fn unmeasured(source: &str, t: &Text) -> String {
    let try_: [&str; 3] = match source.split('.').next() {
        Some("hand" | "motion" | "arm") => ["hand.y", "hand.pinch", "hand.openness"],
        Some("body" | "orb") => ["head.roll", "hand.x", "hands.distance"],
        _ => ["hand.y", "mouth.open", "head.yaw"],
    };
    let names = try_.map(|s| movement(s, t));
    let list = format!("{}, {} {} {}", names[0], names[1], t.n_or, names[2]);
    fill(t.n_unmeasured, &[&source.replace('.', " "), &list])
}

/// The note for a stand-in: the source the camera does not measure and
/// what it reads instead.
fn stand_in_note(source: &str, live: &str, t: &Text) -> String {
    fill(t.n_stand_in, &[&source.replace('.', " "), &movement(live, t)])
}

/// A sentence about the hands or arms in the plural ("when I raise my
/// hands", "beide Hände") that names no side means both hands: a sided
/// hand route of it (also a stand-in such as arm.raise) takes "both",
/// whatever side the parser guessed. True when it changed `r`.
pub fn side_from_text(r: &mut resolve::Route, text: &str) -> bool {
    let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let plural = words.iter().any(|w| ["hands", "arms", "both", "hände", "händen", "arme", "armen", "beide", "beiden"].contains(&w.as_str()));
    let sided = words.iter().any(|w| w == "left" || w == "right" || w.starts_with("link") || w.starts_with("recht"));
    let (live, _, _) = live_source(&r.source, None);
    if !plural || sided || !live.starts_with("hand.") || r.side.as_deref() == Some("both") {
        return false;
    }
    r.side = Some("both".into());
    true
}

/// The targets of a resolved route, whatever its source: a bound parameter
/// (of the chip's own plugin when the chip came from another one), or a
/// Signal macro for `T:macro:N`.
pub fn targets(r: &resolve::Route, plugin: Option<&fl::Key>) -> Vec<Target> {
    r.targets
        .iter()
        .filter_map(|t| match &t.bound {
            Some(b) => {
                let key = b.plugin.as_deref().and_then(fl::Key::parse).or_else(|| plugin.cloned())?;
                Some(Target::Param { plugin: key, index: b.index })
            }
            None => t
                .token
                .strip_prefix("T:macro:")
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| (1..=NUM_MACROS).contains(n))
                .map(|n| Target::Macro(n - 1)),
        })
        .collect()
}

/// The gate signals for a route whose source signal is `sig`. A gate's
/// side: its own, else the route's side when the gate is the same body part
/// as the source ("left hand y while I pinch": the left hand pinches), else
/// the sentence's side when the source has none ("with my right hand: mouth
/// opens the mix while I pinch"); an eye never lends its side to a hand.
/// Err when a gate cannot run, then the route must not run either.
fn gate_signals(r: &resolve::Route, sig: &str, t: &Text) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = sig.split('.').collect();
    let sided = parts.get(2).is_some_and(|h| *h == "L" || *h == "R");
    r.gates
        .iter()
        .map(|g| {
            let src = g[0].trim_start_matches("G:");
            let named = src.replace('.', " ");
            if g.iter().any(|t| t.starts_with("shape:")) {
                return Err(fill(t.n_gate_shape, &[&named]));
            }
            let lent = match (sided, parts.get(1), parts.get(2)) {
                (true, Some(body), Some(h)) if src.split('.').next() == Some(*body) => {
                    Some(if *h == "L" { "left" } else { "right" })
                }
                (false, ..) => r.side.as_deref(),
                _ => None,
            };
            let side = g.iter().find_map(|t| t.strip_prefix("side:")).or(lent);
            // "both hands pinch": both must be open
            let (live, side, _) = live_source(src, side);
            signals_in(live, side, t).map_err(|e| fill(t.n_gate, &[&named, &e]))
        })
        .collect::<Result<Vec<Vec<String>>, String>>()
        .map(|v| v.into_iter().flatten().collect())
}

/// What a resolved route becomes: engine routes and a note for the line
/// ("" when everything runs).
pub fn routes(r: &resolve::Route, plugin: Option<&fl::Key>) -> (Vec<Route>, String) {
    routes_in(r, plugin, &EN)
}

/// `routes` with the note in the language of `t`.
pub fn routes_in(r: &resolve::Route, plugin: Option<&fl::Key>, t: &Text) -> (Vec<Route>, String) {
    if r.source == "hand.shape" {
        // a finger state is a trigger: it plays MIDI parts (midi_routes)
        return (Vec::new(), t.n_shape_midi.into());
    }
    let (live, side, stood) = live_source(&r.source, r.side.as_deref());
    let sigs = match signals_in(live, side, t) {
        Ok(s) => s,
        Err(e) => return (Vec::new(), e),
    };
    let targets = targets(r, plugin);
    if targets.is_empty() {
        return (Vec::new(), t.n_no_param.into());
    }
    let mut notes: Vec<String> = Vec::new();
    if stood {
        notes.push(stand_in_note(&r.source, live, t));
    }
    let get = |p: &str| r.mods.iter().find_map(|m| m.strip_prefix(p)).and_then(|v| v.parse::<f32>().ok());
    let skipped: Vec<&str> = r
        .mods
        .iter()
        .filter(|m| ["mode:", "hold:", "quantize:", "delta:", "BIP"].iter().any(|p| m.starts_with(p)))
        .map(String::as_str)
        .collect();
    if !skipped.is_empty() {
        notes.push(fill(t.n_mods_not_live, &[&skipped.join(" ")]));
    }
    let mut out = Vec::new();
    for sig in &sigs {
        let gates = match gate_signals(r, sig, t) {
            Ok(g) => g,
            Err(e) => return (Vec::new(), e),
        };
        for t in &targets {
            // both hands are one line: route.rs averages them
            let mut route = Route::to(sig, t.clone(), get("amt:").unwrap_or(1.0));
            route.lo = get("lo:").unwrap_or(0.0);
            route.hi = get("hi:").unwrap_or(1.0);
            route.gates.clone_from(&gates);
            route.cond.range.invert = r.inv;
            route.cond.range.curve = match r.mods.iter().find_map(|m| m.strip_prefix("curve:")) {
                Some("exp") => Curve::Exp,
                Some("log") => Curve::Log,
                Some("s") => Curve::S,
                _ => Curve::Linear,
            };
            // smoothing: the One Euro filter's cutoff, higher is snappier
            route.cond.filter.min_cutoff *= match r.mods.iter().find_map(|m| m.strip_prefix("smooth:")) {
                Some("low") => 3.0,
                Some("high") => 1.0 / 3.0,
                _ => 1.0,
            };
            out.push(route);
        }
    }
    (out, notes.join(", "))
}

/// True for a target token a synthexis MIDI part plays.
fn is_part(token: &str) -> bool {
    ["T:midi.pattern", "T:midi.chord", "T:midi.note"].iter().any(|p| token.starts_with(p))
}

/// True when the route names a synthexis MIDI target.
pub fn is_midi(r: &resolve::Route) -> bool {
    r.targets.iter().any(|t| is_part(&t.token))
}

/// A route with MIDI and other targets ("head yaw plays chords and drives
/// the drive") as two: its MIDI targets, and the others without the MIDI
/// values (note:, chord:). None for a half without targets.
pub fn split_midi(r: &resolve::Route) -> (Option<resolve::Route>, Option<resolve::Route>) {
    let (midi, rest): (Vec<resolve::Target>, Vec<resolve::Target>) = r.targets.iter().cloned().partition(|t| is_part(&t.token));
    let plain: Vec<String> = r.mods.iter().filter(|m| !m.starts_with("note:") && !m.starts_with("chord:")).cloned().collect();
    let half = |targets: Vec<resolve::Target>, mods: Vec<String>| (!targets.is_empty()).then(|| resolve::Route { targets, mods, ..r.clone() });
    (half(midi, r.mods.clone()), half(rest, plain))
}

/// Shape name or mask (ontology "shapes"; mXXXXX thumb..pinky).
fn shape_mask(name: &str) -> Option<u8> {
    if let Some(bits) = name.strip_prefix('m').filter(|b| b.len() == 5 && b.chars().all(|c| c == '0' || c == '1')) {
        return u8::from_str_radix(bits, 2).ok();
    }
    // the ontology's id for the L, which SHAPES names "L"
    let n = if name == "l_shape" { "L".to_string() } else { name.replace('_', " ") };
    crate::fingers::SHAPES.iter().find(|s| s.1.eq_ignore_ascii_case(&n) || s.2.eq_ignore_ascii_case(&n)).map(|s| s.0)
}

/// What starts the part: (signal, trigger) per side the sentence names.
fn triggers(r: &resolve::Route, t: &Text) -> Result<Vec<(String, Trigger)>, String> {
    let get = |p: &str| r.mods.iter().find_map(|m| m.strip_prefix(p));
    let side = r.side.as_deref();
    let shape_sig = |s: &str| signals_in("hand.shape", s.into(), t);
    let fingers = ["thumb", "index", "middle", "ring", "pinky"];
    // a trigger has no inversion: INV takes the other side of a movement
    // the parser folds (pin.rs fold: a fist is the hand's openness
    // inverted, a closed eye the open one; the engine has both, fist = 1 -
    // openness), any other INV cannot run
    let source = match (r.inv, r.source.as_str()) {
        (true, "hand.openness") => "hand.fist",
        (true, "eye.open") => "eye.closed",
        (true, s) if !s.is_empty() => return Err(fill(t.n_not_live, &[&format!("{} {}", s.replace('.', " "), t.r_inverted)])),
        (_, s) => s,
    };
    if source == "hand.shape" {
        let mask = get("shape:").and_then(shape_mask).ok_or_else(|| t.n_which_move.to_string())?;
        let sigs = match side {
            Some(s) => shape_sig(s)?,
            None => return Err(t.n_which_hand.into()),
        };
        return Ok(sigs.into_iter().map(|s| (s, Trigger::Shape { mask, care: 31 })).collect());
    }
    if let Some(f) = source.strip_prefix("hand.finger.").and_then(|f| fingers.iter().position(|x| *x == f)) {
        // "my index finger plays ...": the finger shown, the others free
        let sigs = match side {
            Some(s) => shape_sig(s)?,
            None => return Err(t.n_which_hand.into()),
        };
        let b = finger_bit(f);
        return Ok(sigs.into_iter().map(|s| (s, Trigger::Shape { mask: b, care: b })).collect());
    }
    let (live, side, _) = live_source(source, side);
    Ok(signals_in(live, side, t)?.into_iter().map(|s| (s, Trigger::Above)).collect())
}

/// The part a MIDI target plays, named, with the channel of the part it
/// copies. A pattern name a part in the store has (painted in the finger
/// editor, offered by the completion) plays a copy of that part, a bound
/// one first; an empty name is the words that named the target, else the
/// hi-hat.
fn part_for(tg: &resolve::Target, r: &resolve::Route, store: &Store, finger: Option<usize>) -> (String, Play, Option<Channel>) {
    let get = |p: &str| r.mods.iter().find_map(|m| m.strip_prefix(p));
    if let Some(name) = tg.token.strip_prefix("T:midi.pattern:") {
        let named = |n: &str| {
            let n = n.trim();
            let bound = |id: u64| store.binds.iter().any(|b| b.part == id);
            let own = store.parts.iter().filter(|p| !n.is_empty() && p.name.eq_ignore_ascii_case(n)).max_by_key(|p| bound(p.id));
            own.map(|p| (p.name.clone(), p.play.clone(), p.channel.clone())).or_else(|| midi::template(n).map(|p| (n.to_string(), p, None)))
        };
        let empty = || (name.to_string(), Play::Clip { bars: 1, steps: Vec::new(), rows: Vec::new() }, None);
        return match name.trim() {
            "" => named(&tg.text).or_else(|| named("hi-hat basic")).unwrap_or_else(empty),
            n => named(n).unwrap_or_else(empty),
        };
    }
    let key = store.key;
    if tg.token.starts_with("T:midi.note") {
        let pitch = get("note:").and_then(midi::parse_note).unwrap_or(60);
        return (midi::note_name(pitch), Play::Note { pitch, vel: 100 }, None);
    }
    // a chord: a named one (Am, F, G7) or a degree, else one per finger
    let n = key.scale.steps().len() as u8;
    let named = get("chord:").and_then(|c| (0..n).find(|d| midi::chord_name(key, *d, 3).eq_ignore_ascii_case(c.trim()) || midi::degree_name(key, *d).eq_ignore_ascii_case(c.trim())));
    let d = named.unwrap_or_else(|| finger.map_or(0, |f| [0, 5, 2, 6, 4][f]));
    (midi::chord_name(key, d, 3), Play::Chords { degrees: vec![d], size: 3, octave: 4, vel: 96, strum: 0 }, None)
}

/// A new part in `store` for the MIDI target `tg` of `r`, named and played
/// as `part_for` says; returns its id. Its channel: the copied part's, else
/// the picked plugin's when it is a generator, else the channel of the other
/// parts on signal `sig`, else the first channel FL lists.
pub fn new_part(
    tg: &resolve::Target,
    r: &resolve::Route,
    store: &mut Store,
    plugin: Option<&fl::Key>,
    channels: &[fl::ChannelRow],
    sig: Option<&str>,
    finger: Option<usize>,
) -> u64 {
    let picked = plugin.and_then(|k| match k.at {
        fl::At::Gen { ch } => Some(Channel { index: ch, name: k.plugin.clone() }),
        fl::At::Fx { .. } => None,
    });
    let (name, mut play, copied) = part_for(tg, r, store, finger);
    // a copy keeps its notes on its own channel
    let fit = copied.is_none();
    let channel = copied
        .or(picked)
        .or_else(|| {
            let sig = sig?;
            store.binds.iter().filter(|b| b.signal == sig).find_map(|b| store.get(b.part).and_then(|p| p.channel.clone()))
        })
        .or_else(|| channels.first().map(|c| Channel { index: c.index, name: c.label() }));
    if let Some(c) = channel.as_ref().filter(|_| fit) {
        midi::fit_to_channel(&mut play, &c.name);
    }
    store.add(&name, play, channel)
}

/// The route a sentence said for a Pong side: its first sourceless route
/// with a target, else one made from the frame tokens outside the routes
/// (the parser reads "plays a trap roll" as a query with
/// `T:midi.pattern:trap roll` outside any route, and "spiel einen A-Moll
/// Akkord" as a key alone, which on a side is that key's chord). `rest` as
/// assistant::Reply has it (span values as "=text").
pub fn wall_route(routes: &[resolve::Route], rest: &[String]) -> Option<resolve::Route> {
    if let Some(r) = routes.iter().find(|r| r.source.is_empty() && !r.targets.is_empty()) {
        return Some(r.clone());
    }
    let mut mods = Vec::new();
    let mut targets = Vec::new();
    let mut it = rest.iter().peekable();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "CHORD" | "NOTE" => {
                if let Some(v) = it.peek().and_then(|v| v.strip_prefix('=')) {
                    mods.push(format!("{}:{v}", tok.to_lowercase()));
                    it.next();
                }
            }
            t if t.starts_with("T:") => {
                let text = t.splitn(3, ':').nth(2).unwrap_or("").to_string();
                targets.push(resolve::Target { token: t.to_string(), text, bound: None });
            }
            t if ["lo:", "hi:", "mode:"].iter().any(|p| t.starts_with(p)) => mods.push(t.to_string()),
            _ => {}
        }
    }
    if targets.is_empty() && rest.iter().any(|t| t.starts_with("key:")) {
        targets.push(resolve::Target { token: "T:midi.chord:".into(), text: String::new(), bound: None });
    }
    (!targets.is_empty()).then(|| resolve::Route { source: String::new(), side: None, inv: false, gates: Vec::new(), mods, targets })
}

/// What a wall sentence could not do, in words of the language shown:
/// the targets it left out, a part without an FL channel yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallNote {
    pub ignored: Vec<String>,
    pub no_channel: bool,
}

impl WallNote {
    pub fn text(&self, t: &Text) -> String {
        let mut notes = Vec::new();
        if !self.ignored.is_empty() {
            notes.push(fill(t.w_ignored, &[&self.ignored.join(", ")]));
        }
        if self.no_channel {
            notes.push(t.w_no_channel.to_string());
        }
        notes.join(", ")
    }
}

/// What a sourceless route makes a Pong side do: its first target (the
/// note names the others it leaves out). A parameter of the picked plugin
/// ramps from lo: to hi: (default 0.5 to 1.0, a cutoff at rest stays
/// audible), mode:toggle and a T:toggle target flip; `T:macro:N` drives
/// Signal N (default 0 to 1); a MIDI target becomes a new part in `store`
/// that plays at once, bound to no finger and listed in `store.walls`.
pub fn wall_action(
    route: &resolve::Route,
    plugin: Option<&fl::Key>,
    store: &mut Store,
    channels: &[fl::ChannelRow],
    t: &Text,
) -> Result<(Action, WallNote), String> {
    let Some(first) = route.targets.first() else { return Err(t.w_no_target.into()) };
    let named = |tg: &resolve::Target| tg.token.trim_start_matches("T:").replace(':', " ").trim().to_string();
    let mut note = WallNote { ignored: route.targets.iter().skip(1).map(named).collect(), no_channel: false };
    let get = |p: &str| route.mods.iter().find_map(|m| m.strip_prefix(p)).and_then(|v| v.parse::<f32>().ok());
    let toggle = route.mods.iter().any(|m| m == "mode:toggle") || first.token.starts_with("T:toggle");
    let how = |lo: f32| How {
        mode: if toggle { Mode::Toggle } else { Mode::Ramp },
        lo: get("lo:").unwrap_or(if toggle { 0.0 } else { lo }),
        hi: get("hi:").unwrap_or(1.0),
        ..How::default()
    };
    if is_part(&first.token) {
        let id = new_part(first, route, store, plugin, channels, None, None);
        if let Some(p) = store.get_mut(id) {
            p.start = Start::Now;
            note.no_channel = p.channel.is_none();
        }
        if !store.walls.contains(&id) {
            store.walls.push(id);
        }
        return Ok((Action::Part { id }, note));
    }
    if let Some(n) = first.token.strip_prefix("T:macro:").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=NUM_MACROS).contains(n)) {
        return Ok((Action::Macro { index: n - 1, how: how(0.0) }, note));
    }
    match (&first.bound, first.token.starts_with("T:param") || first.token.starts_with("T:toggle")) {
        (Some(b), _) => {
            let key = b.plugin.as_deref().and_then(fl::Key::parse).or_else(|| plugin.cloned()).ok_or_else(|| t.n_no_param.to_string())?;
            Ok((Action::Param { key: key.id(), index: b.index, name: b.name.clone(), how: how(0.5) }, note))
        }
        (None, true) => Err(t.n_no_param.into()),
        (None, false) => Err(fill(t.n_not_live, &[&named(first)])),
    }
}

/// A MIDI line: its parts go into `store` (bound only at Keep), its
/// routes start them. None when the route has no MIDI target. The channel
/// is the copied part's (see `part_for`), else the picked plugin's when it
/// is a generator, else the hand's other parts', else the first channel FL
/// lists.
pub fn midi_routes(r: &resolve::Route, plugin: Option<&fl::Key>, store: &mut Store, channels: &[fl::ChannelRow], t: &Text) -> Option<(Vec<Route>, String, Vec<u64>)> {
    if !is_midi(r) {
        return None;
    }
    let refuse = |note: String| Some((Vec::new(), note, Vec::new()));
    if let Some(g) = r.gates.first() {
        // a bind in the MIDI store has no gates: no part, not an ungated one
        return refuse(fill(t.n_gate, &[&g[0].trim_start_matches("G:").replace('.', " "), &fill(t.n_mods_not_live, &[&"MIDI"])]));
    }
    let trig = match triggers(r, t) {
        Ok(x) => x,
        Err(e) => return refuse(e),
    };
    let finger = r.source.strip_prefix("hand.finger.").and_then(|f| ["thumb", "index", "middle", "ring", "pinky"].iter().position(|x| *x == f));
    // one part per trigger, as the store binds it: the first target, a
    // repeated one once
    let mut tokens: Vec<&resolve::Target> = Vec::new();
    for tg in r.targets.iter().filter(|x| is_part(&x.token)) {
        if !tokens.iter().any(|x| x.token == tg.token) {
            tokens.push(tg);
        }
    }
    let mut notes = Vec::new();
    if let (live, _, true) = live_source(&r.source, None) {
        notes.push(stand_in_note(&r.source, live, t));
    }
    if tokens.len() > 1 {
        let named: Vec<String> = tokens.iter().map(|x| x.token.trim_start_matches("T:").replace(':', " ").trim().to_string()).collect();
        notes.push(fill(t.n_not_live, &[&named.join(" + ")]));
        tokens.truncate(1);
    }
    let mut routes = Vec::new();
    let mut made = Vec::new();
    for tg in tokens {
        for (sig, trigger) in &trig {
            let id = new_part(tg, r, store, plugin, channels, Some(sig), finger);
            made.push(id);
            routes.push(Route::to(sig, Target::Part { id, trigger: *trigger }, 1.0));
        }
    }
    if routes.iter().any(|x| matches!(&x.target, Target::Part { id, .. } if store.get(*id).is_some_and(|p| p.channel.is_none()))) {
        notes.push(t.n_no_channel.to_string());
    }
    Some((routes, notes.join(", "), made))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn resolved(source: &str, side: Option<&str>, mods: &[&str], gates: Vec<Vec<String>>, bound: bool) -> resolve::Route {
        resolve::Route {
            source: source.into(),
            side: side.map(str::to_string),
            inv: true,
            gates,
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![resolve::Target {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: bound.then(|| resolve::Bound { index: 12, name: "Amp Drive".into(), via: resolve::Via::Name, plugin: None }),
            }],
        }
    }

    #[test]
    fn left_fist_drives_topos() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let gate = vec![vec!["G:hand.pinch".to_string(), "side:right".to_string()]];
        let (rs, note) = routes(&resolved("hand.openness", Some("left"), &["hi:0.8", "smooth:low"], gate, true), Some(&key));
        assert_eq!(note, "");
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].signal, "cam0.hand.L.openness");
        assert_eq!(rs[0].target, Target::Param { plugin: key, index: 12 });
        assert!(rs[0].cond.range.invert);
        assert_eq!((rs[0].lo, rs[0].hi), (0.0, 0.8));
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch"]);
    }

    /// With two Serum 2 in the project a route binds to the instance the
    /// picker shows, or to the one its chip came from, never to the other
    /// one of that name.
    #[test]
    fn a_route_binds_to_the_picked_instance_of_twins() {
        let serum = |ch| fl::Key { at: fl::At::Gen { ch }, plugin: "Serum 2".into() };
        let r = resolved("hand.x", Some("right"), &[], Vec::new(), true);
        let (rs, _) = routes(&r, Some(&serum(9)));
        assert_eq!(rs.iter().map(|x| x.target.clone()).collect::<Vec<_>>(), [Target::Param { plugin: serum(9), index: 12 }]);
        let mut chip = r.clone();
        if let Some(b) = chip.targets[0].bound.as_mut() {
            b.plugin = Some(serum(2).id());
        }
        assert_eq!(targets(&chip, Some(&serum(9))), [Target::Param { plugin: serum(2), index: 12 }]);
    }

    #[test]
    fn no_side_no_route_and_unsided_sources_work() {
        let (rs, note) = routes(&resolved("hand.openness", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        let (rs, _) = routes(&resolved("hand.y", Some("both"), &["hi:0.8"], vec![], true), Some(&fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() }));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y", "cam0.hand.R.y"]);
        assert!(rs.iter().all(|r| r.depth == 1.0 && (r.hi - 0.8).abs() < 1e-6));
        let (rs, note) = routes(&resolved("mouth.open", None, &["mode:step"], vec![], false), None);
        assert!(rs.is_empty() && note == "no parameter to drive", "{note}");
        let mut macro_route = resolved("mouth.open", None, &["mode:step"], vec![], false);
        macro_route.targets[0].token = "T:macro:3".into();
        let (rs, note) = routes(&macro_route, None);
        assert_eq!(rs[0].target, Target::Macro(2));
        assert_eq!(note, "mode:step not live yet");
        let (rs, note) = routes(&resolved("motion.circle", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note == "the camera does not measure motion circle yet; try hand height, pinch or hand openness", "{note}");
        let (_, note) = routes(&resolved("", None, &[], vec![], true), None);
        assert_eq!(note, "which movement?");
        let (_, note) = routes(&resolved("eye.open", None, &[], vec![], true), None);
        assert!(note.starts_with("which eye?"), "{note}");
    }

    /// Noa's FL sentence (2026-11-04): "when i rise my hands up i want the
    /// filter" parsed as S:arm.raise side:left on Serum 2's Filter 1 On.
    /// The camera does not measure the arm: hand height stands in, on both
    /// hands since the sentence says "hands", and the note says so.
    #[test]
    fn an_unmeasured_source_falls_back_to_the_obvious_live_one() {
        let serum = serum();
        let mut raise = resolved("arm.raise", Some("left"), &[], vec![], true);
        raise.inv = false;
        let (rs, note) = routes(&raise, Some(&serum));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y"]);
        assert_eq!(note, "the camera does not measure arm raise, so this uses hand height");
        assert_eq!(stand_in_label("arm.raise", Some("both"), &EN).as_deref(), Some("hand height (both hands)"));
        assert_eq!(stand_in_label("orb.size", None, &EN).as_deref(), Some("distance between the hands"));
        assert_eq!(stand_in_label("hand.y", Some("left"), &EN), None);
        let (_, de) = routes_in(&raise, Some(&serum), &crate::i18n::DE);
        assert_eq!(de, "die Kamera misst arm raise nicht, deshalb wird Handhöhe genutzt");
        assert!(side_from_text(&mut raise, "when i rise my hands up i want the filter"));
        assert_eq!(raise.side.as_deref(), Some("both"));
        let (rs, _) = routes(&raise, Some(&serum));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y", "cam0.hand.R.y"]);
        // a side in the words, or one arm: the parser's side stays
        let mut left = resolved("arm.raise", Some("left"), &[], vec![], true);
        assert!(!side_from_text(&mut left, "raise my left arm and both hands"));
        assert!(!side_from_text(&mut left, "when I raise my arm"));
        assert!(!side_from_text(&mut resolved("mouth.open", None, &[], vec![], true), "open my mouth with both hands"));
        let mut german = resolved("arm.raise", None, &[], vec![], true);
        assert!(side_from_text(&mut german, "wenn ich die Hände hebe"));
        // no side, singular: which hand, as for the hand itself
        let (rs, note) = routes(&resolved("arm.raise", None, &[], vec![], true), Some(&serum));
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        // the orb between the palms: their distance, both hands' height
        let (rs, note) = routes(&resolved("orb.size", None, &[], vec![], true), Some(&serum));
        assert_eq!((rs.len(), rs.first().map(|r| r.signal.as_str())), (1, Some("cam0.hands.distance")));
        assert!(note.contains("distance between the hands"), "{note}");
        let (rs, _) = routes(&resolved("orb.y", None, &[], vec![], true), Some(&serum));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y", "cam0.hand.R.y"]);
        // a grab plays like a fist, with the stand-in named
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        let (rs, note, _) = midi_routes(&midi("motion.grab", Some("right"), &[], "T:midi.pattern:kick"), None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(rs.first().map(|r| r.signal.as_str()), Some("cam0.hand.R.fist"));
        assert!(note.contains("motion grab") && note.contains("fist"), "{note}");
        // a gate on a stand-in reads it too
        let gate = vec![vec!["G:arm.raise".to_string(), "side:right".to_string()]];
        let (rs, _) = routes(&resolved("mouth.open", None, &[], gate, true), Some(&serum));
        assert_eq!(rs.first().map(|r| r.gates.clone()), Some(vec!["cam0.hand.R.y".to_string()]));
    }

    /// Without an obvious stand-in the note names the source and three
    /// live movements to try, never a bare "not live yet".
    #[test]
    fn an_unmeasured_source_without_a_stand_in_lists_three_to_try() {
        let (rs, note) = routes(&resolved("heart.bpm", None, &[], vec![], true), Some(&serum()));
        assert!(rs.is_empty());
        assert_eq!(note, "the camera does not measure heart bpm yet; try hand height, mouth open or head turn");
        let (_, note) = routes(&resolved("body.lean", None, &[], vec![], true), Some(&serum()));
        assert_eq!(note, "the camera does not measure body lean yet; try head tilt, hand left-right or distance between the hands");
        let (_, de) = routes_in(&resolved("hand.pitch", Some("left"), &[], vec![], true), Some(&serum()), &crate::i18n::DE);
        assert_eq!(de, "die Kamera misst hand pitch noch nicht; probier Handhöhe, Pinch oder Handöffnung");
    }

    /// Every ontology source is measured, has a stand-in that is, or gets
    /// the note with three live movements; the movements named are live.
    #[test]
    fn every_ontology_source_runs_stands_in_or_says_what_to_try() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/mapping/ontology.yaml");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut in_sources = false;
        let mut sources = Vec::new();
        for line in text.lines() {
            if !line.starts_with(' ') && !line.starts_with('#') && !line.is_empty() {
                in_sources = line.starts_with("sources:");
            }
            if let Some(rest) = line.trim_start().strip_prefix("- {id: ").filter(|_| in_sources) {
                sources.push(rest.chars().take_while(|c| *c != ',' && *c != '}').collect::<String>().trim().to_string());
            }
        }
        assert!(sources.len() > 30, "ontology not read ({})", sources.len());
        let live = |s: &str| signals_in(s, Some("left"), &EN).is_ok();
        let mut stand_ins = 0;
        for src in sources.iter().filter(|s| s.as_str() != "hand.shape") {
            let (to, side, stood) = live_source(src, Some("left"));
            if stood {
                stand_ins += 1;
                assert!(signals_in(to, side, &EN).is_ok(), "{src} -> {to} is not live");
            } else if !live(src) {
                let note = signals_in(src, Some("left"), &EN).err().unwrap_or_default();
                assert!(note.starts_with("the camera does not measure") && note.matches(", ").count() == 1 && note.contains(" or "), "{src}: {note}");
            }
        }
        assert_eq!(stand_ins, STAND_INS.len(), "every stand-in is an ontology source");
        for s in ["hand.y", "hand.x", "hands.distance", "hand.fist", "hand.openness", "hand.pinch", "hand.roll", "mouth.open", "head.roll", "head.yaw"] {
            assert!(live(s), "{s} is named as a movement to try");
        }
    }

    #[test]
    fn gates_take_the_routes_hand_and_never_run_open() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        // two gates: both must hold
        let gates = vec![vec!["G:hand.pinch".to_string(), "side:right".to_string()], vec!["G:eye.open".to_string(), "side:left".to_string()]];
        let (rs, _) = routes(&resolved("hand.openness", Some("left"), &[], gates, true), Some(&key));
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch", "cam0.eye.L.open"]);
        // a sided gate without a side takes the route's hand
        let gates = vec![vec!["G:hand.pinch".to_string()]];
        let (rs, _) = routes(&resolved("hand.y", Some("both"), &[], gates.clone(), true), Some(&key));
        assert_eq!(rs.iter().map(|r| r.gates[0].as_str()).collect::<Vec<_>>(), ["cam0.hand.L.pinch", "cam0.hand.R.pinch"]);
        // an unsided source has no hand to lend: no route, not an ungated one
        let (rs, note) = routes(&resolved("mouth.open", None, &[], gates, true), Some(&key));
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        // the sentence's side for an unsided source; an eye lends no hand
        let mut mouth = resolved("mouth.open", None, &[], vec![vec!["G:hand.pinch".to_string()]], true);
        mouth.side = Some("right".into());
        let (rs, _) = routes(&mouth, Some(&key));
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch"]);
        let (rs, note) = routes(&resolved("eye.open", Some("left"), &[], vec![vec!["G:hand.pinch".to_string()]], true), Some(&key));
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        let shape = vec![vec!["G:hand.shape".to_string(), "shape:fist".to_string()]];
        let (rs, note) = routes(&resolved("mouth.open", None, &[], shape, true), Some(&key));
        assert!(rs.is_empty() && note.contains("not live yet"), "{note}");
    }

    fn midi(source: &str, side: Option<&str>, mods: &[&str], token: &str) -> resolve::Route {
        resolve::Route {
            source: source.into(),
            side: side.map(str::to_string),
            inv: false,
            gates: vec![],
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![resolve::Target { token: token.into(), text: String::new(), bound: None }],
        }
    }

    #[test]
    fn midi_targets_become_parts_with_a_trigger() {
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        // "thumbs up plays the hi-hat" with the right hand
        let r = midi("hand.shape", Some("right"), &["shape:thumbs_up"], "T:midi.pattern:hi-hat pattern");
        let (rs, note, made) = midi_routes(&r, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(note, "");
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].signal, "cam0.hand.R.shape");
        assert_eq!(rs[0].target, Target::Part { id: made[0], trigger: Trigger::Shape { mask: 0b10000, care: 31 } });
        let part = store.get(made[0]).cloned();
        assert!(matches!(part.as_ref().map(|p| &p.play), Some(Play::Clip { .. })));
        assert_eq!(part.and_then(|p| p.channel).map(|c| c.index), Some(2));
        // "left index plays Am" on Serum 2 (the picked generator), A minor
        let serum = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let r = midi("hand.finger.index", Some("left"), &["chord:Am"], "T:midi.chord:");
        let (rs, _, made) = midi_routes(&r, Some(&serum), &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(rs[0].target, Target::Part { id: made[0], trigger: Trigger::Shape { mask: 0b01000, care: 0b01000 } });
        let p = store.get(made[0]).cloned();
        assert_eq!(p.as_ref().map(|p| p.name.as_str()), Some("Am"));
        assert_eq!(p.and_then(|p| p.channel).map(|c| c.index), Some(1));
        // a note in FL's naming; no side: no part
        let r = midi("hand.pinch", Some("right"), &["note:g2"], "T:midi.note:");
        let (_, _, made) = midi_routes(&r, None, &mut store, &chans, &EN).unwrap_or_default();
        assert!(matches!(store.get(made[0]).map(|p| &p.play), Some(Play::Note { pitch: 31, .. })));
        let r = midi("hand.shape", None, &["shape:peace"], "T:midi.chord:");
        let (rs, note, _) = midi_routes(&r, None, &mut store, &chans, &EN).unwrap_or_default();
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        // a shape never drives a parameter
        let (rs, note) = routes(&midi("hand.shape", Some("left"), &["shape:fist"], "T:param:fx.drive"), None);
        assert!(rs.is_empty() && !note.is_empty());
        assert!(midi_routes(&midi("hand.y", Some("left"), &[], "T:param:fx.drive"), None, &mut store, &chans, &EN).is_none());
    }

    /// A bind has no gate and a trigger no inversion: a gated line makes no
    /// part, INV takes the fist or the closed eye, any other INV says so.
    #[test]
    fn midi_gates_and_inv_never_play_the_wrong_way() {
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        let mut gated = midi("hand.pinch", Some("right"), &[], "T:midi.pattern:hi-hat");
        gated.gates = vec![vec!["G:hand.pinch".into(), "side:left".into()]];
        let (rs, note, made) = midi_routes(&gated, None, &mut store, &chans, &EN).unwrap_or_default();
        assert!(rs.is_empty() && made.is_empty() && store.parts.is_empty(), "{rs:?}");
        assert_eq!(note, "gate hand pinch: MIDI not live yet");
        // "left fist plays the kick": the parser's hand openness with INV
        let mut fist = midi("hand.openness", Some("left"), &[], "T:midi.pattern:kick");
        fist.inv = true;
        let (rs, _, made) = midi_routes(&fist, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(rs[0].signal, "cam0.hand.L.fist");
        assert_eq!(rs[0].target, Target::Part { id: made[0], trigger: Trigger::Above });
        let mut blink = midi("eye.open", Some("right"), &[], "T:midi.pattern:clap");
        blink.inv = true;
        let (rs, _, _) = midi_routes(&blink, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(rs[0].signal, "cam0.eye.R.closed");
        let mut frown = midi("face.smile", None, &[], "T:midi.chord:");
        frown.inv = true;
        let n = store.parts.len();
        let (rs, note, _) = midi_routes(&frown, None, &mut store, &chans, &EN).unwrap_or_default();
        assert!(rs.is_empty() && store.parts.len() == n);
        assert_eq!(note, "face smile inverted is not live yet");
    }

    /// The store binds one part per trigger: a second MIDI target of the
    /// line goes with a note, a repeated one counts once.
    #[test]
    fn one_part_per_trigger() {
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        let mut r = midi("head.yaw", None, &["note:g2"], "T:midi.chord:");
        r.targets.push(resolve::Target { token: "T:midi.note:".into(), text: String::new(), bound: None });
        let (rs, note, made) = midi_routes(&r, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!((rs.len(), made.len()), (1, 1));
        assert!(matches!(store.get(made[0]).map(|p| &p.play), Some(Play::Chords { .. })));
        assert_eq!(note, "midi.chord + midi.note is not live yet");
        let mut r = midi("eye.open", Some("right"), &[], "T:midi.pattern:clap pattern");
        r.targets.push(r.targets[0].clone());
        let (rs, note, _) = midi_routes(&r, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!((rs.len(), note.as_str()), (1, ""));
    }

    #[test]
    fn every_ontology_shape_has_its_mask() {
        // tools/mapping/ontology.yaml "shapes"
        for (id, mask) in [
            ("fist", 0b00000), ("open", 0b11111), ("point", 0b01000), ("thumbs_up", 0b10000), ("peace", 0b01100),
            ("three", 0b01110), ("four", 0b01111), ("rock", 0b01001), ("shaka", 0b10001), ("l_shape", 0b11000),
            ("pinky_up", 0b00001), ("thumb_index_middle", 0b11100),
        ] {
            assert_eq!(shape_mask(id), Some(mask), "{id}");
        }
        assert_eq!(shape_mask("m10101"), Some(0b10101));
    }

    /// A pattern name a part in the store has plays a copy of it on its
    /// channel; an empty name takes the words that named the target.
    #[test]
    fn a_pattern_name_of_the_store_plays_that_part() {
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        let painted = Play::Clip { bars: 1, steps: vec![midi::Step { at: 3, len: 1, pitch: 38, vel: 90, ratchet: 1, nudge: 0 }], rows: vec![38] };
        let own = store.add("Funky Groove", painted.clone(), Some(Channel { index: 4, name: "Drums".into() }));
        store.bind("cam0.hand.R.shape", Trigger::Shape { mask: 0, care: 31 }, own);
        let (_, _, made) = midi_routes(&midi("hand.pinch", Some("left"), &[], "T:midi.pattern:funky groove"), None, &mut store, &chans, &EN).unwrap_or_default();
        let part = store.get(made[0]).cloned();
        assert_ne!(made[0], own);
        assert_eq!(part.as_ref().map(|p| (p.name.as_str(), &p.play)), Some(("Funky Groove", &painted)));
        assert_eq!(part.and_then(|p| p.channel).map(|c| c.index), Some(4));
        let mut chip = midi("hand.pinch", Some("left"), &[], "T:midi.pattern:");
        chip.targets[0].text = "funky groove".into();
        let (_, _, made) = midi_routes(&chip, None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(store.get(made[0]).map(|p| &p.play), Some(&painted));
        // no name and no words: the hi-hat
        let (_, _, made) = midi_routes(&midi("hand.pinch", Some("left"), &[], "T:midi.pattern:"), None, &mut store, &chans, &EN).unwrap_or_default();
        assert_eq!(store.get(made[0]).map(|p| p.name.as_str()), Some("hi-hat basic"));
    }

    #[test]
    fn a_route_with_midi_and_a_parameter_is_two() {
        let mut r = midi("head.yaw", None, &["note:g2", "hi:0.8"], "T:midi.note:");
        r.targets.push(resolved("head.yaw", None, &[], vec![], true).targets[0].clone());
        let (m, rest) = split_midi(&r);
        let m = m.unwrap_or_else(|| midi("", None, &[], ""));
        let rest = rest.unwrap_or_else(|| midi("", None, &[], ""));
        assert_eq!(m.targets.iter().map(|t| t.token.as_str()).collect::<Vec<_>>(), ["T:midi.note:"]);
        assert_eq!(m.mods, ["note:g2", "hi:0.8"]);
        assert_eq!(rest.targets.iter().map(|t| t.token.as_str()).collect::<Vec<_>>(), ["T:param:fx.drive"]);
        assert_eq!(rest.mods, ["hi:0.8"]);
        let (rs, _) = routes(&rest, Some(&fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() }));
        assert_eq!(rs.len(), 1);
        assert!(split_midi(&midi("head.yaw", None, &[], "T:midi.note:")).1.is_none());
    }

    /// The replies the shipped parser (v11 ensemble, 2026-11-04) gives for
    /// sourceless sentences on Serum 2 with Filter 1 Freq and Filter 1 Res:
    /// (sentence, intent, rest, routes). With SYNTHEXIS_PARSER_LIVE=1 the
    /// test parses them again and checks the parser still says this.
    pub(crate) fn sourceless_replies() -> Vec<(&'static str, &'static str, Vec<String>, Vec<resolve::Route>)> {
        let route = |mods: &[&str], target: resolve::Target| resolve::Route {
            source: String::new(),
            side: None,
            inv: false,
            gates: Vec::new(),
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![target],
        };
        let cutoff = resolve::Target {
            token: "T:param:filter.cutoff".into(),
            text: "cutoff".into(),
            bound: Some(resolve::Bound { index: 0, name: "Filter 1 Freq".into(), via: resolve::Via::Class, plugin: None }),
        };
        let strs = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        vec![
            ("cutoff from 30 to 80 percent", "modify", Vec::new(), vec![route(&["lo:0.3", "hi:0.8"], cutoff)]),
            ("spiel einen A-Moll Akkord", "scale", strs(&["key:A", "scale:minor"]), Vec::new()),
            ("plays a trap roll", "query", strs(&["T:midi.pattern:trap roll", "scope:target"]), Vec::new()),
        ]
    }

    fn serum() -> fl::Key {
        fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() }
    }

    #[test]
    fn real_parser_output_assigns_a_wall() {
        let replies = sourceless_replies();
        if std::env::var_os("SYNTHEXIS_PARSER_LIVE").is_some() {
            let parser = crate::assistant::load();
            assert!(parser.is_some(), "SYNTHEXIS_PARSER_LIVE set but the parser did not load");
            let params = vec![
                synthexis_complete::Param { index: 0, name: "Filter 1 Freq".into(), class: "filter.cutoff".into() },
                synthexis_complete::Param { index: 1, name: "Filter 1 Res".into(), class: "filter.resonance".into() },
            ];
            let scope = crate::assistant::Scope { key: serum().id(), plugin: "Serum 2".into(), params: std::sync::Arc::new(params), aliases: Default::default() };
            for (text, intent, rest, routes) in &replies {
                let Some(p) = parser.as_ref() else { break };
                let live = crate::assistant::parse(p, text, &[], Some(&scope));
                assert_eq!((live.intent.as_str(), &live.rest, &live.routes), (*intent, rest, routes), "{text}");
            }
        }
        let chans = vec![fl::ChannelRow { index: 2, name: "FPC".into(), plugin: "FPC".into() }];
        let mut store = Store::default();
        let walls: Vec<Result<(Action, WallNote), String>> = replies
            .iter()
            .map(|(_, _, rest, routes)| {
                // the field sets the key a sentence names before the wall (sentence.rs apply)
                if let Some(k) = midi::key_from(rest, store.key) {
                    store.key = k;
                }
                let route = wall_route(routes, rest).ok_or_else(|| "no wall route".to_string())?;
                wall_action(&route, Some(&serum()), &mut store, &chans, &EN)
            })
            .collect();
        // "cutoff from 30 to 80 percent": the cutoff ramps from 30 to 80 %
        let how = How { lo: 0.3, hi: 0.8, ..How::default() };
        assert_eq!(walls[0], Ok((Action::Param { key: "gen:1:Serum 2".into(), index: 0, name: "Filter 1 Freq".into(), how }, WallNote::default())));
        // "spiel einen A-Moll Akkord": the A minor chord, on the picked generator
        let part_id = |w: &Result<(Action, WallNote), String>| match w {
            Ok((Action::Part { id }, _)) => *id,
            _ => 0,
        };
        let am = part_id(&walls[1]);
        assert_ne!(am, 0, "{:?}", walls[1]);
        let part = store.get(am).cloned();
        assert_eq!(part.as_ref().map(|p| (p.name.as_str(), p.start)), Some(("Am", Start::Now)));
        assert!(matches!(part.as_ref().map(|p| &p.play), Some(Play::Chords { degrees, .. }) if degrees == &[0]));
        assert_eq!(part.and_then(|p| p.channel).map(|c| c.index), Some(1));
        // "plays a trap roll": the template, played at once, bound to no finger
        let roll = part_id(&walls[2]);
        assert_ne!(roll, 0, "{:?}", walls[2]);
        let part = store.get(roll).cloned();
        assert_eq!(part.as_ref().map(|p| (p.name.as_str(), p.start)), Some(("trap roll", Start::Now)));
        assert!(matches!(part.map(|p| p.play), Some(Play::Clip { steps, .. }) if !steps.is_empty()));
        assert_eq!(store.walls, [am, roll]);
        assert!(store.binds.is_empty());
    }

    /// Defaults (a ramp from half to full), toggles, Signals, a second
    /// target left out with a note, and what cannot be a wall.
    #[test]
    fn wall_actions_take_the_first_target_and_say_what_they_cannot() {
        let mut store = Store::default();
        let sourceless = |mods: &[&str], token: &str, bound: bool| {
            let mut r = resolved("", None, mods, Vec::new(), bound);
            r.inv = false;
            r.targets[0].token = token.into();
            r
        };
        let act = |r: &resolve::Route, store: &mut Store| wall_action(r, Some(&serum()), store, &[], &EN);
        let rest = |a: Action| match a {
            Action::Param { how, .. } | Action::Macro { how, .. } => Some((how.mode, how.lo, how.hi)),
            _ => None,
        };
        let r = sourceless(&[], "T:param:fx.drive", true);
        assert_eq!(act(&r, &mut store).ok().and_then(|a| rest(a.0)), Some((Mode::Ramp, 0.5, 1.0)));
        let r = sourceless(&["mode:toggle"], "T:param:fx.drive", true);
        assert_eq!(act(&r, &mut store).ok().and_then(|a| rest(a.0)), Some((Mode::Toggle, 0.0, 1.0)));
        let r = sourceless(&["hi:0.6"], "T:macro:3", false);
        assert_eq!(act(&r, &mut store).map(|a| a.0), Ok(Action::Macro { index: 2, how: How { hi: 0.6, ..How::default() } }));
        // two targets: the first, the note names the other
        let mut two = sourceless(&[], "T:macro:3", false);
        two.targets.push(resolve::Target { token: "T:midi.note:".into(), text: String::new(), bound: None });
        assert_eq!(act(&two, &mut store).map(|a| a.1.text(&EN)), Ok("midi.note left out, a wall does one thing".to_string()));
        assert!(store.parts.is_empty());
        assert_eq!(act(&sourceless(&[], "T:param:fx.drive", false), &mut store), Err("no parameter to drive".to_string()));
        assert_eq!(act(&sourceless(&[], "T:midi.cc:74", false), &mut store), Err("midi.cc 74 is not live yet".to_string()));
        let mut none = sourceless(&[], "T:macro:3", false);
        none.targets.clear();
        assert_eq!(act(&none, &mut store), Err("say what the wall does".to_string()));
        // no plugin picked: a parameter has nowhere to go
        assert!(wall_action(&sourceless(&[], "T:param:fx.drive", true), None, &mut store, &[], &EN).is_err());
        // a MIDI part without any channel says so
        let (a, note) = wall_action(&sourceless(&["note:c5"], "T:midi.note:", false), None, &mut store, &[], &EN).unwrap_or_default();
        let note = note.text(&EN);
        assert!(matches!(a, Action::Part { .. }) && note.contains("channel"), "{note}");
        assert!(!note.contains(EN.f_title), "a wall's channel is no finger's: {note}");
    }

    #[test]
    fn wall_route_takes_a_sourceless_route_or_the_tokens_outside() {
        let sourced = resolved("hand.y", Some("left"), &[], Vec::new(), true);
        let mut free = resolved("", None, &["hi:0.8"], Vec::new(), true);
        free.inv = false;
        assert_eq!(wall_route(&[sourced.clone(), free.clone()], &[]), Some(free));
        let strs = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // "spiel einen Am Akkord": CHORD =am T:midi.chord: outside a route
        let r = wall_route(std::slice::from_ref(&sourced), &strs(&["CHORD", "=am", "T:midi.chord:"]));
        assert_eq!(r.as_ref().map(|r| (r.mods.clone(), r.targets[0].token.as_str())), Some((strs(&["chord:am"]), "T:midi.chord:")));
        assert_eq!(wall_route(&[sourced], &strs(&["scope:all"])), None);
    }

    #[test]
    fn a_chip_from_another_plugin_keeps_its_plugin() {
        let picked = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let mut r = resolved("hand.openness", Some("left"), &[], vec![], true);
        if let Some(b) = r.targets[0].bound.as_mut() {
            b.plugin = Some("fx:0:0:Topos".into());
        }
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        assert_eq!(targets(&r, Some(&picked)), [Target::Param { plugin: topos, index: 12 }]);
    }
}
