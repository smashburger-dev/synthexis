//! Sentence field of the mapping assistant with IDE-style completion
//! (synthexis_complete): the best item shows as ghost text after the
//! cursor and Tab accepts it. From two typed characters a list of up to 8
//! items opens below the field (5 rows visible, it scrolls); the arrow keys
//! move through it, Tab or Enter picks, Esc dismisses. Enter picks only
//! after an arrow key, so Enter on a finished sentence still sends it.
//! A gesture done by one hand (hand, finger, eye, arm, one-hand motions)
//! needs its side picked: the selected row opens a panel right of the list,
//! Left picks left and Right picks right at any moment while it is open,
//! Enter confirms. There is no default side and no "both hands"; only a
//! side named right before the gesture ("die linke Fau") is preset. Tab or
//! Enter on a row without its side opens the panel and inserts nothing; the
//! ghost text shows the open choice ("(left/right?)"). A bare "Finger"
//! shows the fingers once a side is picked: Up/Down go through thumb to
//! pinky inside the panel. Esc closes the panel first, then the list.
//! An accepted item becomes a chip (a picked side shows as a badge); on Enter the sentence and
//! its chips (as pinned spans) go to the parser worker.
//!
//! The window draws the parts one by one (docs/design/ui-redesign.md R4,
//! R9): `tick` once per frame, then `picker_ui`, `field_ui`, `note_ui`,
//! `routes_strip` and `routes_inline`.
//!
//! Plugin picker: lists the plugins of the open FL project (fl.rs) as
//! "Portal, Master slot 1" and by default follows the plugin window last
//! clicked in FL. It opens upward over the camera with one group per
//! mixer track, Sound (generators routed to it) above FX (its slots), in
//! columns that wrap after a header and five plugins and scroll sideways.
//! Picking an entry pins it until the top row "Follow FL's focus". The
//! pin persists as a fl::Key id and is dropped when its slot holds
//! another plugin. Parameter names come from FL when a target is
//! picked; their classes from the exported plugin of the same name, else
//! the labels of a mapped plugin, else param-sense (sense.rs). The
//! completion loader thread stays on as the classifier: it gets each list
//! from the bridge as it arrives (fl::Bridge::on_names), so the classes are
//! ready even while the synthexis window draws no frame. It takes the
//! shared labels (sense::shared) for each list and lets them go after.
//!
//! After Enter the assistant's answer shows inline in the top section
//! under the strip, one line per route ("left hand openness -> Topos /
//! Drive"), and its routes run at once as pending (assign.rs, writer.rs):
//! the suggestions stay until Keep (or Cmd+Enter) makes them stay or
//! Discard (or Cmd+Backspace) removes them; Keep is dimmed with the reason
//! on hover while none of them can run. The arrow button flips a route's
//! direction. A reply about the hands asks the window to open Hands, one
//! about the face or head Head, a wall edit Pong. A sentence while
//! routes are pending adds to them. Kept routes are listed with a button
//! to remove each; "clear" removes all, "undo" the pending or the last kept
//! ones, "unmap" the ones with the same movement and target. A parameter
//! chip picked through a class phrase ("Cutoff" -> Filter 1 Freq) is kept
//! as an alias of that plugin (alias.rs).
//!
//! Language: every string comes from i18n.rs in the language setting, and
//! the engine filters its items by it (English items only in English, both
//! with German first in German); what the user types is never restricted.
//!
//! Completion data: $SYNTHEXIS_COMPLETION_DIR, else
//! ~/Library/Application Support/synthexis/completion, as
//! tools/mapping/export_completion.py writes it. The vocabulary loads on
//! its own thread (about 10 ms); the labels and param-sense the picker
//! classifies with are shared by all instances and load on first need
//! (sense::shared). Using the field keeps the parser loaded
//! (assistant::warm);
//! queries run on the GUI thread (about 2 ms per keystroke for a plugin of
//! 2,600 parameters), never on the audio thread.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, TryRecvError};
use nih_plug_egui::egui::{
    self, emath::easing, text::CCursor, text::CCursorRange, text::LayoutJob, text::TextFormat,
    Align, Align2, Color32, EventFilter, Id, Key, Layout, Modifiers, Order, Rect, RichText, Sense, TextEdit,
    Ui, UiBuilder, Vec2,
};
use parking_lot::RwLock;
use synthexis_complete::data::{self, PluginInfo};
use synthexis_complete::doc::ChipText;
use synthexis_complete::{finger_open, Chip, Engine, Lang, Param, Suggestion, Variants, FINGERS, GHOST_MIN_CHARS, SIDES};
use synthexis_parser::{Pin, PinWhat};

use crate::alias;
use crate::assign;
use crate::assistant;
use crate::edges::{EdgeIn, NodeTarget, Routing};
use crate::fl::{self, Link};
use crate::glass::{self, white, Tok};
use crate::i18n::{self, fill, Text};
use crate::log::log;
use crate::pong::arena::Side;
use crate::pong::setup::Action;
use crate::route::Target;
use crate::saved;
use crate::sense;
use crate::writer::Writer;

const TOP: usize = 8;
const VISIBLE: usize = 5;
const OPEN_SECS: f32 = 0.15;
const MOVE_SECS: f32 = 0.08;
const ROW_H: f32 = 22.0;
/// A plugin picker column's width (spec v3 2.7).
const PICK_COL_W: f32 = 184.0;
/// A picker column holds a track's header and five plugins, then wraps.
const PICK_COL_ROWS: usize = 6;
/// The ids of the field, the plugin picker button and the suggestions.
const FIELD_ID: &str = "synthexis-sentence";
const PICKER_ID: &str = "synthexis-picker";
const ROUTES_ID: &str = "synthexis-routes";
/// Signals of the hands (the node view shows only these, R19).
const HAND_SIGNAL: &str = "cam0.hand.";

/// A writer id with the (signal, target) of its engine route.
type Owned = (u64, (String, Target));
type Loaded = Result<(Engine, Vec<PluginInfo>), String>;
type Classified = (fl::Key, Vec<Param>);

/// While the field has the focus, the parser is asked to stay loaded this
/// often (assistant.rs drops it after 3 minutes unused).
const WARM_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Where the picker's classes come from.
enum Labels {
    /// sense::shared, loaded on first need.
    Shared,
    /// Fixed classes (tests).
    #[cfg(test)]
    Fixed(Arc<sense::Classes>),
}

pub struct Sentence {
    doc: ChipText,
    engine: Option<Engine>,
    /// Plugins with exported completion data, for parameter classes.
    exported: Vec<PluginInfo>,
    classes: HashMap<String, HashMap<String, String>>,
    /// Labels of the mapped plugins and param-sense, and their result per target.
    labels: Labels,
    classified: HashMap<fl::Key, Arc<Vec<Param>>>,
    /// Classes from the classifier thread (param-sense: about 80 ms for
    /// 2,600 names); None without a bridge, then sync_fl classifies itself.
    sensed: Option<Receiver<Classified>>,
    loader: Option<Receiver<Loaded>>,
    /// Ask the parser to load while the field is used (off in tests).
    warm_parser: bool,
    last_warm: Option<std::time::Instant>,
    status: Status,
    /// The assistant's answer to the last sentence, where it arrives, and
    /// the plugin it was resolved on.
    /// In the order sent, so a "clear" typed before a "map" is applied
    /// first; with the sentence as typed.
    reply: std::collections::VecDeque<(Receiver<assistant::Reply>, Option<fl::Key>, String)>,
    next_batch: u64,
    next_line: u64,
    /// What the last sentence did when it made no route ("no route (none)").
    mapped: Vec<String>,
    /// Routes of the sentences since the last Keep or Discard, and the kept ones.
    pending: Vec<Mapped>,
    kept: Vec<Mapped>,
    /// What the Keeps did in the MIDI store, for undo.
    midi_kept: Vec<MidiKept>,
    /// The MIDI key before a sentence with pending lines set it: Discard
    /// sets it back, Keep keeps the new one.
    key_before: Option<crate::midi::Key>,
    /// None without a plugin instance (tests).
    writer: Option<Arc<Writer>>,
    /// Where the kept lines are saved with the project (params `routes`),
    /// and what was saved last.
    store: Option<Arc<RwLock<String>>>,
    saved_json: String,
    /// The language setting of the last frame, for lines and messages.
    lang: Lang,
    bridge: Option<&'static fl::Bridge>,
    seen: u64,
    link: Link,
    targets: Vec<fl::Target>,
    fl_loaded: bool,
    focus: Option<fl::Key>,
    got: Option<(fl::Key, fl::Params)>,
    /// Current target: the pin, else the FL focus.
    want: Option<fl::Key>,
    /// Target whose parameters the engine holds.
    applied: Option<fl::Key>,
    pick_note: PickNote,
    /// Where the picker button and the field sat last frame.
    picker_button: Rect,
    field_rect: Rect,
    /// The field's `enabled` of the last frame (tick refilters with it).
    enabled: bool,
    /// The completion list showed last frame.
    list_shown: bool,
    /// The suggestions are unfolded with nothing pending: the reply lines
    /// and the kept lines show (not persisted). Pending lines always show.
    routes_open: bool,
    /// A reply, or a new pending batch, arrived since the suggestions last
    /// looked: a reply without a batch unfolds them.
    reply_news: bool,
    batch_news: bool,
    /// The last reply made new pending lines or wall edits.
    fresh: bool,
    /// Id of the pending batch (new with each reply that adds to it).
    batch_id: u64,
    next_batch_id: u64,
    /// A reply about the hands asks the window to open Hands (R7), one
    /// about the face or head Head, a wall edit Pong.
    open_hands: bool,
    open_head: bool,
    open_pong: bool,
    sugg: Vec<Suggestion>,
    list_open: bool,
    /// An arrow key moved through the list; Enter now picks instead of sending.
    nav: bool,
    sel: usize,
    /// First visible row of the list.
    scroll: usize,
    /// The variant panel's choice for the selected row (indices into SIDES
    /// and FINGERS); reset when the selection moves.
    side: Option<usize>,
    finger: Option<usize>,
    /// Esc closed the panel; Up/Down move through the list again.
    panel_closed: bool,
    /// Esc pressed; cleared by the next edit or cursor move.
    dismissed: bool,
    cursor: usize,
    id: Option<Id>,
    /// The Pong side a sentence without a movement assigns (the side the
    /// Pong section has selected); a Keep ends it.
    scope: Option<Side>,
    /// Sides those sentences set since the last Keep or Discard, one each.
    walls: Vec<WallEdit>,
    /// The writer's state_gen the lists were built from: a preset or a
    /// project load bumps it and the field reloads.
    state_gen: u64,
}

/// A side's action set by a scoped sentence, pending: it plays at once,
/// Discard puts `before` back and drops the part it made.
struct WallEdit {
    side: Side,
    before: Action,
    /// What the sentence set, and what it could not do.
    action: Action,
    note: assign::WallNote,
    /// The MIDI part the sentence made for the side.
    part: Option<u64>,
}

impl WallEdit {
    /// "top wall -> Serum 2 / Filter 1 Freq · range 30-80 %", in the
    /// language shown; the channel note goes once the part has one.
    fn line(&self, store: &crate::midi::Store, t: &Text) -> String {
        let row = format!("{} -> {}", wall_name(self.side, t), action_text(&self.action, store, t));
        let no_channel = matches!(self.action, Action::Part { id } if store.get(id).is_some_and(|p| p.channel.is_none()));
        let note = assign::WallNote { no_channel, ..self.note.clone() }.text(t);
        if note.is_empty() { row } else { format!("{row} · {note}") }
    }
}

/// The side's name in the field ("top wall").
fn wall_name(side: Side, t: &Text) -> &'static str {
    match side {
        Side::Top => t.w_top,
        Side::Right => t.w_right,
        Side::Bottom => t.w_bottom,
        Side::Left => t.w_left,
    }
}

/// The side's name standing alone ("Bottom wall"), for the strip.
fn wall_title(side: Side, t: &Text) -> &'static str {
    match side {
        Side::Top => t.fw_top,
        Side::Right => t.fw_right,
        Side::Bottom => t.fw_bottom,
        Side::Left => t.fw_left,
    }
}

/// The name a route's target bound for parameter `index`, else "#index".
/// The sections a reply shows in, (Hands, Head): Hands for a hand source
/// (a stand-in's live one: arm.raise reads hand.y) or a hand signal, Head
/// for a face, eye, mouth or head source or signal.
pub fn sections_for(sources: &[String], signals: &[&str]) -> (bool, bool) {
    let hands = sources.iter().any(|s| s.starts_with("hand.") || s.starts_with("hands."))
        || signals.iter().any(|s| s.starts_with(HAND_SIGNAL) || s.starts_with("cam0.hands."));
    let head = sources.iter().any(|s| crate::preset::head_source(s)) || signals.iter().any(|s| crate::preset::head_signal(s));
    (hands, head)
}

fn param_name(route: &synthexis_parser::resolve::Route, index: u32) -> String {
    route
        .targets
        .iter()
        .filter_map(|t| t.bound.as_ref())
        .find(|b| b.index == index)
        .map_or_else(|| format!("#{index}"), |b| b.name.clone())
}

/// What a side does, for its pending row: the part's name, the plugin and
/// parameter or the Signal, with the range or "toggles".
fn action_text(a: &Action, store: &crate::midi::Store, t: &Text) -> String {
    let how = |how: &crate::pong::setup::How| match how.mode {
        crate::pong::setup::Mode::Toggle => t.r_toggles.to_string(),
        _ => fill(t.r_range, &[&(how.lo * 100.0).round(), &(how.hi * 100.0).round()]),
    };
    match a {
        Action::Off => t.w_off.into(),
        Action::Part { id } => store.get(*id).map_or_else(|| t.w_off.to_string(), |p| p.name.clone()),
        Action::Param { key, name, how: h, .. } => {
            let plugin = fl::Key::parse(key).map(|k| k.plugin).unwrap_or_default();
            format!("{plugin} / {name} · {}", how(h))
        }
        Action::Macro { index, how: h } => format!("Signal {} · {}", index + 1, how(h)),
    }
}

/// Sets `setup`; the parts in `made` go when no side and no finger plays
/// them any more (and leave auto_channel), the parts in `back` return.
fn drop_wall_parts(w: &Writer, mut setup: crate::pong::setup::Setup, made: Vec<u64>, back: Vec<crate::midi::Part>) {
    let used = setup.wall_parts();
    let mut store = w.player.store();
    let gone: Vec<u64> = made.into_iter().filter(|id| !used.contains(id) && !store.binds.iter().any(|b| b.part == *id)).collect();
    setup.auto_channel.retain(|p| !gone.contains(p));
    if !gone.is_empty() || !back.is_empty() {
        store.parts.retain(|p| !gone.contains(&p.id));
        for p in back {
            if store.get(p.id).is_none() {
                store.parts.push(p);
            }
        }
        w.update_midi(store);
    }
    w.pong.set_setup(setup);
}

/// The field's own line under the picker, shown in the current language.
#[derive(Clone, Debug, PartialEq)]
enum Status {
    Ready,
    Loading,
    LoaderFailed(String),
    NoData(String),
}

impl Status {
    fn text(&self, t: &Text) -> String {
        match self {
            Status::Ready => String::new(),
            Status::Loading => t.loading.to_string(),
            Status::LoaderFailed(e) => fill(t.loader_failed, &[e]),
            Status::NoData(e) => fill(t.no_data, &[e]),
        }
    }
}

/// What the picker says about its current target.
#[derive(Clone, Debug, PartialEq)]
enum PickNote {
    None,
    /// The pinned plugin left its slot (its label).
    Gone(String),
    Params(usize),
    /// The classifier thread has the names, the classes are not back yet.
    Reading,
    /// FL's own error for the parameter names.
    Error(String),
}

impl PickNote {
    fn text(&self, t: &Text) -> String {
        match self {
            PickNote::None => String::new(),
            PickNote::Gone(label) => fill(t.pin_gone, &[label]),
            PickNote::Params(n) => fill(t.n_params, &[n]),
            PickNote::Reading => t.reading_classes.to_string(),
            PickNote::Error(e) => e.clone(),
        }
    }
}

/// The link to FL in words; MIDI port errors stay technical.
fn link_text(link: &Link, t: &Text) -> Option<String> {
    match link {
        Link::Waiting => Some(t.fl_waiting.to_string()),
        Link::Down(e) if e == fl::NO_ANSWER => Some(t.fl_no_answer.to_string()),
        Link::Down(e) => Some(match e.strip_prefix(fl::OLD_SCRIPT) {
            Some(rest) => fill(t.fl_old_script, &[&rest.split(',').next().unwrap_or(rest)]),
            None => e.clone(),
        }),
        Link::Up => None,
    }
}

/// The completion vocabulary as the loader thread reads it (also the
/// load-time probe's completion part).
pub fn load_vocab() -> Loaded {
    let t0 = std::time::Instant::now();
    let res = data::load(&data_dir());
    log(&format!("completion: vocab loaded in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3));
    res
}

/// Labels and param-sense as sense::shared reads them, for the probes.
pub fn load_labels() -> (sense::Classes, Vec<String>) {
    let t0 = std::time::Instant::now();
    let out = sense::Classes::load(&data_dir());
    log(&format!("completion: labels and param-sense loaded in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3));
    out
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_COMPLETION_DIR") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/completion")
}

/// One resolved route in the list: what the line says and the engine
/// routes it became.
/// A kept line the top section's strip lists: one with a route the
/// hands' and the Head's drives lists do not show (two hands' distance, a
/// source the camera does not measure). A line from a hand, a finger, the
/// face or the head is kept and unmapped in its drives list instead.
fn in_strip(m: &Mapped) -> bool {
    m.keys.is_empty() || !m.keys.iter().all(|(s, _)| crate::fingers::in_drives_list(s) || crate::head_ui::in_drives_list(s))
}

struct Mapped {
    route: synthexis_parser::resolve::Route,
    /// The plugin the sentence was resolved on.
    key: Option<fl::Key>,
    note: String,
    /// The writer's ids of its engine routes, and (signal, target) of each,
    /// in the same order.
    ids: Vec<u64>,
    keys: Vec<(String, Target)>,
    /// Keep number; undo takes back the last Keep as a whole.
    batch: u64,
    /// Stable id of the line; its engine routes carry it (route.rs
    /// averages a line's routes on one target).
    line: u64,
    /// How many engine routes the line had when it was made.
    full: usize,
    /// It lost part of its engine routes (unmap or modify of one hand).
    partly: bool,
    /// Kept routes a modify or replace took out; Discard, and undo of the
    /// Keep that kept this line, bring them back.
    replaced: Vec<Mapped>,
    /// MIDI parts the line made (assign::midi_routes) and what starts each:
    /// Keep binds them in the MIDI store, Discard drops them.
    parts: Vec<(String, crate::route::Trigger, u64)>,
}

impl Mapped {
    /// A copy without the lines it replaced.
    fn clone_shallow(&self) -> Mapped {
        Mapped {
            route: self.route.clone(),
            key: self.key.clone(),
            note: self.note.clone(),
            ids: self.ids.clone(),
            keys: self.keys.clone(),
            batch: self.batch,
            line: self.line,
            full: self.full,
            partly: self.partly,
            replaced: Vec::new(),
            parts: self.parts.clone(),
        }
    }

    fn new(route: synthexis_parser::resolve::Route, key: Option<&fl::Key>, t: &Text) -> (Self, Vec<crate::route::Route>) {
        let (routes, note) = assign::routes_in(&route, key, t);
        let keys: Vec<(String, Target)> = routes.iter().map(|x| (x.signal.clone(), x.target.clone())).collect();
        let full = keys.len();
        (Mapped { route, key: key.cloned(), note, ids: Vec::new(), keys, batch: 0, line: 0, full, partly: false, replaced: Vec::new(), parts: Vec::new() }, routes)
    }

    fn line(&self, t: &Text) -> String {
        let plugin = self.key.as_ref().map_or("", |k| k.plugin.as_str());
        let w = t.words();
        let mut l = self.route.describe_in(plugin, &w);
        // a stand-in names what runs: "hand height (both hands) -> ..."
        if let Some(live) = assign::stand_in_label(&self.route.source, self.route.side.as_deref(), t) {
            if let Some(rest) = l.strip_prefix(&self.route.source_label_in(&w)) {
                let inv = if self.route.inv { format!(" {}", w.inverted) } else { String::new() };
                l = format!("{live}{inv}{rest}");
            }
        }
        let notes: Vec<&str> = [self.note.as_str(), if self.partly { t.partly } else { "" }]
            .into_iter()
            .filter(|n| !n.is_empty())
            .collect();
        if notes.is_empty() {
            l
        } else {
            format!("{l} · {}", notes.join(", "))
        }
    }
}

/// What one Keep did in the MIDI store; undo takes it back with the kept
/// lines of its batch.
struct MidiKept {
    batch: u64,
    /// The binds it made, and the binds on their triggers before.
    binds: Vec<crate::midi::Bind>,
    before: Vec<crate::midi::Bind>,
    /// The key before its sentences set it.
    key: Option<crate::midi::Key>,
    /// Kept lines its MIDI lines replaced.
    replaced: Vec<Mapped>,
    /// (side, action before, action kept) of the Pong sides it kept, and
    /// the parts the sides played before that it deleted.
    walls: Vec<(Side, Action, Action)>,
    parts: Vec<crate::midi::Part>,
}


/// Parameter name -> class of the exported plugin named `plugin`.
fn exported_classes(plugins: &[PluginInfo], plugin: &str) -> HashMap<String, String> {
    let info = plugins.iter().find(|p| p.name.eq_ignore_ascii_case(plugin));
    info.and_then(|i| data::load_plugin(&data_dir(), i).ok())
        .map(|ps| ps.into_iter().map(|p| (p.name, p.class)).collect())
        .unwrap_or_default()
}

/// The shared labels and param-sense (sense::shared), waiting while they
/// load. Only for the classifier thread: it blocks.
fn shared_labels() -> Arc<sense::Classes> {
    loop {
        if let Some(c) = sense::shared(&data_dir()) {
            return c;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Classifies each parameter list the bridge hands over until the field
/// is gone (its receiver dropped). `labels` is asked per list and let go
/// after it, so the shared copy can drop while no plugin is picked.
fn classify_loop(
    b: &fl::Bridge,
    labels: impl Fn() -> Arc<sense::Classes>,
    plugins: &[PluginInfo],
    out: &crossbeam_channel::Sender<Classified>,
) {
    let (tx, rx) = crossbeam_channel::unbounded();
    b.on_names(tx);
    let mut exported: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (key, names) in rx {
        let ex = exported.entry(key.plugin.clone()).or_insert_with(|| exported_classes(plugins, &key.plugin));
        let params = classify(&labels(), &key, names, ex);
        if out.send((key, params)).is_err() {
            return;
        }
    }
}

/// FL's parameter names of `key` with their classes (sense::Classes::of).
fn classify(
    labels: &sense::Classes,
    key: &fl::Key,
    names: Vec<(u32, String)>,
    exported: &HashMap<String, String>,
) -> Vec<Param> {
    let t0 = std::time::Instant::now();
    let list: Vec<String> = names.iter().map(|(_, n)| n.clone()).collect();
    let (classes, src) = labels.of(&key.plugin, &list, exported);
    log(&format!(
        "picker: classes for {}: {} labelled, {} by param-sense, {:.1} ms",
        key.label(),
        src.labelled,
        src.sensed,
        t0.elapsed().as_secs_f64() * 1e3
    ));
    names.into_iter().zip(classes).map(|((index, name), class)| Param { index, name, class }).collect()
}

fn set_cursor(ctx: &egui::Context, id: Id, text: &str, byte: usize) {
    if let Some(mut st) = TextEdit::load_state(ctx, id) {
        let ci = text[..byte.min(text.len())].chars().count();
        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(ci))));
        st.store(ctx, id);
    }
}

fn to_pin(range: &std::ops::Range<usize>, chip: &Chip) -> Pin {
    let what = match chip {
        Chip::Param { class, .. } => PinWhat::Param {
            id: chip.id(),
            class: class.clone(),
            switch: chip.is_switch(),
        },
        Chip::Gesture { source, shape, side, .. } => PinWhat::Gesture {
            source: source.clone(),
            shape: shape.clone(),
            side: side.clone(),
        },
        Chip::Target { kind, .. } => PinWhat::Target { kind: kind.clone() },
    };
    Pin { bytes: (range.start, range.end), what }
}

/// The open or made choice of a row, for the ghost text: "left/right?",
/// "left, finger?", "right, index".
fn variant_note(v: Variants, side: Option<usize>, finger: Option<usize>, t: &Text) -> String {
    let s = side.and_then(|i| t.sides().get(i).copied()).unwrap_or(t.side_open).to_string();
    let f = finger.and_then(|i| t.fingers().get(i).copied()).unwrap_or(t.finger_open).to_string();
    match (v.sided, v.finger) {
        (true, true) => format!("{s}, {f}"),
        (false, true) => f,
        _ => s,
    }
}

/// The badge after a chip: its finger when the phrase named none, its side.
fn badge(chip: &Chip, t: &Text) -> Option<String> {
    let Chip::Gesture { source, phrase, side, .. } = chip else { return None };
    let finger = finger_open(phrase_source(source), phrase)
        .then(|| FINGERS.iter().position(|f| source.strip_prefix("hand.finger.") == Some(*f)))
        .flatten()
        .map(|i| t.fingers()[i].to_string());
    let side = side.as_deref().and_then(|s| SIDES.iter().position(|x| *x == s)).map(|i| t.side_badges()[i].to_string());
    let parts: Vec<String> = [finger, side].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A picked finger replaced the source; finger_open only asks the family.
fn phrase_source(source: &str) -> &str {
    if source.starts_with("hand.finger.") {
        "hand.finger.index"
    } else {
        source
    }
}

/// Secondary text of a list row: what the item is (class and source ids
/// stay ids in both languages).
fn hint(s: &Suggestion, t: &Text) -> String {
    match (&s.chip, &s.via) {
        (Chip::Param { class, .. }, Some(v)) => format!("{class} · \"{v}\""),
        (Chip::Param { class, .. }, None) => class.clone(),
        (Chip::Gesture { source, shape: Some(sh), .. }, _) => format!("{source}/{sh}"),
        (Chip::Gesture { source, pol, .. }, _) if *pol < 0 => fill(t.inverse, &[source]),
        (Chip::Gesture { source, .. }, _) => source.clone(),
        (Chip::Target { kind, .. }, _) => match kind.as_str() {
            "midi.note" => t.midi_note.into(),
            "midi.chord" => t.midi_chord.into(),
            "midi.pattern" => t.midi_pattern.into(),
            "midi.cc" => t.midi_cc.into(),
            "macro" => t.macro_.into(),
            "xy" => t.xy_pad.into(),
            k => k.strip_prefix("midi.prop:").map_or_else(|| k.to_string(), |p| fill(t.midi_prop, &[&p])),
        },
    }
}

impl Sentence {
    /// `bridge`: None runs without FL (tests); `writer`: None maps nothing;
    /// `store`: the saved kept lines (saved.rs), shown again.
    pub fn new(bridge: Option<&'static fl::Bridge>, writer: Option<Arc<Writer>>, store: Option<Arc<RwLock<String>>>) -> Self {
        let (tx, rx) = bounded(1);
        let (stx, srx) = crossbeam_channel::unbounded();
        let spawned = std::thread::Builder::new()
            .name("synthexis-completion".into())
            .spawn(move || {
                let loaded = load_vocab();
                let plugins = loaded.as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
                let _ = tx.send(loaded);
                if let Some(b) = bridge {
                    classify_loop(b, shared_labels, &plugins, &stx);
                }
            });
        let status = match spawned {
            Ok(_) => Status::Loading,
            Err(e) => Status::LoaderFailed(e.to_string()),
        };
        let mut s = Self {
            doc: ChipText::default(),
            engine: None,
            exported: Vec::new(),
            classes: HashMap::new(),
            labels: Labels::Shared,
            classified: HashMap::new(),
            sensed: bridge.map(|_| srx),
            loader: Some(rx),
            warm_parser: bridge.is_some(),
            last_warm: None,
            status,
            reply: std::collections::VecDeque::new(),
            next_batch: 1,
            next_line: 1,
            mapped: Vec::new(),
            pending: Vec::new(),
            kept: Vec::new(),
            midi_kept: Vec::new(),
            key_before: None,
            writer,
            store,
            saved_json: String::new(),
            lang: Lang::En,
            bridge,
            seen: u64::MAX,
            link: Link::Waiting,
            targets: Vec::new(),
            fl_loaded: false,
            focus: None,
            got: None,
            want: None,
            applied: None,
            pick_note: PickNote::None,
            picker_button: Rect::NOTHING,
            field_rect: Rect::NOTHING,
            enabled: true,
            list_shown: false,
            routes_open: false,
            reply_news: false,
            batch_news: false,
            fresh: false,
            batch_id: 0,
            next_batch_id: 1,
            open_hands: false,
            open_head: false,
            open_pong: false,
            sugg: Vec::new(),
            list_open: false,
            nav: false,
            sel: 0,
            scroll: 0,
            side: None,
            finger: None,
            panel_closed: false,
            dismissed: false,
            cursor: 0,
            id: None,
            scope: None,
            walls: Vec::new(),
            state_gen: 0,
        };
        s.restore_saved();
        s.state_gen = s.writer.as_ref().map_or(0, |w| w.state_gen());
        s
    }

    /// After a preset or project load (the writer's state_gen moved): the
    /// kept lines come again from the project, and saved_json is what was
    /// loaded, so the next save keeps those lines. This also covers a
    /// reopened project, whose window is built before set_state. What was
    /// pending still runs and goes: initialize() again on the same state
    /// (FL re-activating the plugin) takes it back as Discard does; a load
    /// of other kept lines replaced the parts and sides, so only its
    /// routes are left to remove.
    fn sync_state(&mut self) {
        let Some(g) = self.writer.as_ref().map(|w| w.state_gen()) else { return };
        if g == self.state_gen {
            return;
        }
        self.state_gen = g;
        let same = self.store.as_ref().is_none_or(|s| *s.read() == self.saved_json);
        if same {
            self.discard();
            // the kept lines the pending ones had replaced, before the reload reads them
            self.save();
        } else {
            let pending = std::mem::take(&mut self.pending);
            self.remove(&pending.iter().flat_map(|m| m.ids.clone()).collect::<Vec<_>>());
        }
        self.kept.clear();
        self.pending.clear();
        self.midi_kept.clear();
        self.key_before = None;
        self.mapped.clear();
        self.walls.clear();
        self.scope = None;
        self.restore_saved();
        if let Some(w) = &self.writer {
            self.next_line = self.next_line.max(w.max_line() + 1);
        }
    }

    /// The Pong side sentences without a movement assign, None for none.
    pub fn set_scope(&mut self, side: Option<Side>) {
        self.scope = side;
    }

    pub fn scope(&self) -> Option<Side> {
        self.scope
    }

    /// What Discard does: pending routes and their parts go, the routes
    /// they replaced, the key before and the sides before come back (the
    /// Pong section runs it before a preset load).
    pub fn discard_pending(&mut self) {
        self.discard();
    }

    /// The saved kept lines with the engine routes initialize() started for
    /// them (saved::run).
    fn restore_saved(&mut self) {
        let (Some(store), Some(w)) = (&self.store, &self.writer) else { return };
        let json = store.read().clone();
        for sv in saved::load(&json) {
            let key = sv.key.as_deref().and_then(fl::Key::parse);
            let (ids, keys): (Vec<u64>, Vec<(String, Target)>) =
                w.line_routes(sv.line).into_iter().map(|(id, sig, t)| (id, (sig, t))).unzip();
            self.next_line = self.next_line.max(sv.line + 1);
            self.next_batch = self.next_batch.max(sv.batch + 1);
            self.kept.push(Mapped {
                route: sv.route,
                key,
                note: sv.note,
                ids,
                keys,
                batch: sv.batch,
                line: sv.line,
                full: sv.full,
                partly: sv.partly,
                replaced: Vec::new(),
                parts: Vec::new(),
            });
        }
        self.saved_json = json;
    }

    /// Saves the kept lines when they changed.
    fn save(&mut self) {
        let Some(store) = &self.store else { return };
        let lines: Vec<saved::Saved> = self
            .kept
            .iter()
            .map(|m| saved::Saved {
                key: m.key.as_ref().map(fl::Key::id),
                route: m.route.clone(),
                line: m.line,
                batch: m.batch,
                full: m.full,
                partly: m.partly,
                note: m.note.clone(),
                keys: m.keys.iter().map(|(sig, t)| (sig.clone(), saved::target_id(t))).collect(),
            })
            .collect();
        let json = if lines.is_empty() { String::new() } else { saved::store(&lines) };
        if json != self.saved_json {
            store.write().clone_from(&json);
            self.saved_json = json;
        }
    }

    fn poll_loader(&mut self) {
        let Some(rx) = &self.loader else { return };
        let res = match rx.try_recv() {
            Ok(res) => res,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Err("completion loader stopped".into()),
        };
        self.loader = None;
        match res {
            Ok((engine, plugins)) => {
                self.engine = Some(engine);
                self.exported = plugins;
                self.status = Status::Ready;
            }
            Err(e) => {
                log(&format!("completion: {e}"));
                self.status = Status::NoData(e);
            }
        }
    }

    /// The picker's classes, None while the shared ones still load.
    fn labels(&self) -> Option<Arc<sense::Classes>> {
        match &self.labels {
            Labels::Shared => sense::shared(&data_dir()),
            #[cfg(test)]
            Labels::Fixed(c) => Some(c.clone()),
        }
    }

    /// Parameter name -> class from the exported plugin named `plugin`.
    fn classes_of(&mut self, plugin: &str) -> &HashMap<String, String> {
        if !self.classes.contains_key(plugin) {
            let map = exported_classes(&self.exported, plugin);
            self.classes.insert(plugin.to_string(), map);
        }
        &self.classes[plugin]
    }

    /// Takes the bridge's news, resolves the target and hands its
    /// parameters to the engine once FL sent them.
    fn sync_fl(&mut self, pin: &RwLock<String>) {
        let Some(b) = self.bridge else { return };
        while let Some(Ok((k, params))) = self.sensed.as_ref().map(Receiver::try_recv) {
            self.classified.insert(k, Arc::new(params));
        }
        {
            let v = b.view();
            if v.gen != self.seen {
                self.seen = v.gen;
                self.link = v.link.clone();
                self.targets = v.targets.clone();
                self.fl_loaded = v.loaded;
                self.focus = v.focus.clone();
                self.got = v.params.clone();
            }
        }
        let pinned = fl::Key::parse(&pin.read());
        if let Some(p) = &pinned {
            if self.fl_loaded && !self.targets.iter().any(|t| t.key == *p) {
                self.pick_note = PickNote::Gone(p.label());
                pin.write().clear();
            }
        }
        let target = fl::Key::parse(&pin.read()).or_else(|| self.focus.clone());
        if target != self.want {
            let how = if pin.read().is_empty() { "follows FL" } else { "pinned" };
            log(&format!("picker: {} ({how})", target.as_ref().map_or("none".into(), fl::Key::label)));
            self.want = target;
            if let Some(k) = &self.want {
                b.want(k);
            }
            if let Some(e) = self.engine.as_mut() {
                // no stale parameters while FL sends the new ones
                e.set_plugin(&self.want.as_ref().map(fl::Key::id).unwrap_or_default(), Vec::new());
                self.applied = None;
            }
        }
        let Some(want) = self.want.clone() else { return };
        if self.applied.as_ref() == Some(&want) || self.engine.is_none() {
            return;
        }
        let Some((k, res)) = self.got.clone() else { return };
        if k != want {
            return;
        }
        match res {
            Ok(names) => {
                if !self.classified.contains_key(&want) {
                    // the classifier thread started on these names already
                    if self.sensed.is_some() {
                        self.pick_note = PickNote::Reading;
                        return;
                    }
                    // the shared labels load on first need: try again next frame
                    let Some(labels) = self.labels() else { return };
                    let exported = self.classes_of(&want.plugin).clone();
                    let params = classify(&labels, &want, names, &exported);
                    self.classified.insert(want.clone(), Arc::new(params));
                }
                let params = self.classified.get(&want).cloned().unwrap_or_default();
                self.pick_note = PickNote::Params(params.len());
                if let Some(e) = self.engine.as_mut() {
                    e.set_plugin(&want.id(), params.to_vec());
                }
                self.applied = Some(want);
            }
            Err(e) => self.pick_note = PickNote::Error(e),
        }
    }

    /// The plugin picker's groups (spec v3 2.7): one per mixer track,
    /// Master first, its sounds (channel order) above its FX (slot order).
    fn picker_groups(&self) -> Vec<(String, Vec<&fl::Target>)> {
        let mut tracks: BTreeMap<u32, Vec<&fl::Target>> = BTreeMap::new();
        for t in &self.targets {
            tracks.entry(t.track).or_default().push(t);
        }
        tracks
            .into_values()
            .map(|mut items| {
                items.sort_by_key(|t| {
                    let n = match t.key.at {
                        fl::At::Gen { ch } => ch,
                        fl::At::Fx { slot, .. } => slot,
                    };
                    (!t.is_sound(), n)
                });
                (items.first().map(|t| t.track_label()).unwrap_or_default(), items)
            })
            .collect()
    }

    /// The plugin picker, `size` big (R4): the pinned plugin's name, the
    /// followed one's in the placeholder alpha, else "Plugin picker". It
    /// opens upward over the camera (spec v3 2.7): "Follow FL's focus",
    /// then one group per mixer track in columns of at most
    /// `PICK_COL_ROWS` rows (fewer when the camera leaves less room), a
    /// track kept in one column where it fits; the columns scroll
    /// sideways when they are wider than the window. The first row
    /// follows FL's focus again; any other row pins.
    pub fn picker_ui(&mut self, ui: &mut Ui, size: Vec2, pin: &RwLock<String>) {
        let t = i18n::text(self.lang);
        let pinned = fl::Key::parse(&pin.read());
        // glass::picker_button has VALUE and HINT only: the followed name
        // takes HINT (0.52) where R4 asks for ROW (0.56)
        let (shown, placeholder, hover) = match (&pinned, &self.want) {
            (Some(p), _) => (p.plugin.clone(), false, p.label()),
            (None, Some(f)) => (f.plugin.clone(), true, t.picker_following.to_string()),
            (None, None) => (t.plugin_picker.to_string(), true, String::new()),
        };
        let id = Id::new(PICKER_ID);
        let button = glass::picker_button(ui, id, size, &shown, placeholder, &hover);
        self.picker_button = button.rect;
        let current = pinned.clone().or_else(|| self.want.clone());
        let groups = self.picker_groups();
        let sizes: Vec<(bool, usize)> = std::iter::once((false, 1)).chain(groups.iter().map(|g| (true, g.1.len()))).collect();
        let pad = 2.0 * glass::POP_PAD_PT;
        // a full column decides the side, here and in `popup`
        let want = PICK_COL_ROWS as f32 * Tok::POP_ROW_H + pad;
        let room = glass::place(button.rect, size.x, want, glass::popup_window(ui.ctx()), glass::Open::Up).max_h;
        let cap = (((room - pad) / Tok::POP_ROW_H).floor().max(0.0) as usize).clamp(2, PICK_COL_ROWS);
        let cols = glass::columns(&sizes, cap);
        let n = cols.len().max(1) as f32;
        let w = (n * PICK_COL_W + (n - 1.0) * Tok::GAP + pad).max(size.x).max(glass::POP_MIN_W);
        let col_h = cap.min(cols.iter().map(Vec::len).max().unwrap_or(1)) as f32 * Tok::POP_ROW_H;
        let seps = glass::col_seps(&cols);
        // Left/Right page the columns while the list is open (the keys go
        // nowhere else then); last frame's offset and room decide where to
        let pop_id = id.with("popup");
        let hx = pop_id.with("hx");
        let dir = if ui.memory(|m| m.is_popup_open(pop_id)) {
            ui.input_mut(|i| i32::from(i.consume_key(Modifiers::NONE, Key::ArrowRight)) - i32::from(i.consume_key(Modifiers::NONE, Key::ArrowLeft)))
        } else {
            0
        };
        let (last_x, max_x) = ui.data(|d| d.get_temp::<(f32, f32)>(hx)).unwrap_or((0.0, 0.0));
        let set_x = ui.data(|d| d.get_temp::<f32>(pop_id.with("hx-shot"))).map(|x| x.clamp(0.0, max_x)).or((dir != 0).then(|| glass::col_step(last_x, dir, PICK_COL_W + Tok::GAP, max_x)));
        let choice = glass::popup(ui, id, &button, glass::Open::Up, w, want, f32::INFINITY, |ui, inner_h| {
            let fits = col_h <= inner_h + 0.5;
            ui.style_mut().always_scroll_the_only_direction = true;
            let mut area = egui::ScrollArea::new([true, !fits]).max_height(inner_h).auto_shrink([false, true]);
            if let Some(x) = set_x {
                area = area.horizontal_scroll_offset(x);
            }
            let out = area
                .show(ui, |ui| {
                    let mut choice = None;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(Tok::GAP, 0.0);
                        let mut col_rects = Vec::with_capacity(cols.len());
                        for col in &cols {
                            let layout = egui::Layout::top_down_justified(egui::Align::LEFT);
                            let r = ui.allocate_ui_with_layout(egui::vec2(PICK_COL_W, col_h), layout, |ui| {
                                ui.set_width(PICK_COL_W);
                                for cell in col {
                                    match *cell {
                                        glass::ColCell::Item { group: 0, .. } => {
                                            if glass::pick_row(ui, id.with("follow"), pinned.is_none(), true, t.follow_fl, "", t.follow_fl_hover, None).clicked {
                                                choice = Some(None);
                                            }
                                        }
                                        glass::ColCell::Head { group, .. } => {
                                            let label = groups.get(group - 1).map(|g| g.0.as_str()).unwrap_or("");
                                            glass::pick_header(ui, &glass::fit(ui, label, Tok::BODY, PICK_COL_W - 16.0));
                                        }
                                        glass::ColCell::Item { group, item } => {
                                            let Some(tg) = groups.get(group - 1).and_then(|g| g.1.get(item)) else { continue };
                                            let k = &tg.key;
                                            let place = match k.at {
                                                fl::At::Gen { .. } => t.sound.to_string(),
                                                fl::At::Fx { slot, .. } => format!("{} {slot}", t.fx),
                                            };
                                            let row = glass::pick_row(ui, id.with(("row", k.id())), current.as_ref() == Some(k), true, &k.plugin, &place, &k.label(), None);
                                            if row.clicked {
                                                choice = Some(Some(k.clone()));
                                            }
                                        }
                                    }
                                }
                            });
                            col_rects.push(r.response.rect);
                        }
                        // thin lines in the gaps: stronger where a track starts
                        for (pair, new_group) in col_rects.windows(2).zip(&seps) {
                            let x = ui.painter().round_to_pixel_center((pair[0].right() + pair[1].left()) / 2.0);
                            let top = pair[0].top() + 3.0;
                            let y = egui::Rangef::new(top, top + col_h - 6.0);
                            ui.painter().vline(x, y, egui::Stroke::new(1.0_f32, white(glass::col_sep_alpha(*new_group))));
                        }
                    });
                    choice
                });
            let room = (out.content_size.x - out.inner_rect.width()).max(0.0);
            ui.data_mut(|d| d.insert_temp(hx, (out.state.offset.x, room)));
            // chevrons in the padding where columns hide (design-glass)
            let (view, x, full) = (out.inner_rect, out.state.offset.x, out.content_size.x);
            let mid = view.center().y;
            if x > 1.0 {
                glass::chevron_at(ui.painter(), egui::pos2(view.left() - 4.0, mid), 4.0, 2.0, white(Tok::VALUE));
            }
            if x + view.width() < full - 1.0 {
                glass::chevron_at(ui.painter(), egui::pos2(view.right() + 4.0, mid), 4.0, 0.0, white(Tok::VALUE));
            }
            out.inner
        })
        .flatten();
        if let Some(c) = choice {
            ui.memory_mut(|m| m.close_popup());
            *pin.write() = c.map(|k| k.id()).unwrap_or_default();
            self.pick_note = PickNote::None;
        }
    }

    /// Window captures: holds the plugin picker's columns scrolled to `x`
    /// (`SYNTHEXIS_UI_SCROLL_X`; past the end shows the last columns).
    pub fn shot_scroll_x(ctx: &egui::Context, x: f32) {
        ctx.data_mut(|d| d.insert_temp(Id::new(PICKER_ID).with("popup").with("hx-shot"), x));
    }

    /// Window captures: carries out `routes` for the plugin `key` as the
    /// typed `text` would (a map reply) and keeps them if `keep`.
    pub fn shot_said(&mut self, routes: Vec<synthexis_parser::resolve::Route>, key: &fl::Key, text: &str, keep: bool) {
        let r = assistant::Reply { intent: "map".into(), rest: Vec::new(), routes, lines: vec![text.to_string()] };
        self.apply_said(r, Some(key), text);
        if keep {
            self.keep();
        }
    }

    /// What the picker needs from FL and the field's own status, joined
    /// by " · ".
    fn note_text(&self, t: &Text) -> String {
        let note = match &self.link {
            _ if self.bridge.is_none() => String::new(),
            Link::Up if self.fl_loaded && self.targets.is_empty() => t.fl_no_plugins.to_string(),
            Link::Up if self.want.is_none() => t.fl_click_plugin.to_string(),
            Link::Up => self.pick_note.text(t),
            l => link_text(l, t).unwrap_or_default(),
        };
        let status = self.status.text(t);
        let notes: Vec<&str> = [note.as_str(), status.as_str()].into_iter().filter(|n| !n.is_empty()).collect();
        notes.join(" · ")
    }

    /// The plugin note under the picker (13, HINT, cut with "…" to `width`,
    /// the whole text on hover); the row stays reserved when it is empty.
    pub fn note_ui(&self, ui: &mut Ui, width: f32) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, Tok::CTRL_H), Sense::hover());
        let text = self.note_text(i18n::text(self.lang));
        if text.is_empty() {
            return;
        }
        let shown = glass::fit(ui, &text, Tok::BODY, width);
        let g = glass::galley(ui, &shown, Tok::BODY);
        glass::text_at(ui.painter(), egui::pos2(rect.left(), rect.center().y - g.size().y / 2.0), g, white(Tok::HINT));
        if shown != text {
            resp.on_hover_text(text);
        }
    }

    fn refresh(&mut self, enabled: bool) {
        self.sugg = match (&self.engine, enabled) {
            (Some(e), true) => e.complete(&self.doc.text, self.cursor, self.doc.floor(self.cursor), TOP),
            _ => Vec::new(),
        };
        // new items after an edit: the best one is selected again
        self.sel = 0;
        self.scroll = 0;
        self.nav = false;
        self.reset_variant();
        let typed = self.sugg.first().map_or(0, |s| self.doc.text[s.replace.clone()].chars().count());
        self.list_open = !self.dismissed && self.sugg.len() >= 2 && typed >= GHOST_MIN_CHARS;
    }

    fn reset_variant(&mut self) {
        let v = self.sugg.get(self.sel).map(|s| s.variants).unwrap_or_default();
        (self.side, self.finger, self.panel_closed) = (v.side, None, false);
    }

    /// The variant choice row `i` would take: the panel's for the selected
    /// row, else only the side its sentence names.
    fn choice(&self, i: usize) -> (Option<usize>, Option<usize>) {
        if i == self.sel {
            (self.side, self.finger)
        } else {
            (self.sugg.get(i).and_then(|s| s.variants.side), None)
        }
    }

    /// Row `i` still needs a side or a finger before it can become a chip.
    fn needs_choice(&self, i: usize) -> bool {
        let Some(v) = self.sugg.get(i).map(|s| s.variants) else { return false };
        let (side, finger) = self.choice(i);
        (v.sided && side.is_none()) || (v.finger && finger.is_none())
    }

    /// The selected row's panel is showing: the list is open, the row has
    /// variants and Esc has not closed it.
    fn panel_open(&self) -> bool {
        self.list_open && !self.panel_closed && self.sugg.get(self.sel).is_some_and(|s| s.variants.any())
    }

    /// Tab, Enter or a click on row `i`: a row that still needs a choice
    /// opens its panel and inserts nothing; any other becomes a chip.
    fn take(&mut self, ctx: &egui::Context, id: Id, i: usize) {
        if self.needs_choice(i) {
            self.list_open = true;
            self.dismissed = false;
            self.select(i);
            self.panel_closed = false;
        } else {
            self.accept(ctx, id, i);
        }
    }

    fn select(&mut self, i: usize) {
        if i != self.sel {
            self.sel = i;
            self.reset_variant();
        }
        self.nav = true;
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + VISIBLE {
            self.scroll = i + 1 - VISIBLE;
        }
    }

    fn accept(&mut self, ctx: &egui::Context, id: Id, i: usize) {
        let Some(mut s) = self.sugg.get(i).cloned() else { return };
        let (side, finger) = self.choice(i);
        s.chip = s.chip.with_variant(side, finger);
        if let (Chip::Param { name, .. }, Some(via), Some(k)) = (&s.chip, &s.via, &self.applied) {
            alias::learn(&k.plugin, via, name);
        }
        self.cursor = self.doc.accept(&s);
        if let Some(e) = self.engine.as_mut() {
            e.picked(&s.chip);
        }
        set_cursor(ctx, id, &self.doc.text, self.cursor);
        self.list_open = false;
        self.sugg.clear();
    }

    /// The assistant's replies in the order sent; asks for frames meanwhile.
    fn poll_reply(&mut self, ctx: &egui::Context) {
        while let Some((rx, key, text)) = self.reply.front() {
            match rx.try_recv() {
                Ok(r) => {
                    let (key, text) = (key.clone(), text.clone());
                    self.reply.pop_front();
                    self.apply_said(r, key.as_ref(), &text);
                }
                Err(TryRecvError::Disconnected) => {
                    self.mapped = vec![i18n::text(self.lang).parser_unavailable.into()];
                    self.reply_news = true;
                    self.reply.pop_front();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    return;
                }
            }
        }
    }

    /// The user's MIDI names complete like the vocabulary's targets: their
    /// pattern names and the templates, the chords of their key (name and
    /// degree), the notes their parts play.
    fn sync_midi(&mut self) {
        let (Some(w), Some(e)) = (&self.writer, self.engine.as_mut()) else { return };
        let store = w.player.store();
        let mut user: Vec<(String, String)> = Vec::new();
        let mut add = |kind: &str, phrase: String| {
            if !user.iter().any(|(k, p)| k == kind && p.eq_ignore_ascii_case(&phrase)) {
                user.push((kind.to_string(), phrase));
            }
        };
        for p in &store.parts {
            match &p.play {
                crate::midi::Play::Clip { .. } => add("midi.pattern", p.name.clone()),
                crate::midi::Play::Note { pitch, .. } => add("midi.note", crate::midi::note_name(*pitch)),
                crate::midi::Play::Chords { .. } => {}
            }
        }
        for name in crate::midi::TEMPLATES.iter().take(5) {
            add("midi.pattern", (*name).to_string());
        }
        let key = store.key;
        for d in 0..key.scale.steps().len() as u8 {
            add("midi.chord", crate::midi::chord_name(key, d, 3));
        }
        e.set_user_targets(user);
    }

    /// The lines for `route`: MIDI targets make their parts in the player's
    /// store at once (they play while pending), everything else goes
    /// through assign::routes_in; a route with both is two lines.
    fn make(&self, route: synthexis_parser::resolve::Route, key: Option<&fl::Key>, t: &Text) -> Vec<(Mapped, Vec<crate::route::Route>)> {
        let Some(w) = self.writer.as_ref().filter(|_| assign::is_midi(&route)) else { return vec![Mapped::new(route, key, t)] };
        let (midi, rest) = assign::split_midi(&route);
        let mut out = Vec::new();
        if let Some(route) = midi {
            let mut store = w.player.store();
            let chans = self.bridge.map(|b| b.view().channels.clone()).unwrap_or_default();
            match assign::midi_routes(&route, key, &mut store, &chans, t) {
                Some((routes, note, _)) => {
                    w.update_midi(store);
                    let keys: Vec<(String, Target)> = routes.iter().map(|x| (x.signal.clone(), x.target.clone())).collect();
                    let parts = routes
                        .iter()
                        .filter_map(|x| match x.target {
                            Target::Part { id, trigger } => Some((x.signal.clone(), trigger, id)),
                            _ => None,
                        })
                        .collect();
                    let full = keys.len();
                    let m = Mapped { route, key: key.cloned(), note, ids: Vec::new(), keys, batch: 0, line: 0, full, partly: false, replaced: Vec::new(), parts };
                    out.push((m, routes));
                }
                None => out.push(Mapped::new(route, key, t)),
            }
        }
        out.extend(rest.map(|r| Mapped::new(r, key, t)));
        out
    }

    /// Runs `routes` as pending in the writer under a new line id; their
    /// ids go to `m`.
    fn propose(&mut self, m: &mut Mapped, routes: Vec<crate::route::Route>) {
        if m.line == 0 {
            m.line = self.next_line;
            self.next_line += 1;
            self.fresh = true;
        }
        let routes = routes.into_iter().map(|mut r| {
            r.line = m.line;
            r
        });
        if let Some(w) = &self.writer {
            m.ids = w.propose(routes.collect());
        }
    }

    /// `apply` for the sentence `text` as typed: a sentence about the
    /// hands in the plural without a side reads both hands, whatever side
    /// the parser guessed (assign::side_from_text).
    fn apply_said(&mut self, mut r: assistant::Reply, key: Option<&fl::Key>, text: &str) {
        for route in &mut r.routes {
            assign::side_from_text(route, text);
        }
        self.apply(r, key);
    }

    /// Carries out a reply (apply_reply) and notes it for the suggestions:
    /// a reply that added pending lines or wall edits starts a new batch.
    /// While no wall is the scope, one about the hands asks for Hands
    /// (R7) and one about the face or head for Head (`sections_for`, by
    /// the sources and the new lines' signals); a wall edit asks for Pong.
    fn apply(&mut self, r: assistant::Reply, key: Option<&fl::Key>) {
        let runs = matches!(r.intent.as_str(), "map" | "modify" | "replace");
        let scoped = self.scope.is_some();
        let sources: Vec<String> = if runs && !scoped { r.routes.iter().map(|x| assign::live_source(&x.source, None).0.to_string()).collect() } else { Vec::new() };
        let first_line = self.next_line;
        self.fresh = false;
        self.apply_reply(r, key);
        self.reply_news = true;
        if self.fresh {
            self.batch_id = self.next_batch_id;
            self.next_batch_id += 1;
            self.batch_news = true;
        }
        // the new lines' signals: a hand part (assign::midi_routes, a shape
        // trigger), a param route, a stand-in (arm.raise reads hand.y)
        let signals: Vec<&str> = if scoped { Vec::new() } else { self.pending.iter().filter(|m| m.line >= first_line).flat_map(|m| m.keys.iter().map(|k| k.0.as_str())).collect() };
        let (hands, head) = sections_for(&sources, &signals);
        self.open_hands |= hands;
        self.open_head |= head;
        self.open_pong |= scoped && self.fresh && !self.walls.is_empty();
    }

    /// Carries out a reply: new routes run as pending, the edit intents
    /// change the list.
    fn apply_reply(&mut self, r: assistant::Reply, key: Option<&fl::Key>) {
        self.mapped.clear();
        let t = i18n::text(self.lang);
        // a key anywhere in the frame ("left hand chords in A minor", "set
        // the scale to E minor") is the MIDI parts' key
        let mut key_line = None;
        let mut key_was = None;
        if let Some(w) = &self.writer {
            let mut store = w.player.store();
            if let Some(k) = crate::midi::key_from(&r.rest, store.key) {
                key_was = Some(store.key);
                store.key = k;
                w.update_midi(store);
                key_line = Some(fill(t.key_set, &[&crate::midi::key_name(k)]));
            }
        }
        // scoped to a Pong side, a sentence without a movement sets the side
        if let Some(lines) = self.scope.filter(|_| r.routes.iter().all(|x| x.source.is_empty())).and_then(|side| self.apply_wall(side, &r, key)) {
            self.mapped = lines;
            if let (Some(k), false) = (key_was, self.walls.is_empty()) {
                self.key_before.get_or_insert(k);
            }
            self.mapped.extend(key_line);
            return;
        }
        // a line makes its MIDI parts in the store: only for the intents
        // that run lines; unmap reads the routes as they are
        type Made = Vec<(Mapped, Vec<crate::route::Route>)>;
        let (made, routes): (Made, Vec<synthexis_parser::resolve::Route>) = if matches!(r.intent.as_str(), "map" | "modify" | "replace") {
            (r.routes.into_iter().flat_map(|route| self.make(route, key, t)).collect(), Vec::new())
        } else {
            (Vec::new(), r.routes)
        };
        match r.intent.as_str() {
            "map" if !made.is_empty() => {
                for (mut m, routes) in made {
                    self.propose(&mut m, routes);
                    self.pending.push(m);
                }
            }
            // a change of a route that runs: the old one goes (Discard
            // brings it back), the new one runs as pending
            "modify" | "replace" if made.iter().any(|(m, _)| !m.keys.is_empty()) => {
                let by_signal = r.intent == "replace";
                let keys: Vec<(String, Target)> = made.iter().flat_map(|(m, _)| m.keys.clone()).collect();
                let hit = |k: &(String, Target)| keys.iter().any(|x| x.0 == k.0 && (by_signal || x.1 == k.1));
                let (from_kept, from_pending) = self.take_matching(&hit);
                self.remove(&from_kept.iter().chain(&from_pending).flat_map(|m| m.ids.clone()).collect::<Vec<_>>());
                self.drop_parts(&from_pending);
                // kept routes come back on Discard; a pending one was never
                // kept, it only hands on what it had replaced
                let mut replaced: Vec<Mapped> = from_kept;
                replaced.extend(from_pending.into_iter().flat_map(|m| m.replaced));
                let mut first = true;
                for (mut m, routes) in made {
                    self.propose(&mut m, routes);
                    if first {
                        m.replaced = std::mem::take(&mut replaced);
                        first = false;
                    }
                    self.pending.push(m);
                }
            }
            "modify" | "replace" => {
                // nothing to run: show why (the note), not a bare line
                self.mapped = made.iter().map(|(m, _)| m.line(t)).collect();
                if self.mapped.is_empty() {
                    self.mapped = r.lines;
                }
            }
            "clear" => {
                let scoped = r.rest.iter().any(|t| t.starts_with("scope:") && t != "scope:all");
                if scoped {
                    self.mapped = vec![t.clear_scoped.into()];
                } else {
                    let n = self.pending.len() + self.kept.len();
                    let all: Vec<Mapped> = self.pending.drain(..).chain(self.kept.drain(..)).collect();
                    self.drop_routes(all);
                    self.mapped = vec![fill(t.removed_n, &[&n])];
                }
            }
            "undo" => {
                if self.pending.is_empty() && self.walls.is_empty() {
                    // the last Keep: its kept lines and what it did in the MIDI store
                    let last = self.kept.iter().map(|m| m.batch).chain(self.midi_kept.iter().map(|k| k.batch)).max();
                    let (gone, stay): (Vec<Mapped>, Vec<Mapped>) = self.kept.drain(..).partition(|m| Some(m.batch) == last);
                    self.kept = stay;
                    let (midi, stay): (Vec<MidiKept>, Vec<MidiKept>) = self.midi_kept.drain(..).partition(|k| Some(k.batch) == last);
                    self.midi_kept = stay;
                    let n = gone.len() + midi.iter().map(|k| k.binds.len() + k.walls.len()).sum::<usize>();
                    self.mapped = vec![fill(t.undid_n, &[&n])];
                    for mut m in gone {
                        self.remove(&m.ids);
                        for old in std::mem::take(&mut m.replaced) {
                            self.restore(old);
                        }
                    }
                    for k in midi {
                        self.unkeep(k);
                    }
                } else {
                    self.mapped = vec![fill(t.undid_n, &[&(self.pending.len() + self.walls.len())])];
                    self.discard();
                }
            }
            "unmap" => {
                // a movement named without its side means both sides; one
                // that is not live matches nothing
                let mut unknown = false;
                let mut sigs: Vec<String> = Vec::new();
                for route in &routes {
                    if route.source.is_empty() {
                        continue;
                    }
                    let side = route.side.as_deref();
                    match assign::signals_in(&route.source, side, t).or_else(|_| assign::signals_in(&route.source, Some("both"), t)) {
                        Ok(s) => sigs.extend(s),
                        Err(_) => unknown = true,
                    }
                }
                let targets: Vec<Target> = routes.iter().flat_map(|x| assign::targets(x, key)).collect();
                let names_target = routes.iter().any(|x| !x.targets.is_empty());
                if unknown {
                    self.mapped = vec![t.unmap_unknown_move.into()];
                } else if names_target && targets.is_empty() {
                    self.mapped = vec![t.unmap_unknown_target.into()];
                } else if sigs.is_empty() && targets.is_empty() {
                    self.mapped = vec![t.unmap_say_what.into()];
                } else {
                    let hit = |k: &(String, Target)| {
                        (sigs.is_empty() || sigs.contains(&k.0)) && (targets.is_empty() || targets.contains(&k.1))
                    };
                    let (kept, pending) = self.take_matching(&hit);
                    let n: usize = kept.iter().chain(&pending).map(|m| m.ids.len()).sum();
                    self.mapped = vec![fill(t.removed_n, &[&n])];
                    self.drop_routes(kept.into_iter().chain(pending).collect());
                }
            }
            "scale" if key_line.is_some() => {}
            _ => self.mapped = r.lines,
        }
        if let Some(k) = key_was {
            match r.intent.as_str() {
                // said for itself ("set the scale to E minor"): it stays
                "scale" => self.key_before = None,
                // with lines that run pending: Discard sets it back
                _ if !self.pending.is_empty() => {
                    self.key_before.get_or_insert(k);
                }
                _ => {}
            }
        }
        self.mapped.extend(key_line);
    }

    /// Takes the engine routes whose (signal, target) `hit` matches out of
    /// the kept and pending lists, one engine route at a time; a line that
    /// keeps some of its routes stays, marked. Returns what was taken from
    /// each list, with only the taken ids (and, for a whole line, what it
    /// had replaced); the caller removes them from the writer.
    fn take_matching(&mut self, hit: &dyn Fn(&(String, Target)) -> bool) -> (Vec<Mapped>, Vec<Mapped>) {
        let mut taken_from = [Vec::new(), Vec::new()];
        for (n, list) in [&mut self.kept, &mut self.pending].into_iter().enumerate() {
            let out = &mut taken_from[n];
            let mut stay = Vec::new();
            for mut m in list.drain(..) {
                let (take, keep): (Vec<Owned>, Vec<Owned>) =
                    m.ids.iter().copied().zip(m.keys.iter().cloned()).partition(|(_, k)| hit(k));
                if take.is_empty() {
                    stay.push(m);
                    continue;
                }
                let mut taken = m.clone_shallow();
                (taken.ids, taken.keys) = take.into_iter().unzip();
                // a MIDI part goes with its route: Keep binds only the rest
                let has = |keys: &[(String, Target)], p: &(String, crate::route::Trigger, u64)| {
                    keys.contains(&(p.0.clone(), Target::Part { id: p.2, trigger: p.1 }))
                };
                taken.parts.retain(|p| has(&taken.keys, p));
                if keep.is_empty() {
                    taken.replaced = std::mem::take(&mut m.replaced);
                } else {
                    (m.ids, m.keys) = keep.into_iter().unzip();
                    m.parts.retain(|p| has(&m.keys, p));
                    m.partly = true;
                    stay.push(m);
                }
                out.push(taken);
            }
            *list = stay;
        }
        let [kept, pending] = taken_from;
        (kept, pending)
    }

    /// Brings a kept line's taken routes back: into the line when it still
    /// holds the rest (its mark goes once it is whole), else as its own.
    fn restore(&mut self, old: Mapped) {
        let (mut back, routes) = Mapped::new(old.route.clone(), old.key.as_ref(), i18n::text(self.lang));
        let (routes, keys): (Vec<_>, Vec<_>) =
            routes.into_iter().zip(std::mem::take(&mut back.keys)).filter(|(_, k)| old.keys.contains(k)).unzip();
        back.keys = keys;
        back.line = old.line;
        self.propose(&mut back, routes);
        if let Some(w) = &self.writer {
            w.accept(&back.ids);
        }
        match self.kept.iter_mut().find(|m| m.line == old.line) {
            Some(m) => {
                m.ids.extend(back.ids);
                m.keys.extend(back.keys);
                if m.ids.len() >= m.full {
                    m.partly = false;
                }
            }
            None => {
                back.note = old.note;
                back.batch = old.batch;
                back.full = old.full;
                back.replaced = old.replaced;
                self.kept.push(back);
            }
        }
    }

    fn remove(&self, ids: &[u64]) {
        if let Some(w) = &self.writer {
            w.remove(ids);
        }
    }

    /// Removes lines that go without Keep (clear, unmap): their engine
    /// routes, and the MIDI parts of pending ones.
    fn drop_routes(&self, gone: Vec<Mapped>) {
        self.remove(&gone.iter().flat_map(|m| m.ids.clone()).collect::<Vec<_>>());
        self.drop_parts(&gone);
    }

    /// Drops the MIDI parts of lines that were never kept.
    fn drop_parts(&self, gone: &[Mapped]) {
        let ids: Vec<u64> = gone.iter().flat_map(|m| m.parts.iter().map(|p| p.2)).collect();
        if let (false, Some(w)) = (ids.is_empty(), &self.writer) {
            let mut store = w.player.store();
            store.parts.retain(|p| !ids.contains(&p.id));
            w.update_midi(store);
        }
    }

    /// A reply while scoped to `side`: clear and unmap set the side off,
    /// a target the sentence names becomes its action (assign::wall_route,
    /// wall_action). None for a reply that is no wall sentence (undo, a
    /// question without a target); it then goes the normal way.
    fn apply_wall(&mut self, side: Side, r: &assistant::Reply, key: Option<&fl::Key>) -> Option<Vec<String>> {
        let t = i18n::text(self.lang);
        let w = self.writer.clone()?;
        let mut store = w.player.store();
        let (action, note) = match r.intent.as_str() {
            "undo" => return None,
            "clear" | "unmap" => (Action::Off, assign::WallNote::default()),
            _ => {
                let route = assign::wall_route(&r.routes, &r.rest)?;
                let chans = self.bridge.map(|b| b.view().channels.clone()).unwrap_or_default();
                match assign::wall_action(&route, key, &mut store, &chans, t) {
                    Ok(x) => x,
                    Err(e) => return Some(vec![e]),
                }
            }
        };
        let made = match action {
            Action::Part { id } => Some(id),
            _ => None,
        };
        let mut setup = (*w.pong.setup()).clone();
        // a second sentence for the side replaces the first: its part goes,
        // the side's action before both stays what Discard restores
        let before = match self.walls.iter().position(|e| e.side == side) {
            Some(i) => {
                let old = self.walls.remove(i);
                store.parts.retain(|p| Some(p.id) != old.part);
                setup.auto_channel.retain(|p| Some(*p) != old.part);
                old.before
            }
            None => setup.wall(side).clone(),
        };
        // no FL channel list yet: the part gets a channel when it comes
        if let Some(id) = made.filter(|id| store.get(*id).is_some_and(|p| p.channel.is_none())) {
            setup.auto_channel.push(id);
        }
        w.update_midi(store);
        setup.walls[side.index()] = action.clone();
        w.pong.set_setup(setup);
        self.walls.push(WallEdit { side, before, action, note, part: made });
        self.fresh = true;
        Some(Vec::new())
    }

    /// Discard of the sides: each gets its action before back, the parts
    /// the sentences made go. A side changed since (by hand) stays.
    fn discard_walls(&mut self) {
        let edits = std::mem::take(&mut self.walls);
        let Some(w) = self.writer.as_ref().filter(|_| !edits.is_empty()) else { return };
        let mut setup = (*w.pong.setup()).clone();
        for e in &edits {
            if *setup.wall(e.side) == e.action {
                setup.walls[e.side.index()] = e.before.clone();
            }
        }
        drop_wall_parts(w, setup, edits.iter().filter_map(|e| e.part).collect(), Vec::new());
    }

    /// Keep of the sides: they stay, the scope ends, and a part a side
    /// played before goes when no finger and no side uses it any more.
    /// `midi` records it for undo.
    fn keep_walls(&mut self, midi: &mut MidiKept) {
        let edits = std::mem::take(&mut self.walls);
        if edits.is_empty() {
            return;
        }
        self.scope = None;
        let Some(w) = &self.writer else { return };
        let used = w.pong.setup().wall_parts();
        let mut store = w.player.store();
        let old: Vec<u64> = edits
            .iter()
            .filter_map(|e| match e.before {
                Action::Part { id } => Some(id),
                _ => None,
            })
            .filter(|id| !used.contains(id) && !store.binds.iter().any(|b| b.part == *id))
            .collect();
        midi.walls = edits.into_iter().map(|e| (e.side, e.before, e.action)).collect();
        if !old.is_empty() {
            midi.parts = store.parts.iter().filter(|p| old.contains(&p.id)).cloned().collect();
            store.parts.retain(|p| !old.contains(&p.id));
            w.update_midi(store);
        }
    }

    /// Takes back a Keep of the sides: each side that still does what was
    /// kept gets its action before, with the parts the Keep deleted; the
    /// parts the kept actions played go when nothing else uses them.
    fn unkeep_walls(&self, walls: &[(Side, Action, Action)], parts: Vec<crate::midi::Part>) {
        let Some(w) = self.writer.as_ref().filter(|_| !walls.is_empty()) else { return };
        let mut setup = (*w.pong.setup()).clone();
        for (side, before, kept) in walls {
            if setup.wall(*side) == kept {
                setup.walls[side.index()] = before.clone();
            }
        }
        let made = walls
            .iter()
            .filter_map(|(_, _, kept)| match kept {
                Action::Part { id } => Some(*id),
                _ => None,
            })
            .collect();
        drop_wall_parts(w, setup, made, parts);
    }

    /// The suggestions show and the chevron or Esc can fold them: unfolded
    /// with nothing pending.
    fn foldable(&self) -> bool {
        self.routes_open && self.has_rows() && !self.has_pending()
    }

    /// Keep: the pending routes stay as one batch; lines without an engine
    /// route (a question, "not live yet") go.
    fn keep(&mut self) {
        let batch = self.next_batch;
        self.next_batch += 1;
        let mut midi = MidiKept { batch, binds: Vec::new(), before: Vec::new(), key: self.key_before.take(), replaced: Vec::new(), walls: Vec::new(), parts: Vec::new() };
        self.keep_walls(&mut midi);
        let ids: Vec<u64> = self.pending.iter().flat_map(|m| m.ids.clone()).collect();
        if let Some(w) = &self.writer {
            w.accept(&ids);
        }
        for mut m in std::mem::take(&mut self.pending) {
            if !m.parts.is_empty() {
                // a MIDI line lives on as binds in the MIDI store, which the
                // project saves and the finger editor shows
                if let Some(w) = &self.writer {
                    let mut store = w.player.store();
                    for (sig, trigger, part) in &m.parts {
                        midi.before.extend(store.binds.iter().filter(|b| b.signal == *sig && b.trigger == *trigger).cloned());
                        store.bind(sig, *trigger, *part);
                        midi.binds.push(crate::midi::Bind { signal: sig.clone(), trigger: *trigger, part: *part });
                    }
                    // the line's routes become the bind routes, so a finger
                    // held while Keep is clicked plays on
                    w.keep_midi(&m.ids, store);
                }
                midi.replaced.append(&mut m.replaced);
                self.mapped = vec![i18n::text(self.lang).midi_kept.into()];
                continue;
            }
            if m.ids.is_empty() {
                // a line that runs nothing still gives back what it replaced
                for old in m.replaced {
                    self.restore(old);
                }
            } else {
                m.batch = batch;
                self.kept.push(m);
            }
        }
        if !midi.binds.is_empty() || midi.key.is_some() || !midi.replaced.is_empty() || !midi.walls.is_empty() {
            self.midi_kept.push(midi);
        }
    }

    /// Takes back what a Keep did in the MIDI store: its binds go with
    /// their parts; the binds they replaced, the key before and the kept
    /// lines its MIDI lines replaced come back.
    fn unkeep(&mut self, k: MidiKept) {
        self.unkeep_walls(&k.walls, k.parts);
        if let Some(w) = self.writer.as_ref().filter(|_| !k.binds.is_empty() || k.key.is_some()) {
            let mut store = w.player.store();
            store.binds.retain(|b| !k.binds.contains(b));
            for b in k.before.iter().filter(|b| !k.binds.contains(b)) {
                let free = !store.binds.iter().any(|x| x.signal == b.signal && x.trigger == b.trigger);
                if free && store.get(b.part).is_some() {
                    store.binds.push(b.clone());
                }
            }
            let used: Vec<u64> = store.binds.iter().map(|b| b.part).collect();
            store.parts.retain(|p| used.contains(&p.id) || !k.binds.iter().any(|b| b.part == p.id));
            if let Some(key) = k.key {
                store.key = key;
            }
            w.set_midi(store);
        }
        for old in k.replaced {
            self.restore(old);
        }
    }

    /// Discard: the pending routes go, the ones they replaced come back,
    /// and the key the sentences set.
    fn discard(&mut self) {
        self.discard_walls();
        let pending = std::mem::take(&mut self.pending);
        self.remove(&pending.iter().flat_map(|m| m.ids.clone()).collect::<Vec<_>>());
        self.drop_parts(&pending);
        for m in pending {
            for old in m.replaced {
                self.restore(old);
            }
        }
        if let (Some(k), Some(w)) = (self.key_before.take(), &self.writer) {
            let mut store = w.player.store();
            store.key = k;
            w.update_midi(store);
        }
    }

    /// Keep (the suggestions, Cmd+Enter): they fold. Nothing happens
    /// while `keep_blocked`.
    fn keep_now(&mut self) {
        if self.keep_blocked().is_some() {
            return;
        }
        self.keep();
        self.routes_open = false;
        self.save();
    }

    /// Discard (the suggestions, Cmd+Backspace): they fold.
    fn discard_now(&mut self) {
        self.discard();
        self.routes_open = false;
        self.save();
    }

    /// Something to show in the suggestions: reply lines, pending, wall
    /// edits or kept lines the drives lists do not show (`in_strip`).
    fn has_rows(&self) -> bool {
        !self.mapped.is_empty() || self.has_pending() || self.kept.iter().any(in_strip)
    }

    /// Pending lines or wall edits wait for Keep or Discard.
    fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.walls.is_empty()
    }

    /// Why Keep cannot act, None when it can: the pending lines exist but
    /// none of them runs an engine route or a part and no wall changed
    /// (a source the camera does not measure, "which hand?"). The reason
    /// is the first line's note.
    pub fn keep_blocked(&self) -> Option<String> {
        if self.pending.is_empty() || !self.walls.is_empty() || self.pending.iter().any(|m| !m.keys.is_empty()) {
            return None;
        }
        let t = i18n::text(self.lang);
        let why = self.pending.iter().map(|m| m.note.as_str()).find(|n| !n.is_empty()).unwrap_or(t.n_no_param);
        Some(fill(t.keep_none, &[&why]))
    }

    /// Whether the suggestions show in the top section: always while
    /// something is pending (until Keep or Discard), else while unfolded
    /// (the strip's chevron, a reply without a batch, unfold_routes).
    pub fn inline_shown(&self) -> bool {
        self.has_rows() && (self.has_pending() || self.routes_open)
    }

    /// The suggestions' rows: the reply lines, the pending lines with the
    /// flip button (a line that cannot run quieter), the wall edits, Keep
    /// and Discard, then "Kept" and the kept lines with their x while
    /// unfolded.
    fn inline_rows(&mut self, ui: &mut Ui) {
        let t = i18n::text(self.lang);
        let line = |ui: &mut Ui, text: &str, a: f32| {
            ui.add(egui::Label::new(RichText::new(text).size(Tok::BODY).color(white(a))).wrap());
        };
        for l in &self.mapped {
            line(ui, l, Tok::VALUE);
        }
        let mut flip = None;
        for (i, m) in self.pending.iter().enumerate() {
            ui.horizontal(|ui| {
                // a MIDI part's trigger has no inversion (assign::triggers)
                if !m.ids.is_empty() && m.parts.is_empty() && glass::quiet(ui, "⇅").on_hover_text(t.flip_hover).clicked() {
                    flip = Some(i);
                }
                line(ui, &m.line(t), if m.keys.is_empty() { Tok::HINT } else { Tok::STRONG });
            });
        }
        if let (Some(w), false) = (&self.writer, self.walls.is_empty()) {
            let store = w.player.store();
            for e in &self.walls {
                line(ui, &e.line(&store, t), Tok::STRONG);
            }
        }
        if let Some(m) = flip.and_then(|i| self.pending.get_mut(i)) {
            m.route.inv = !m.route.inv;
            if let Some(w) = &self.writer {
                w.flip(&m.ids);
            }
        }
        if self.has_pending() {
            let blocked = self.keep_blocked();
            let mut act = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Tok::GAP;
                let keep = glass::dim_button(ui, t.keep, blocked.is_none(), blocked.as_deref().unwrap_or_default());
                let keep = if blocked.is_none() { keep.on_hover_text(t.keys_note) } else { keep };
                if keep.clicked() {
                    act = Some(true);
                } else if glass::quiet(ui, t.discard).clicked() {
                    act = Some(false);
                }
            });
            match act {
                Some(true) => self.keep_now(),
                Some(false) => self.discard_now(),
                None => {}
            }
        }
        if !self.routes_open || !self.kept.iter().any(in_strip) {
            return;
        }
        line(ui, t.ov_kept_head, Tok::LABEL);
        let mut gone = None;
        for (i, m) in self.kept.iter().enumerate().filter(|(_, m)| in_strip(m)) {
            ui.horizontal(|ui| {
                if glass::quiet(ui, "×").on_hover_text(t.remove_hover).clicked() {
                    gone = Some(i);
                }
                line(ui, &m.line(t), Tok::VALUE);
            });
        }
        if let Some(i) = gone.filter(|i| *i < self.kept.len()) {
            let m = self.kept.remove(i);
            self.remove(&m.ids);
            self.save();
        }
    }

    fn submit(&mut self) {
        let text = self.doc.text.trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        let pins: Vec<Pin> = self
            .doc
            .chips
            .iter()
            .filter(|c| c.range.end <= text.len())
            .map(|c| to_pin(&c.range, &c.chip))
            .collect();
        let asked = self.applied.clone();
        let scope = self.applied.as_ref().map(|k| assistant::Scope {
            key: k.id(),
            plugin: k.plugin.clone(),
            params: self.classified.get(k).cloned().unwrap_or_default(),
            aliases: alias::of(&k.plugin),
        });
        // the strip says "Reading the sentence" until the reply is back
        self.mapped.clear();
        let rx = assistant::submit(&text, pins, scope);
        self.reply.push_back((rx, asked, text));
        self.doc.clear();
        self.cursor = 0;
        self.sugg.clear();
        self.list_open = false;
    }

    /// Polls (loader, state, FL, MIDI, reply); once per frame before the
    /// parts. `pin`: the pinned target's fl::Key id, empty while following
    /// FL; `lang`: the language setting, for the strings and the items.
    pub fn tick(&mut self, ctx: &egui::Context, pin: &RwLock<String>, lang: Lang) {
        self.lang = lang;
        self.poll_loader();
        self.sync_state();
        if let Some(e) = self.engine.as_mut().filter(|e| e.lang() != Some(lang)) {
            e.set_lang(Some(lang));
            self.refresh(self.enabled);
        }
        self.sync_fl(pin);
        self.sync_midi();
        self.poll_reply(ctx);
        // after sync_state: a loaded project's lines are never overwritten
        self.save();
    }

    /// The field (R4: `size`, 13 pt, centred text) with its ghost text,
    /// chip badges, completion list and variant panel. While it has the
    /// focus, Cmd+Enter keeps the pending routes and Cmd+Backspace
    /// discards them; Esc on the empty field folds the overlay, then ends
    /// the wall scope.
    pub fn field_ui(&mut self, ui: &mut Ui, size: Vec2, enabled: bool) {
        self.enabled = enabled;
        let t = i18n::text(self.lang);
        let id = Id::new(FIELD_ID);
        self.id = Some(id);

        let has_focus = ui.memory(|m| m.has_focus(id));
        if has_focus && self.has_pending() {
            let (keep, discard) = ui.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::Enter), i.consume_key(Modifiers::COMMAND, Key::Backspace)));
            if keep {
                self.keep_now();
            } else if discard {
                self.discard_now();
            }
        }
        if has_focus && self.warm_parser && self.last_warm.is_none_or(|t| t.elapsed() >= WARM_EVERY) {
            assistant::warm();
            self.last_warm = Some(std::time::Instant::now());
        }
        let active = enabled && has_focus && !self.sugg.is_empty();
        let at_end = self.cursor == self.doc.text.len();
        let ghost = active
            && !self.dismissed
            && self.sel == 0
            && at_end
            && self.sugg[0].ghost_worthy(&self.doc.text);
        // Esc on the empty field with nothing else open folds the
        // suggestions (never pending ones), then ends the scope
        let folds = self.foldable();
        let calm = !self.list_open && !self.panel_open() && self.doc.text.is_empty();
        if calm && (folds || self.scope.is_some()) && has_focus && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if folds {
                self.routes_open = false;
            } else {
                self.scope = None;
            }
        }
        let mut take = None;
        // with completion on the field keeps Tab (lock_focus below), so a
        // Tab with nothing to accept does nothing instead of inserting '\t'
        if enabled && has_focus && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) && (ghost || self.list_open) {
            take = Some(if self.list_open { self.sel } else { 0 });
        }
        if active {
            let n = self.sugg.len();
            let v = self.sugg[self.sel].variants;
            let panel = self.panel_open();
            // the finger step: once a side is picked, Up/Down go through the
            // fingers inside the panel instead of the list
            let fingers = panel && v.finger && (self.side.is_some() || !v.sided);
            ui.input_mut(|i| {
                if panel && i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.panel_closed = true;
                } else if (ghost || self.list_open) && i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.list_open = false;
                    self.dismissed = true;
                }
                // Left/Right pick the side at any moment while the panel is
                // open (it never locks) and never move the cursor then
                if panel && v.sided {
                    for (k, side) in [(Key::ArrowLeft, 0), (Key::ArrowRight, 1)] {
                        if i.consume_key(Modifiers::NONE, k) {
                            self.side = Some(side);
                            self.nav = true;
                        }
                    }
                }
                if fingers {
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        self.finger = Some(self.finger.map_or(0, |f| (f + 1).min(FINGERS.len() - 1)));
                        self.nav = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        self.finger = Some(self.finger.map_or(0, |f| f.saturating_sub(1)));
                        self.nav = true;
                    }
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    if self.list_open {
                        self.select((self.sel + 1).min(n - 1));
                    } else {
                        self.list_open = true;
                        self.dismissed = false;
                        self.select(0);
                    }
                }
                if self.list_open && i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    self.select(self.sel.saturating_sub(1));
                }
                if self.list_open && self.nav && i.consume_key(Modifiers::NONE, Key::Enter) {
                    take = Some(self.sel);
                }
            });
        }
        if let Some(i) = take {
            self.take(ui.ctx(), id, i);
        }

        let chip_bg = ui.visuals().selection.bg_fill.gamma_multiply(0.45);
        let chip_fg = ui.visuals().strong_text_color();
        let plain = ui.visuals().text_color();
        let font = glass::font(Tok::BODY);
        let chips: Vec<std::ops::Range<usize>> = self.doc.chips.iter().map(|c| c.range.clone()).collect();
        let doc_text = self.doc.text.clone();
        let mut layouter = |ui: &Ui, text: &str, _wrap: f32| {
            let mut job = LayoutJob::default();
            let fmt = |chip: bool| TextFormat {
                font_id: font.clone(),
                color: if chip { chip_fg } else { plain },
                background: if chip { chip_bg } else { Color32::TRANSPARENT },
                ..Default::default()
            };
            // chip ranges belong to the stored text; a fresh edit lays out plain once
            let mut at = 0;
            if text == doc_text {
                for r in &chips {
                    if r.start < at || !text.is_char_boundary(r.start) || !text.is_char_boundary(r.end) {
                        continue;
                    }
                    job.append(&text[at..r.start], 0.0, fmt(false));
                    job.append(&text[r.clone()], 0.0, fmt(true));
                    at = r.end;
                }
            }
            job.append(&text[at..], 0.0, fmt(false));
            ui.fonts(|f| f.layout_job(job))
        };
        let mut text = self.doc.text.clone();
        let out = TextEdit::singleline(&mut text)
            .id(id)
            .font(glass::font(Tok::BODY))
            .min_size(size)
            .margin(egui::Margin::symmetric(10, 7))
            .vertical_align(Align::Center)
            .desired_width((size.x - 20.0).max(24.0))
            // the field family of glass::text_field: the dither shows
            // through, the placeholder at HINT like the pickers beside it
            .background_color(glass::veil(Tok::FIELD))
            .hint_text(egui::RichText::new(t.placeholder).color(white(Tok::HINT)))
            .lock_focus(enabled)
            .layouter(&mut layouter)
            .show(ui);
        // the output's rect is the text area inside the margin
        self.field_rect = out.response.rect.expand2(egui::vec2(10.0, 7.0));
        // a small badge over the end of a chip with a picked finger or side
        if text == self.doc.text {
            for c in &self.doc.chips {
                let Some(b) = badge(&c.chip, t) else { continue };
                let ci = self.doc.text[..c.range.end.min(self.doc.text.len())].chars().count();
                let at = out.galley.pos_from_ccursor(CCursor::new(ci)).translate(out.galley_pos.to_vec2());
                let p = ui.painter().with_clip_rect(out.text_clip_rect.expand(8.0));
                let g = p.layout_no_wrap(b, glass::font(Tok::SMALL), ui.visuals().selection.stroke.color);
                let r = Rect::from_min_size(at.left_top() - egui::vec2(g.size().x + 4.0, 6.0), g.size() + egui::vec2(4.0, 0.0));
                p.rect_filled(r, 3.0, ui.visuals().selection.bg_fill);
                p.galley(r.min + egui::vec2(2.0, 0.0), g, Color32::PLACEHOLDER);
            }
        }

        let edited = text != self.doc.text;
        // a deletion that removed a whole chip moves the cursor itself
        let forced = if edited { self.doc.update(&text) } else { None };
        if let Some(c) = forced {
            set_cursor(ui.ctx(), id, &self.doc.text, c);
        }
        let cursor = forced
            .or_else(|| {
                out.cursor_range.map(|r| {
                    let ci = r.primary.ccursor.index;
                    self.doc.text.char_indices().nth(ci).map_or(self.doc.text.len(), |(b, _)| b)
                })
            })
            .unwrap_or(self.cursor)
            .min(self.doc.text.len());
        if cursor != self.cursor || edited {
            self.cursor = cursor;
            self.dismissed = false;
            self.refresh(enabled);
        }
        if !(enabled && out.response.has_focus()) {
            self.list_open = false;
        }
        if out.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.submit();
            ui.memory_mut(|m| m.request_focus(id));
        }

        // ghost text after the cursor at the end of the line
        let show_ghost = enabled
            && out.response.has_focus()
            && !self.dismissed
            && self.sel == 0
            && self.cursor == self.doc.text.len()
            && self.sugg.first().is_some_and(|s| s.ghost_worthy(&self.doc.text));
        if let (true, Some(s)) = (show_ghost, self.sugg.first()) {
            let mut ghost = s
                .ghost_suffix(&self.doc.text)
                .unwrap_or_else(|| format!("  → {}", s.chip.label()));
            // the choice Tab takes, or the one still open ("left/right?")
            if s.variants.any() {
                let (side, finger) = self.choice(0);
                let note = variant_note(s.variants, side, finger, t);
                ghost = if self.needs_choice(0) { format!("{ghost}  ({note})") } else { format!("{ghost}  · {note}") };
            }
            let pos = out.galley_pos + egui::vec2(out.galley.rect.right(), 0.0);
            ui.painter()
                .with_clip_rect(out.text_clip_rect)
                .text(pos, Align2::LEFT_TOP, ghost, glass::font(Tok::BODY), ui.visuals().weak_text_color());
        }

        // the list opens in 150 ms with an ease-out; selection and scrolling
        // glide in 80 ms
        let shown = enabled && self.list_open && !self.sugg.is_empty();
        self.list_shown = shown;
        // egui drops the focus on Esc before widgets see it; while there is
        // something to dismiss, the field keeps Esc (and Tab) for itself
        if out.response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: enabled,
                        // Esc on the empty field folds the suggestions or ends the scope (above)
                        escape: show_ghost || shown || (self.doc.text.is_empty() && (self.scope.is_some() || self.foldable())),
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    },
                )
            });
        }
        let open = ui
            .ctx()
            .animate_bool_with_time_and_easing(id.with("list"), shown, OPEN_SECS, easing::cubic_out);
        let ctx = ui.ctx().clone();
        if !shown {
            // the next list starts at its top row, without a glide
            ctx.animate_value_with_time(id.with("sel"), 0.0, 0.0);
            ctx.animate_value_with_time(id.with("scroll"), 0.0, 0.0);
            return;
        }
        let sel_y = ctx.animate_value_with_time(id.with("sel"), self.sel as f32, MOVE_SECS);
        let scroll_y = ctx.animate_value_with_time(id.with("scroll"), self.scroll as f32, MOVE_SECS);
        let width = self.field_rect.width().min(420.0);
        let rows = self.sugg.len().min(VISIBLE);
        let full = ROW_H * rows as f32 + 8.0;
        let pos = self.field_rect.left_bottom() + egui::vec2(0.0, 2.0);
        let mut clicked = None;
        let mut sel_rect = None;
        egui::Area::new(id.with("list-area"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .show(&ctx, |ui| {
                ui.set_clip_rect(Rect::from_min_size(pos, egui::vec2(width + 16.0, full * open + 8.0)));
                ui.multiply_opacity(open);
                glass::frosted(ui, egui::Margin::same(8), |ui| {
                    let (bx, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H * rows as f32), Sense::hover());
                    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(bx));
                    ui.set_clip_rect(bx.intersect(ui.clip_rect()));
                    let row = |y: f32| Rect::from_min_size(bx.min + egui::vec2(0.0, (y - scroll_y) * ROW_H), egui::vec2(width, ROW_H));
                    ui.painter().rect_filled(row(sel_y), 3.0, ui.visuals().selection.bg_fill);
                    sel_rect = Some(row(self.sel as f32).intersect(bx));
                    if self.sugg.len() > VISIBLE {
                        // where the visible rows sit in the whole list
                        let n = self.sugg.len() as f32;
                        let h = bx.height() * VISIBLE as f32 / n;
                        let y = bx.top() + bx.height() * scroll_y / n;
                        let bar = Rect::from_min_size(egui::pos2(bx.right() - 2.0, y), egui::vec2(2.0, h));
                        ui.painter().rect_filled(bar, 1.0, ui.visuals().weak_text_color().gamma_multiply(0.6));
                    }
                    for (i, s) in self.sugg.iter().enumerate() {
                        let rect = row(i as f32);
                        if !rect.intersects(bx) {
                            continue;
                        }
                        let resp = ui.interact(rect.intersect(bx), id.with(("row", i)), Sense::click());
                        let p = ui.painter();
                        p.text(
                            rect.left_center() + egui::vec2(6.0, 0.0),
                            Align2::LEFT_CENTER,
                            s.chip.label(),
                            glass::font(Tok::BODY),
                            ui.visuals().strong_text_color(),
                        );
                        p.text(
                            rect.right_center() - egui::vec2(10.0, 0.0),
                            Align2::RIGHT_CENTER,
                            hint(s, t),
                            glass::font(Tok::SMALL),
                            if i == self.sel {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().weak_text_color()
                            },
                        );
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        let list = Rect::from_min_size(pos, egui::vec2(width + 16.0, full + 8.0));
        if let Some(r) = sel_rect.filter(|r| r.height() > 0.0) {
            self.variant_panel(&ctx, ui, id, list, r, open, t);
        }
        if let Some(i) = clicked {
            self.take(ui.ctx(), id, i);
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Row 2 of the chat column (R9): a chevron that folds the suggestions
    /// under it (not while something is pending: those stay until Keep or
    /// Discard) and the summary joined by " · " (the wall scope, then the
    /// first of the suggested routes, the assistant's status and the kept
    /// routes) and the scope's x. Keep and Discard sit in the suggestions
    /// (`routes_inline`). Empty when there is nothing to show. Returns the
    /// strip's rect.
    pub fn routes_strip(&mut self, ui: &mut Ui, width: f32) -> Rect {
        let t = i18n::text(self.lang);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, Tok::CTRL_H), Sense::hover());
        let pending = self.pending.len() + self.walls.len();
        let mut parts: Vec<String> = Vec::new();
        if let Some(side) = self.scope {
            parts.push(wall_title(side, t).to_string());
        }
        if pending > 0 {
            parts.push(fill(t.ov_pending, &[&pending]));
        } else if !self.reply.is_empty() {
            parts.push(t.ov_reading.to_string());
        } else if self.kept.iter().any(in_strip) {
            parts.push(fill(t.ov_kept, &[&self.kept.iter().filter(|m| in_strip(m)).count()]));
        }
        let rows = self.has_rows();
        if parts.is_empty() && !rows {
            return rect;
        }
        let mut ui = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
        ui.spacing_mut().item_spacing.x = Tok::GAP;
        // the buttons keep their room; the summary is cut to the rest
        let button_w = |ui: &Ui, s: &str| glass::galley(ui, s, Tok::BODY).size().x + 16.0 + Tok::GAP;
        let reserved = if self.scope.is_some() { button_w(&ui, "×") } else { 0.0 };
        let chevron = if rows { 16.0 } else { 0.0 };
        let summary = glass::fit(&ui, &parts.join(" · "), Tok::BODY, (width - chevron - reserved - 4.0).max(0.0));
        let g = glass::galley(&ui, &summary, Tok::BODY);
        // pending suggestions stay open: the chevron folds only the rest
        let folds = rows && pending == 0;
        let sense = if folds { Sense::click() } else { Sense::hover() };
        let (r, resp) = ui.allocate_exact_size(egui::vec2(chevron + g.size().x, Tok::CTRL_H), sense);
        if folds && resp.clicked() {
            self.routes_open = !self.routes_open;
        }
        let lit = resp.hovered() && folds;
        if rows {
            let turn = ui.ctx().animate_bool_with_time(Id::new(ROUTES_ID).with("turn"), self.inline_shown(), 0.15);
            glass::chevron_at(ui.painter(), egui::pos2(r.left() + 4.0, r.center().y), 4.0, turn, white(if lit { Tok::VALUE } else { Tok::HINT }));
        }
        glass::text_at(ui.painter(), egui::pos2(r.left() + chevron, r.center().y - g.size().y / 2.0), g, white(Tok::VALUE));
        if folds {
            resp.on_hover_text(if self.routes_open { t.ov_hide } else { t.ov_show });
        }
        if self.scope.is_some() && glass::quiet(&mut ui, "×").on_hover_text(t.w_drop_scope).clicked() {
            self.scope = None;
        }
        rect
    }

    /// The suggestions inline in the top section, under the strip, as
    /// wide as `ui`: an inset group with the reply lines, the pending lines
    /// with Keep and Discard, and the kept lines while unfolded. At most
    /// `max_h` high; taller, it scrolls inside. The top section grows by
    /// its height and pushes the sections down (the panel scrolls); there
    /// is no overlay over them. Nothing while `inline_shown` is false.
    /// Returns the rect it took (Rect::NOTHING for none).
    pub fn routes_inline(&mut self, ui: &mut Ui, max_h: f32) -> Rect {
        if std::mem::take(&mut self.batch_news) {
            self.reply_news = false;
        } else if std::mem::take(&mut self.reply_news) {
            self.routes_open = true;
        }
        if !self.inline_shown() {
            return Rect::NOTHING;
        }
        let pad = egui::vec2(8.0, 6.0);
        let inner_w = (ui.available_width() - 2.0 * pad.x).max(0.0);
        let out = egui::Frame::NONE
            .fill(white(Tok::INSET_FILL))
            .corner_radius(egui::CornerRadius::same(Tok::R_TOGGLE))
            .inner_margin(egui::Margin::symmetric(pad.x as i8, pad.y as i8))
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.spacing_mut().item_spacing.y = 2.0;
                egui::ScrollArea::vertical()
                    .id_salt(ROUTES_ID)
                    .max_height((max_h - 2.0 * pad.y).max(Tok::CTRL_H))
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.inline_rows(ui));
            });
        out.response.rect
    }

    /// Unfolds the suggestions from outside (a click on a node in the
    /// Hands Routes view, R19): the kept lines show too.
    pub fn unfold_routes(&mut self) {
        self.routes_open = true;
    }

    /// True once after a reply with a hand route (a parameter, a Signal or
    /// a part, pending; a stand-in such as arm.raise counts by its hand
    /// signal) while no wall was the scope (R7).
    pub fn take_open_hands(&mut self) -> bool {
        std::mem::take(&mut self.open_hands)
    }

    /// True once after a reply with a face, eye, mouth or head route while
    /// no wall was the scope.
    pub fn take_open_head(&mut self) -> bool {
        std::mem::take(&mut self.open_head)
    }

    /// True once after a reply that set a Pong wall (the wall scope).
    pub fn take_open_pong(&mut self) -> bool {
        std::mem::take(&mut self.open_pong)
    }

    /// The routes from the hands for the node view (R19): one edge per
    /// (signal, target) of the pending and kept lines on a `cam0.hand.`
    /// signal. `focus`: a pending line drives a parameter or a Signal from
    /// a hand, or the field holds a parameter chip. `batch`: the pending
    /// batch's id, 0 when nothing is pending.
    pub fn routing(&self) -> Routing {
        self.routing_of(|s| s.starts_with(HAND_SIGNAL))
    }

    /// The routes from the face and head for the Head section, as
    /// `routing` does for the hands: the pending and kept lines on a
    /// `cam0.mouth.`, `cam0.face.`, `cam0.eye.` or `cam0.head.` signal.
    pub fn face_routing(&self) -> Routing {
        self.routing_of(crate::preset::head_signal)
    }

    /// What `target` of line `m` is in the node view and the drives
    /// lists; `store` caches the MIDI store a part's name comes from.
    fn node_target(&self, m: &Mapped, target: &Target, store: &mut Option<Option<crate::midi::Store>>) -> NodeTarget {
        match target {
            Target::Param { plugin, index } => NodeTarget::Param {
                plugin: m.key.as_ref().map_or_else(|| plugin.plugin.clone(), |k| k.plugin.clone()),
                name: param_name(&m.route, *index),
            },
            Target::Macro(n) => NodeTarget::Macro(*n),
            Target::Part { id, .. } => {
                let s = store.get_or_insert_with(|| self.writer.as_ref().map(|w| w.player.store()));
                let name = s.as_ref().and_then(|s| s.get(*id)).map(|p| p.name.clone()).unwrap_or_default();
                NodeTarget::Part { id: *id, name }
            }
        }
    }

    /// The `x` of a kept row in a drives list (fingers.rs param_list): the
    /// kept engine routes from `signal` to `target` go from the engine and
    /// the saved project; a line that keeps other routes stays, marked
    /// "partly removed". Pending lines are left alone. Returns how many
    /// engine routes went.
    pub fn unmap_kept(&mut self, signal: &str, target: &NodeTarget) -> usize {
        let gone = self.take_kept(signal, target);
        if gone.is_empty() {
            return 0;
        }
        let n = gone.iter().map(|m| m.ids.len()).sum();
        self.drop_routes(gone);
        self.save();
        n
    }

    /// Takes the kept engine routes from `signal` to `target` out of the
    /// kept lines (`take_matching`); pending lines stay. The caller removes
    /// them from the writer.
    fn take_kept(&mut self, signal: &str, target: &NodeTarget) -> Vec<Mapped> {
        let mut store = None;
        let mut hits: Vec<(String, Target)> = Vec::new();
        for m in &self.kept {
            for (sig, tg) in m.keys.iter().filter(|(s, _)| s == signal) {
                if self.node_target(m, tg, &mut store) == *target {
                    hits.push((sig.clone(), tg.clone()));
                }
            }
        }
        if hits.is_empty() {
            return Vec::new();
        }
        let pending = std::mem::take(&mut self.pending);
        let (gone, _) = self.take_matching(&|k| hits.contains(k));
        self.pending = pending;
        gone
    }

    /// A box of the Hands Routes view picked something else (param_find):
    /// the kept engine routes from `signal` to `target` start from another
    /// hand movement (`Found::Move`) or drive another parameter or Signal,
    /// in the engine and the saved project. Each becomes a kept line of its
    /// own in the old line's batch, with its range, curve, smoothing and
    /// gates, on the signal's hand only; the old line keeps its other
    /// routes, marked "partly removed". A route that would not run stays
    /// as it was. Returns how many engine routes the new lines run.
    pub fn retarget_kept(&mut self, signal: &str, target: &NodeTarget, to: &crate::param_find::Found) -> usize {
        let gone = self.take_kept(signal, target);
        if gone.is_empty() {
            return 0;
        }
        self.remove(&gone.iter().flat_map(|m| m.ids.clone()).collect::<Vec<_>>());
        let t = i18n::text(self.lang);
        let mut n = 0;
        for mut old in gone {
            let Some((mut m, routes)) = Self::retargeted(&old, signal, to, t) else {
                self.restore(old);
                continue;
            };
            m.batch = old.batch;
            m.replaced = std::mem::take(&mut old.replaced);
            m.line = self.next_line;
            self.next_line += 1;
            self.propose(&mut m, routes);
            if let Some(w) = &self.writer {
                w.accept(&m.ids);
            }
            n += m.keys.len();
            self.kept.push(m);
        }
        self.save();
        n
    }

    /// Line `old` (the routes from `signal` that `take_kept` took) made
    /// again with `to` in place of its target or its movement and the
    /// side of `signal`, with only the routes that stand for the old ones.
    /// None when none of them would run.
    fn retargeted(old: &Mapped, signal: &str, to: &crate::param_find::Found, t: &Text) -> Option<(Mapped, Vec<crate::route::Route>)> {
        use crate::param_find::Found;
        use synthexis_parser::resolve::{Bound, Target as T, Via};
        let mut route = old.route.clone();
        let hand = signal.split('.').nth(2).filter(|h| *h == "L" || *h == "R");
        match hand {
            Some("L") => route.side = Some("left".into()),
            Some(_) => route.side = Some("right".into()),
            None => {}
        }
        // the new signal or the new target the old routes stand for
        let (sig, tg): (Option<String>, Option<Target>) = match to {
            Found::Move(feat) => {
                let h = hand?;
                route.source = format!("hand.{feat}");
                (Some(format!("cam0.hand.{h}.{feat}")), None)
            }
            Found::Param { key, index, name } => {
                let picked = fl::Key::parse(key)?;
                // the old target's own instance when it is the same plugin
                let plugin = old
                    .keys
                    .iter()
                    .find_map(|(_, tg)| match tg {
                        Target::Param { plugin, .. } if plugin.plugin == picked.plugin => Some(plugin.clone()),
                        _ => None,
                    })
                    .unwrap_or(picked);
                let bound = Bound { index: *index, name: name.clone(), via: Via::Chip, plugin: Some(plugin.id()) };
                route.targets = vec![T { token: "T:param".into(), text: name.clone(), bound: Some(bound) }];
                (None, Some(Target::Param { plugin, index: *index }))
            }
            Found::Macro(i) => {
                route.targets = vec![T { token: format!("T:macro:{}", i + 1), text: format!("Signal {}", i + 1), bound: None }];
                (None, Some(Target::Macro(*i)))
            }
        };
        let want: Vec<(String, Target)> = old.keys.iter().map(|(s, x)| (sig.clone().unwrap_or_else(|| s.clone()), tg.clone().unwrap_or_else(|| x.clone()))).collect();
        let (mut m, routes) = Mapped::new(route, old.key.as_ref(), t);
        let (routes, keys): (Vec<_>, Vec<_>) = routes.into_iter().zip(std::mem::take(&mut m.keys)).filter(|(_, k)| want.contains(k)).unzip();
        if keys.is_empty() {
            return None;
        }
        m.keys = keys;
        m.full = m.keys.len();
        Some((m, routes))
    }

    /// `routing` over the signals `from` takes.
    fn routing_of(&self, from: impl Fn(&str) -> bool) -> Routing {
        let mut store = None;
        let mut edges: Vec<EdgeIn> = Vec::new();
        let lines = self.pending.iter().map(|m| (m, true)).chain(self.kept.iter().map(|m| (m, false)));
        for (m, pending) in lines {
            for (signal, target) in m.keys.iter().filter(|(s, _)| from(s)) {
                let target = self.node_target(m, target, &mut store);
                if !edges.iter().any(|e| e.signal == *signal && e.target == target) {
                    edges.push(EdgeIn { signal: signal.clone(), target, pending });
                }
            }
        }
        let drives = |m: &Mapped| m.keys.iter().any(|(s, t)| from(s) && matches!(t, Target::Param { .. } | Target::Macro(_)));
        let focus = self.pending.iter().any(drives) || self.doc.chips.iter().any(|c| matches!(c.chip, Chip::Param { .. }));
        let batch = if self.pending.is_empty() && self.walls.is_empty() { 0 } else { self.batch_id };
        Routing { edges, focus, batch }
    }

    /// The selected row's choice, right of the list (always: the plugin
    /// window has one fixed width that fits list and panel): the hand row,
    /// and for a bare "finger" the finger row once a side is picked. The
    /// current value is highlighted; a click picks it, and a click that
    /// completes the choice makes the chip.
    #[allow(clippy::too_many_arguments)]
    fn variant_panel(&mut self, ctx: &egui::Context, ui: &Ui, id: Id, list: Rect, row: Rect, t: f32, tx: &Text) {
        if !self.panel_open() {
            return;
        }
        let v = self.sugg[self.sel].variants;
        let fingers = v.finger && (self.side.is_some() || !v.sided);
        let at = egui::pos2(list.right() + 4.0, row.top() - 6.0);
        let hint = match (v.sided, fingers) {
            (true, true) => tx.keys_both,
            (true, false) => tx.keys_side,
            _ => tx.keys_finger,
        };
        let mut picked = None;
        egui::Area::new(id.with("variant-area"))
            .order(Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                ui.multiply_opacity(t);
                glass::frosted(ui, egui::Margin::same(8), |ui| {
                    let mut axis = |ui: &mut Ui, name: &str, vals: &[&str], cur: Option<usize>, k: usize| {
                        ui.horizontal(|ui| {
                            ui.add_sized([44.0, 22.0], egui::Label::new(RichText::new(name).weak().size(Tok::BODY)));
                            for (j, val) in vals.iter().enumerate() {
                                if ui.selectable_label(cur == Some(j), RichText::new(*val).size(Tok::BODY)).clicked() {
                                    picked = Some((k, j));
                                }
                            }
                        });
                    };
                    if v.sided {
                        axis(ui, tx.panel_hand, &tx.sides(), self.side, 0);
                    }
                    if fingers {
                        axis(ui, tx.panel_finger, &tx.fingers(), self.finger, 1);
                    }
                    ui.label(RichText::new(hint).weak().size(Tok::SMALL));
                });
            });
        if let Some((k, j)) = picked {
            match k {
                0 => self.side = Some(j),
                _ => self.finger = Some(j),
            }
            self.nav = true;
            if !self.needs_choice(self.sel) {
                self.accept(ctx, id, self.sel);
            }
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nih_plug_egui::egui::{Event, Pos2, RawInput};
    use synthexis_complete::{Gesture, Param};

    use super::*;

    fn engine() -> Engine {
        let g = |source: &str, phrase: &str| Gesture { source: source.into(), shape: None, phrase: phrase.into(), pol: 1 };
        let (de, en) = (Some(Lang::De), Some(Lang::En));
        let mut e = Engine::new(
            vec![
                g("hand.spread", "spread my fingers"),
                g("hand.fist", "Faust schließen"),
                g("hand.finger.index", "Finger"),
                g("hand.fist", "close my fist"),
                g("hand.finger.index", "finger"),
            ],
            vec![("filter.cutoff".into(), "Cutoff".into()), ("filter.cutoff".into(), "cutoff".into())],
            &HashMap::new(),
        )
        .with_langs(vec![en, de, de, en, en], vec![de, en], Vec::new())
        .with_sided(vec!["hand.fist".into(), "hand.finger.index".into()]);
        let p = |index: u32, name: &str, class: &str| Param { index, name: name.into(), class: class.into() };
        e.set_plugin("Synth/Test", vec![p(0, "Filter 1 Freq", "filter.cutoff"), p(1, "Filter 2 Freq", "filter.cutoff")]);
        e
    }

    fn key(k: Key) -> Event {
        cmd(k, Modifiers::NONE)
    }

    fn cmd(k: Key, modifiers: Modifiers) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    /// The harness window, the default size of the new window.
    const W: f32 = crate::editor::WIN_W as f32;
    const H: f32 = crate::editor::WIN_H as f32;

    /// The picker width of the top section grid (spec 2.5, v3 2.11).
    fn picker_w() -> f32 {
        (W * 0.19).round().clamp(136.0, 220.0)
    }

    /// The top section as the new window lays it out (spec 2.5): picker,
    /// field, then the note and the strip, and the suggestions under them.
    fn top(ui: &mut Ui, s: &mut Sentence, pin: &RwLock<String>, lang: Lang) -> Rect {
        s.tick(ui.ctx(), pin, lang);
        let p = picker_w();
        let chat = W - 2.0 * Tok::INSET - 2.0 * p - 2.0 * Tok::GAP - 34.0;
        let row = |y: f32, h: f32| UiBuilder::new().max_rect(Rect::from_min_size(egui::pos2(Tok::INSET, y), egui::vec2(W - 2.0 * Tok::INSET, h))).layout(Layout::left_to_right(Align::Center));
        ui.scope_builder(row(12.0, Tok::TOP_H), |ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            s.picker_ui(ui, egui::vec2(p, Tok::TOP_H), pin);
            s.field_ui(ui, egui::vec2(chat, Tok::TOP_H), true);
        });
        let strip = ui
            .scope_builder(row(48.0, Tok::CTRL_H), |ui| {
                ui.spacing_mut().item_spacing.x = Tok::GAP;
                s.note_ui(ui, p);
                s.routes_strip(ui, chat)
            })
            .inner;
        let area = Rect::from_min_size(egui::pos2(Tok::INSET, 78.0), egui::vec2(W - 2.0 * Tok::INSET, 200.0));
        ui.scope_builder(UiBuilder::new().max_rect(area).layout(Layout::top_down(Align::Min)), |ui| s.routes_inline(ui, 200.0));
        strip
    }

    /// Source and side of the first chip; empty when it is no gesture.
    fn gesture_of(h: &Harness) -> (String, Option<String>) {
        match h.s.doc.chips.first().map(|c| &c.chip) {
            Some(Chip::Gesture { source, side, .. }) => (source.clone(), side.clone()),
            _ => (String::new(), None),
        }
    }

    struct Harness {
        ctx: egui::Context,
        s: Sentence,
        picked: RwLock<String>,
        lang: Lang,
        strip: Rect,
        out: Option<egui::FullOutput>,
    }

    impl Harness {
        /// English, the default setting.
        fn new() -> Self {
            Self::with(Lang::En)
        }

        /// German: German and English items, German strings.
        fn de() -> Self {
            Self::with(Lang::De)
        }

        fn with(lang: Lang) -> Self {
            let mut s = Sentence::new(None, None, None);
            s.loader = None;
            s.engine = Some(engine());
            s.labels = Labels::Fixed(Arc::new(sense::Classes::default()));
            let mut h = Self { ctx: egui::Context::default(), s, picked: RwLock::new(String::new()), lang, strip: Rect::NOTHING, out: None };
            h.frame(vec![]);
            let id = h.s.id;
            h.ctx.memory_mut(|m| id.into_iter().for_each(|id| m.request_focus(id)));
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let raw = RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(W, H))),
                ..Default::default()
            };
            let (s, picked, lang) = (&mut self.s, &self.picked, self.lang);
            let mut strip = Rect::NOTHING;
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| strip = top(ui, s, picked, lang));
            }));
            self.strip = strip;
        }

        /// Centre of the topmost visible text containing `needle` (inside
        /// its clip rect, so a row cut off by a too small list is missing).
        fn find(&self, needle: &str) -> Option<Pos2> {
            fn walk(s: &egui::Shape, needle: &str) -> Option<Pos2> {
                match s {
                    egui::Shape::Text(t) if t.galley.text().contains(needle) => {
                        Some(t.pos + t.galley.rect.center().to_vec2())
                    }
                    egui::Shape::Vec(v) => v.iter().rev().find_map(|s| walk(s, needle)),
                    _ => None,
                }
            }
            let shapes = &self.out.as_ref()?.shapes;
            shapes.iter().rev().find_map(|c| walk(&c.shape, needle).filter(|p| c.clip_rect.contains(*p)))
        }

        /// Clicks the text like a user, then lets animations settle.
        fn click(&mut self, needle: &str) -> bool {
            let Some(at) = self.find(needle) else { return false };
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            for _ in 0..20 {
                self.frame(vec![]);
            }
            true
        }

        fn popup_open(&self) -> bool {
            self.ctx.memory(|m| m.any_popup_open())
        }

        fn focused(&self) -> bool {
            self.s.id.is_some_and(|id| self.ctx.memory(|m| m.has_focus(id)))
        }

        fn chips(&self) -> Vec<String> {
            self.s.doc.chips.iter().map(|c| c.chip.id()).collect()
        }

        /// The colour of the topmost text containing `needle`.
        fn color_of(&self, needle: &str) -> Option<Color32> {
            fn walk(s: &egui::Shape, needle: &str) -> Option<Color32> {
                match s {
                    egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.override_text_color.unwrap_or(t.fallback_color)),
                    egui::Shape::Vec(v) => v.iter().rev().find_map(|s| walk(s, needle)),
                    _ => None,
                }
            }
            self.out.as_ref()?.shapes.iter().rev().find_map(|c| walk(&c.shape, needle))
        }
    }

    #[test]
    fn tab_accepts_ghost_and_keeps_focus() {
        let mut h = Harness::new();
        assert!(h.focused());
        h.frame(vec![Event::Text("when I spr".into())]);
        assert_eq!(h.s.doc.text, "when I spr");
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(h.s.doc.text, "when I spread my fingers ");
        assert_eq!(h.chips(), ["gesture:hand.spread"]);
        h.frame(vec![]);
        assert!(h.focused(), "Tab moved the focus out of the field");
        h.frame(vec![Event::Text("the cut".into())]);
        assert_eq!(h.s.doc.text, "when I spread my fingers the cut");
    }

    #[test]
    fn arrows_open_list_and_enter_picks() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("the cut".into())]);
        h.frame(vec![key(Key::ArrowDown)]);
        assert!(h.s.list_open);
        h.frame(vec![key(Key::ArrowDown)]);
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(h.s.doc.text, "the Filter 2 Freq ");
        assert_eq!(h.chips(), ["param:Synth/Test#1"]);
        assert!(h.focused());
    }

    #[test]
    fn list_opens_while_typing_and_enter_still_sends() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("the cut".into())]);
        assert!(h.s.list_open);
        assert_eq!(h.s.sugg.len(), 2);
        h.frame(vec![key(Key::Enter)]);
        assert!(h.chips().is_empty());
        assert_eq!(h.s.doc.text, "", "Enter without an arrow key sends the sentence");
    }

    #[test]
    fn list_scrolls_past_five_rows() {
        let mut h = Harness::new();
        let p = |index: u32| Param { index, name: format!("Filter {index} Freq"), class: "filter.cutoff".into() };
        if let Some(e) = h.s.engine.as_mut() {
            e.set_plugin("Synth/Test", (0..10).map(p).collect());
        }
        h.frame(vec![Event::Text("the fil".into())]);
        assert_eq!(h.s.sugg.len(), TOP);
        for _ in 0..6 {
            h.frame(vec![key(Key::ArrowDown)]);
        }
        assert_eq!((h.s.sel, h.s.scroll), (6, 2));
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(h.s.doc.text, "the Filter 6 Freq ");
    }

    #[test]
    fn esc_dismisses_and_space_never_accepts() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("when I spr".into())]);
        h.frame(vec![key(Key::Escape)]);
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(h.s.doc.text, "when I spr");
        h.frame(vec![Event::Text(" ".into())]);
        assert_eq!(h.s.doc.text, "when I spr ");
        assert!(h.chips().is_empty());
    }

    #[test]
    fn tab_with_nothing_to_accept_inserts_nothing() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("zzz".into())]);
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(h.s.doc.text, "zzz");
        assert!(h.focused());
    }

    /// An unmapped plugin: the classifier runs param-sense as soon as the
    /// bridge has the names, before any frame of the field; the next frame
    /// hands the classes to the engine.
    #[test]
    fn unmapped_plugin_is_classified_when_the_names_arrive() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        let labels = Arc::new(sense::Classes::load(&root.join("../assistant/completion")).0);
        h.s.labels = Labels::Fixed(labels.clone());
        let (tx, rx) = crossbeam_channel::unbounded();
        h.s.sensed = Some(rx);
        std::thread::spawn(move || classify_loop(b, move || labels.clone(), &[], &tx));
        let key = fl::Key { at: fl::At::Fx { track: 2, slot: 0 }, plugin: "Totally Unknown Synth".into() };
        let names: Vec<(u32, String)> =
            ["VCF Cutoff", "VCF Resonance", "Amp Attack", "Delay Mix"].iter().enumerate().map(|(i, n)| (i as u32, n.to_string())).collect();
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = vec![fl::Target { key: key.clone(), track: 2, track_name: String::new() }];
            v.focus = Some(key.clone());
        });
        h.frame(vec![]);
        b.set_params(key.clone(), Ok(names));
        // no frame in between: the classes arrive on their own
        let waiting = h.s.sensed.as_ref().map(|r| r.len());
        for _ in 0..400 {
            if h.s.sensed.as_ref().is_some_and(|r| !r.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.s.sensed.as_ref().is_some_and(|r| r.len() == 1), "classes not ready (had {waiting:?})");
        h.frame(vec![]);
        assert_eq!(h.s.applied, Some(key.clone()));
        let classes: Vec<String> = h.s.classified[&key].iter().map(|p| p.class.clone()).collect();
        assert_eq!(classes[0], "filter.cutoff");
        assert!(classes.iter().all(|c| !c.is_empty()), "{classes:?}");
    }

    /// A map sentence runs as pending, Keep keeps it, the arrow flips it,
    /// undo removes the last kept routes, a sentence without a side asks.
    #[test]
    fn routes_run_pending_then_keep_flip_undo() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let route = |side: Option<&str>| R {
            source: "hand.openness".into(),
            side: side.map(str::to_string),
            inv: false,
            gates: Vec::new(),
            mods: Vec::new(),
            targets: vec![T {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: Some(Bound { index: 12, name: "Amp Drive".into(), via: Via::Name, plugin: None }),
            }],
        };
        let reply = |intent: &str, routes| assistant::Reply { intent: intent.into(), rest: Vec::new(), routes, lines: vec!["x".into()] };
        h.s.apply(reply("map", vec![route(Some("left"))]), Some(&key));
        assert_eq!(h.s.pending.len(), 1);
        assert_eq!(h.s.pending[0].line(&i18n::EN), "left hand openness -> Topos / Amp Drive");
        assert_eq!(writer.assigned().iter().map(|a| a.1).collect::<Vec<_>>(), [true]);
        h.frame(vec![]);
        let under_strip = |h: &Harness, s: &str| h.find(s).is_some_and(|p| p.y > h.strip.bottom());
        assert!(under_strip(&h, "Keep") && under_strip(&h, "Discard"), "Keep and Discard sit in the suggestions under the strip {:?}", h.strip);
        assert!(h.click("Keep"));
        h.frame(vec![]);
        assert!(h.s.pending.is_empty() && h.s.kept.len() == 1);
        assert_eq!(writer.assigned().iter().map(|a| a.1).collect::<Vec<_>>(), [false]);
        // no side: a line that asks, no engine route
        h.s.apply(reply("map", vec![route(None)]), Some(&key));
        assert!(h.s.pending[0].line(&i18n::EN).contains("which hand"), "{}", h.s.pending[0].line(&i18n::EN));
        assert_eq!(writer.assigned().len(), 1);
        h.frame(vec![]);
        assert!(h.click("Discard"));
        h.frame(vec![]);
        // flip a pending route
        h.s.apply(reply("map", vec![route(Some("right"))]), Some(&key));
        h.frame(vec![]);
        assert!(h.find("⇅").is_some_and(|p| p.y > h.strip.bottom()), "the flip button sits in the suggestions");
        assert!(h.click("⇅"));
        h.frame(vec![]);
        assert!(h.s.pending[0].line(&i18n::EN).contains("inverted"));
        assert_eq!(writer.assigned().iter().map(|a| a.2).collect::<Vec<_>>(), [false, true]);
        // undo drops the pending ones first, then the last kept
        h.s.apply(reply("undo", Vec::new()), Some(&key));
        h.s.apply(reply("undo", Vec::new()), Some(&key));
        assert!(h.s.pending.is_empty() && h.s.kept.is_empty());
        assert!(writer.assigned().is_empty());
    }

    /// modify replaces the running route (Discard brings it back), undo
    /// takes back a whole Keep, unmap takes one hand of a both-hands line,
    /// a scoped clear removes nothing, replies apply in the order sent.
    #[test]
    fn edits_change_the_running_routes() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let route = |source: &str, side: &str, index: u32, mods: &[&str]| R {
            source: source.into(),
            side: Some(side.into()),
            inv: false,
            gates: Vec::new(),
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![T {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: Some(Bound { index, name: format!("P{index}"), via: Via::Name, plugin: None }),
            }],
        };
        let reply = |intent: &str, routes, rest: &[&str]| assistant::Reply {
            intent: intent.into(),
            rest: rest.iter().map(|t| t.to_string()).collect(),
            routes,
            lines: vec!["x".into()],
        };
        // one Keep of two routes, then a second Keep
        h.s.apply(reply("map", vec![route("hand.openness", "left", 12, &[]), route("hand.y", "right", 13, &[])], &[]), Some(&key));
        h.s.keep();
        h.s.apply(reply("map", vec![route("hand.y", "both", 14, &[])], &[]), Some(&key));
        h.s.keep();
        assert_eq!(writer.assigned().len(), 4);
        // unmap the right hand from P14: the left hand's half stays
        h.s.apply(reply("unmap", vec![route("hand.y", "right", 14, &[])], &[]), Some(&key));
        assert_eq!(writer.assigned().len(), 3);
        assert!(h.s.kept.last().is_some_and(|m| m.ids.len() == 1 && m.line(&i18n::EN).contains("partly removed")));
        // undo: the whole last Keep goes
        h.s.apply(reply("undo", Vec::new(), &[]), Some(&key));
        assert_eq!(writer.assigned().len(), 2);
        // modify the drive route: the old one goes, the new one is pending
        h.s.apply(reply("modify", vec![route("hand.openness", "left", 12, &["smooth:low"])], &[]), Some(&key));
        assert_eq!(writer.assigned().len(), 2);
        assert_eq!(h.s.kept.len(), 1);
        assert!(h.s.pending[0].line(&i18n::EN).contains("snappy"));
        // Discard brings the old one back as kept
        h.s.discard();
        assert_eq!(h.s.kept.len(), 2);
        assert!(writer.assigned().iter().all(|a| !a.1));
        // a clear scoped to one source removes nothing; "clear" removes all
        h.s.apply(reply("clear", Vec::new(), &["S:hand.y", "scope:source"]), Some(&key));
        assert_eq!(writer.assigned().len(), 2);
        h.s.apply(reply("clear", Vec::new(), &[]), Some(&key));
        assert!(writer.assigned().is_empty() && h.s.kept.is_empty());
        // Keep drops a line that made no engine route
        let mut no_side = route("hand.openness", "left", 12, &[]);
        no_side.side = None;
        h.s.apply(reply("map", vec![no_side], &[]), Some(&key));
        h.s.keep();
        assert!(h.s.kept.is_empty());
    }

    /// Review round 2: modify chains, modify of a pending line, undo of a
    /// Keep that replaced, partial modify brought back, unmap without a side.
    #[test]
    fn replace_chains_and_undo_restore_the_state_before() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let route = |source: &str, side: Option<&str>, mods: &[&str]| R {
            source: source.into(),
            side: side.map(str::to_string),
            inv: false,
            gates: Vec::new(),
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![T {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: Some(Bound { index: 12, name: "Drive".into(), via: Via::Name, plugin: None }),
            }],
        };
        let reply = |intent: &str, routes| assistant::Reply { intent: intent.into(), rest: Vec::new(), routes, lines: vec!["x".into()] };
        let lines = |h: &Harness| h.s.kept.iter().map(|m| m.line(&i18n::EN)).collect::<Vec<_>>();
        h.s.apply(reply("map", vec![route("hand.openness", Some("left"), &[])]), Some(&key));
        h.s.keep();
        // two modifies, then Discard: the original comes back
        h.s.apply(reply("modify", vec![route("hand.openness", Some("left"), &["smooth:low"])]), Some(&key));
        h.s.apply(reply("modify", vec![route("hand.openness", Some("left"), &["smooth:high"])]), Some(&key));
        assert_eq!(h.s.pending.len(), 1);
        h.s.discard();
        assert_eq!(lines(&h), ["left hand openness -> Topos / Drive"]);
        assert_eq!(writer.assigned().len(), 1);
        // modify kept, Keep, undo: the original comes back
        h.s.apply(reply("modify", vec![route("hand.openness", Some("left"), &["smooth:low"])]), Some(&key));
        h.s.keep();
        assert!(lines(&h)[0].contains("snappy"));
        h.s.apply(reply("undo", Vec::new()), Some(&key));
        assert_eq!(lines(&h), ["left hand openness -> Topos / Drive"]);
        assert_eq!(writer.assigned().len(), 1);
        // a pending line modified and discarded is gone, not kept
        h.s.apply(reply("map", vec![route("hand.y", Some("right"), &[])]), Some(&key));
        h.s.apply(reply("modify", vec![route("hand.y", Some("right"), &["smooth:low"])]), Some(&key));
        h.s.discard();
        assert_eq!(h.s.kept.len(), 1);
        assert_eq!(writer.assigned().len(), 1);
        // both hands: modify one hand, Discard merges it back into the line
        h.s.apply(reply("map", vec![route("hand.x", Some("both"), &[])]), Some(&key));
        h.s.keep();
        h.s.apply(reply("modify", vec![route("hand.x", Some("right"), &["smooth:low"])]), Some(&key));
        assert!(h.s.kept[1].line(&i18n::EN).contains("partly removed"));
        h.s.discard();
        assert_eq!(h.s.kept.len(), 2);
        assert!(!h.s.kept[1].line(&i18n::EN).contains("partly"), "{}", h.s.kept[1].line(&i18n::EN));
        assert_eq!(h.s.kept[1].ids.len(), 2);
        // unmap of a movement that is not live removes nothing; without a
        // side it means both
        h.s.apply(reply("unmap", vec![route("motion.circle", None, &[])]), Some(&key));
        assert_eq!(writer.assigned().len(), 3);
        h.s.apply(reply("unmap", vec![route("hand.x", None, &[])]), Some(&key));
        assert_eq!(writer.assigned().len(), 1);
    }

    /// Kept lines are saved with the project and come back running after
    /// a reload; a partly removed line keeps only the rest; a second
    /// initialize() starts nothing twice.
    #[test]
    fn kept_routes_survive_a_reload() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let store = Arc::new(RwLock::new(String::new()));
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        h.s.store = Some(store.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let route = |source: &str, side: &str| R {
            source: source.into(),
            side: Some(side.into()),
            inv: true,
            gates: Vec::new(),
            mods: vec!["hi:0.8".into()],
            targets: vec![T {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: Some(Bound { index: 3, name: "Amp Drive".into(), via: Via::Chip, plugin: None }),
            }],
        };
        let reply = |intent: &str, routes| assistant::Reply { intent: intent.into(), rest: Vec::new(), routes, lines: vec!["x".into()] };
        h.s.apply(reply("map", vec![route("hand.openness", "left"), route("hand.x", "both")]), Some(&key));
        h.s.keep();
        h.s.apply(reply("unmap", vec![route("hand.x", "right")]), Some(&key));
        h.frame(vec![]);
        assert!(!store.read().is_empty());
        // reload: a new plugin instance, initialize() twice, then the window
        let again = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let lines = saved::load(&store.read());
        saved::run(&lines, &again);
        saved::run(&lines, &again);
        assert_eq!(again.assigned().len(), 2, "openness + the left hand of x");
        assert!(again.assigned().iter().all(|a| !a.1 && a.2), "kept and inverted");
        let back = Sentence::new(None, Some(again.clone()), Some(store.clone()));
        let shown: Vec<String> = back.kept.iter().map(|m| m.line(&i18n::EN)).collect();
        assert_eq!(shown.len(), 2);
        assert!(shown[1].contains("partly removed"), "{shown:?}");
        assert_eq!(back.kept.iter().map(|m| m.ids.len()).sum::<usize>(), 2);
        assert!(back.next_line > back.kept.iter().map(|m| m.line).max().unwrap_or(0));
    }

    /// MIDI lines: a route with a note and a parameter is two lines, undo
    /// takes back a MIDI Keep (and what it replaced), replace takes one
    /// hand's part, a key said with a line goes with Discard and undo, and
    /// only the intents that run lines make parts.
    #[test]
    fn midi_lines_keep_undo_and_go_like_the_others() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let drive = || T { token: "T:param:fx.drive".into(), text: "drive".into(), bound: Some(Bound { index: 12, name: "Drive".into(), via: Via::Name, plugin: None }) };
        let note = || T { token: "T:midi.note:".into(), text: String::new(), bound: None };
        let route = |side: &str, targets: Vec<T>| R {
            source: "hand.pinch".into(),
            side: Some(side.into()),
            inv: false,
            gates: Vec::new(),
            mods: vec!["note:c3".into()],
            targets,
        };
        let reply = |intent: &str, routes, rest: &[&str]| assistant::Reply {
            intent: intent.into(),
            rest: rest.iter().map(|t| t.to_string()).collect(),
            routes,
            lines: vec!["x".into()],
        };
        let store = || writer.player.store();
        // a note and a parameter: the parameter runs too; no flip on the MIDI line
        h.s.apply(reply("map", vec![route("right", vec![note(), drive()])], &[]), Some(&key));
        assert_eq!(h.s.pending.len(), 2);
        assert!(h.s.pending[1].keys.iter().any(|k| matches!(k.1, Target::Param { index: 12, .. })));
        h.frame(vec![]);
        assert_eq!(texts(&h).iter().filter(|x| *x == "⇅").count(), 1);
        let line = h.s.pending[0].ids.clone();
        h.s.keep();
        assert_eq!((h.s.kept.len(), store().binds.len()), (1, 1));
        // the MIDI line's route lives on as the bind route (a held finger plays on)
        assert!(writer.assigned().iter().any(|(id, pending, _)| line.contains(id) && !pending));
        // undo after a MIDI Keep takes back that Keep, then the one before
        h.s.apply(reply("map", vec![route("left", vec![note()])], &[]), Some(&key));
        h.s.keep();
        h.s.apply(reply("undo", Vec::new(), &[]), Some(&key));
        assert_eq!((h.s.kept.len(), store().binds.len()), (1, 1));
        h.s.apply(reply("undo", Vec::new(), &[]), Some(&key));
        assert!(h.s.kept.is_empty() && store().binds.is_empty() && store().parts.is_empty());
        // a MIDI line that replaced a kept one gives it back on undo
        h.s.apply(reply("map", vec![route("right", vec![drive()])], &[]), Some(&key));
        h.s.keep();
        h.s.apply(reply("replace", vec![route("right", vec![note()])], &[]), Some(&key));
        assert!(h.s.kept.is_empty());
        h.s.keep();
        h.s.apply(reply("undo", Vec::new(), &[]), Some(&key));
        assert_eq!((h.s.kept.len(), store().binds.len()), (1, 0));
        h.s.apply(reply("clear", Vec::new(), &[]), Some(&key));
        // replace takes the right hand's part of a pending both-hands line
        h.s.apply(reply("map", vec![route("both", vec![note()])], &[]), Some(&key));
        h.s.apply(reply("replace", vec![route("right", vec![drive()])], &[]), Some(&key));
        h.s.keep();
        assert_eq!(store().binds.iter().map(|b| b.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.pinch"]);
        assert_eq!(store().parts.len(), 1);
        let left = store().binds.clone();
        // a key with a line: Discard and undo set it back, the old bind returns
        let before = store().key;
        h.s.apply(reply("map", vec![route("left", vec![note()])], &["key:e", "scale:major"]), Some(&key));
        assert_ne!(store().key, before);
        h.s.discard();
        assert_eq!((store().key, store().parts.len()), (before, 1));
        h.s.apply(reply("map", vec![route("left", vec![note()])], &["key:e", "scale:major"]), Some(&key));
        h.s.keep();
        assert_ne!(store().key, before);
        h.s.apply(reply("undo", Vec::new(), &[]), Some(&key));
        assert_eq!((store().key, store().binds.clone(), store().parts.len()), (before, left, 1));
        // unmap makes no part; clear drops a pending line's part
        h.s.apply(reply("unmap", vec![route("right", vec![note()])], &[]), Some(&key));
        assert_eq!(store().parts.len(), 1);
        h.s.apply(reply("map", vec![route("right", vec![note()])], &[]), Some(&key));
        assert_eq!(store().parts.len(), 2);
        h.s.apply(reply("clear", Vec::new(), &[]), Some(&key));
        assert_eq!(store().parts.len(), 1);
    }

    /// A route line and its notes in German: words translated, movement
    /// ids and parameter names as they are.
    #[test]
    fn route_lines_follow_the_language() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let r = |side: Option<&str>| R {
            source: "hand.openness".into(),
            side: side.map(str::to_string),
            inv: true,
            gates: Vec::new(),
            mods: vec!["hi:0.8".into(), "smooth:low".into()],
            targets: vec![T {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: Some(Bound { index: 3, name: "Amp Drive".into(), via: Via::Chip, plugin: None }),
            }],
        };
        let (m, _) = Mapped::new(r(Some("left")), Some(&key), &i18n::DE);
        assert_eq!(m.line(&i18n::DE), "hand openness (links) umgekehrt -> Topos / Amp Drive (Bereich 0-80 %, direkt)");
        assert_eq!(m.line(&i18n::EN), "left hand openness inverted -> Topos / Amp Drive (range 0-80 %, snappy)");
        let (m, _) = Mapped::new(r(None), Some(&key), &i18n::DE);
        assert!(m.line(&i18n::DE).ends_with("welche Hand? sag links oder rechts"), "{}", m.line(&i18n::DE));
    }

    #[test]
    fn picker_follows_fl_and_pins() {
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        let key = |t, s, p: &str| fl::Key { at: fl::At::Fx { track: t, slot: s }, plugin: p.into() };
        let (portal, proq) = (key(0, 1, "Portal"), key(3, 0, "Pro-Q 3"));
        let target = |k: &fl::Key| {
            let fl::At::Fx { track, .. } = k.at else { return None };
            Some(fl::Target { key: k.clone(), track, track_name: String::new() })
        };
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = [&portal, &proq].into_iter().filter_map(target).collect();
            v.focus = Some(portal.clone());
            v.params = Some((portal.clone(), Ok(vec![(93, "MASTER DRY/WET".into())])));
        });
        h.frame(vec![]);
        assert_eq!(h.s.want, Some(portal.clone()));
        let engine = h.s.engine.as_ref().map(|e| e.plugin().map(str::to_string));
        assert_eq!(engine, Some(Some("fx:0:1:Portal".into())));
        assert_eq!(h.s.applied, Some(portal.clone()));

        // pinned: focus moves on, the target stays
        *h.picked.write() = portal.id();
        b.update(|v| v.focus = Some(proq.clone()));
        h.frame(vec![]);
        assert_eq!(h.s.want, Some(portal.clone()));
        let drawn = texts(&h);
        assert!(!drawn.iter().any(|x| x == "Pin" || x == "Unpin"), "no Pin or Unpin: {drawn:?}");

        // the list's top row follows FL's focus again
        assert!(h.click("Portal"));
        assert!(h.popup_open());
        assert!(h.click(i18n::EN.follow_fl));
        assert!(h.picked.read().is_empty() && !h.popup_open());
        h.frame(vec![]);
        assert_eq!(h.s.want, Some(proq.clone()));
        *h.picked.write() = portal.id();
        h.frame(vec![]);
        assert_eq!(h.s.want, Some(portal.clone()));

        // the pinned plugin leaves its slot: pin dropped, follow FL again
        b.update(|v| v.targets = target(&proq).into_iter().collect());
        h.frame(vec![]);
        assert!(h.picked.read().is_empty());
        assert_eq!(h.s.want, Some(proq.clone()));
        assert_eq!(h.s.applied, None, "Pro-Q 3 names not sent yet");
    }

    #[test]
    fn the_picker_sits_left_of_the_field_on_its_row() {
        let mut h = Harness::new();
        h.frame(vec![]);
        let (button, field) = (h.s.picker_button, h.s.field_rect);
        let hint = h.find(i18n::EN.placeholder);
        assert!(button.is_positive(), "picker not drawn");
        let Some(hint) = hint else { return assert!(hint.is_some(), "field hint not drawn") };
        assert!(button.y_range().contains(hint.y), "one row: button {button:?}, field text at {hint:?}");
        assert!((button.width() - picker_w()).abs() < 0.5 && (button.height() - Tok::TOP_H).abs() < 0.5, "picker {button:?}");
        assert!((field.height() - Tok::TOP_H).abs() < 0.5, "field {field:?}");
        assert!(field.left() > button.right() && field.contains(hint), "the field right of the picker: {field:?}");
    }

    #[test]
    fn the_chat_field_is_the_glass_field_with_a_hint_placeholder() {
        let h = Harness::new();
        let field = h.s.field_rect;
        fn fills(s: &egui::Shape, out: &mut Vec<(Rect, Color32)>) {
            match s {
                egui::Shape::Rect(r) => out.push((r.rect, r.fill)),
                // the placeholder's own colour (its galley's), as a point
                egui::Shape::Text(t) if t.galley.text() == i18n::EN.placeholder => {
                    let c = t.galley.job.sections.first().map_or(Color32::TRANSPARENT, |s| s.format.color);
                    out.push((Rect::NOTHING, c));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| fills(s, out)),
                _ => {}
            }
        }
        let mut rects = Vec::new();
        for c in h.out.iter().flat_map(|o| &o.shapes) {
            fills(&c.shape, &mut rects);
        }
        assert!(rects.contains(&(Rect::NOTHING, white(Tok::HINT))), "placeholder at HINT: {rects:?}");
        let back = rects.iter().find(|(r, _)| r.is_positive() && (r.min - field.min).length() < 2.0 && (r.max - field.max).length() < 2.0);
        assert_eq!(back.map(|(_, f)| *f), Some(glass::veil(Tok::FIELD)), "field {field:?} in {rects:?}");
    }

    #[test]
    fn the_picker_shows_plugin_picker_then_the_followed_then_the_pinned_name() {
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        h.frame(vec![]);
        assert_eq!(h.color_of(i18n::EN.plugin_picker), Some(white(Tok::HINT)), "the placeholder");
        let key = |t, s, p: &str| fl::Key { at: fl::At::Fx { track: t, slot: s }, plugin: p.into() };
        let (portal, proq) = (key(0, 1, "Portal"), key(3, 0, "Pro-Q 3"));
        let target = |k: &fl::Key| fl::Target { key: k.clone(), track: 0, track_name: String::new() };
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = vec![target(&portal), target(&proq)];
            v.focus = Some(portal.clone());
        });
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(h.find(i18n::EN.plugin_picker).is_none());
        assert_eq!(h.color_of("Portal"), Some(white(Tok::HINT)), "the followed plugin, quieter than a pin");
        *h.picked.write() = proq.id();
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(h.find("Portal").is_none());
        assert_eq!(h.color_of("Pro-Q 3"), Some(white(Tok::VALUE)), "the pinned plugin");
    }

    #[test]
    fn picker_lists_every_track_in_columns_and_rows_pin() {
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        let fx = |track, slot, p: &str| fl::Target {
            key: fl::Key { at: fl::At::Fx { track, slot }, plugin: p.into() },
            track,
            track_name: String::new(),
        };
        let serum = fl::Target {
            key: fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() },
            track: 0,
            track_name: "Master".into(),
        };
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = vec![fx(0, 0, "ValhallaSupermassive"), fx(0, 1, "Portal"), fx(3, 0, "Pro-Q 3"), serum];
        });
        h.frame(vec![]);
        assert!(h.click(i18n::EN.plugin_picker), "picker button not drawn");
        assert!(h.popup_open());
        // nothing folds: every track and every plugin shows at once
        for row in ["Master", "Sound", "Serum 2", "FX 0", "ValhallaSupermassive", "FX 1", "Portal", "Track 3", "Pro-Q 3"] {
            assert!(h.find(row).is_some(), "{row} not visible in the list: {:?}", texts(&h));
        }
        // Master's sound sits above its FX
        let y = |s: &str| h.find(s).map_or(f32::NAN, |p| p.y);
        assert!(y("Serum 2") < y("ValhallaSupermassive") && y("ValhallaSupermassive") < y("Portal"));
        let list = h.ctx.memory(|m| m.area_rect(Id::new(PICKER_ID).with("popup")));
        let button = h.s.picker_button;
        let win = Rect::from_min_size(Pos2::ZERO, egui::vec2(W, H));
        assert!(list.is_some_and(|r| r.width() >= button.width().max(200.0) - 0.5 && win.contains_rect(r)), "list {list:?}, button {button:?}");
        assert!(h.click("Portal"));
        assert_eq!(*h.picked.read(), "fx:0:1:Portal");
        assert!(!h.popup_open(), "picking a row closes the list");
    }

    /// A big project (Noa's demo: ten tracks of an instrument and two to
    /// four effects) wraps into columns that stay inside the window; every
    /// plugin is drawn.
    #[test]
    fn a_big_project_wraps_into_columns_inside_the_window() {
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        let mut targets = Vec::new();
        for track in 1..=10u32 {
            targets.push(fl::Target { key: fl::Key { at: fl::At::Gen { ch: track }, plugin: format!("Synth {track}") }, track, track_name: String::new() });
            for slot in 0..(2 + track % 3) {
                targets.push(fl::Target { key: fl::Key { at: fl::At::Fx { track, slot }, plugin: format!("Fx {track}.{slot}") }, track, track_name: String::new() });
            }
        }
        let names: Vec<String> = targets.iter().map(|t| t.key.plugin.clone()).collect();
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = targets;
        });
        h.frame(vec![]);
        assert!(h.click(i18n::EN.plugin_picker));
        h.frame(vec![]);
        // more than one column, a track's plugins in one column
        let x = |h: &Harness, s: &str| h.find(s).map_or(f32::NAN, |p| p.x);
        assert!(x(&h, "Synth 1") < x(&h, "Synth 2"), "tracks side by side");
        assert!((x(&h, "Synth 2") - x(&h, "Fx 2.3")).abs() < PICK_COL_W / 4.0, "a track stays in its column");
        let list = h.ctx.memory(|m| m.area_rect(Id::new(PICKER_ID).with("popup")));
        let win = Rect::from_min_size(Pos2::ZERO, egui::vec2(W, H)).shrink(glass::POP_MARGIN - 0.5);
        assert!(list.is_some_and(|r| win.contains_rect(r)), "{list:?}");
        // the wheel scrolls the columns sideways until every plugin showed
        let over = list.map_or(Pos2::ZERO, |r| r.center());
        let mut seen: Vec<&String> = names.iter().filter(|n| h.find(n).is_some()).collect();
        assert!(seen.len() < names.len(), "the columns are wider than the window");
        for _ in 0..30 {
            h.frame(vec![Event::PointerMoved(over), Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -120.0), modifiers: Modifiers::NONE }]);
            seen.extend(names.iter().filter(|n| h.find(n).is_some()));
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), names.len(), "every plugin reachable");
        assert!(h.popup_open(), "scrolling keeps the list open");
    }

    /// Noa (v3 follow-up): Left/Right page the big picker's columns inside
    /// the list; the keys reach nothing else while it is open.
    #[test]
    fn arrow_keys_page_the_picker_columns() {
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        let mut targets = Vec::new();
        for track in 1..=10u32 {
            targets.push(fl::Target { key: fl::Key { at: fl::At::Gen { ch: track }, plugin: format!("Synth {track}") }, track, track_name: String::new() });
            for slot in 0..(2 + track % 3) {
                targets.push(fl::Target { key: fl::Key { at: fl::At::Fx { track, slot }, plugin: format!("Fx {track}.{slot}") }, track, track_name: String::new() });
            }
        }
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = targets;
        });
        h.frame(vec![]);
        assert!(h.click(i18n::EN.plugin_picker));
        h.frame(vec![]);
        let hx = Id::new(PICKER_ID).with("popup").with("hx");
        let offset = |h: &Harness| h.ctx.data(|d| d.get_temp::<(f32, f32)>(hx)).unwrap_or((f32::NAN, f32::NAN));
        let (x0, room) = offset(&h);
        assert_eq!(x0, 0.0);
        assert!(room > PICK_COL_W, "the columns overflow: {room}");
        assert!(h.find("Synth 1").is_some() && h.find("Synth 10").is_none());
        let (text, cursor) = (h.s.doc.text.clone(), h.s.cursor);
        h.frame(vec![key(Key::ArrowRight)]);
        h.frame(vec![]);
        assert_eq!(offset(&h).0, PICK_COL_W + Tok::GAP, "one column");
        for _ in 0..30 {
            h.frame(vec![key(Key::ArrowRight)]);
        }
        h.frame(vec![]);
        assert!((offset(&h).0 - room).abs() < 0.5, "Right stops at the end: {:?}", offset(&h));
        assert!(h.find("Synth 10").is_some() && h.find("Fx 10.0").is_some(), "the last track shows");
        for _ in 0..30 {
            h.frame(vec![key(Key::ArrowLeft)]);
        }
        h.frame(vec![]);
        assert_eq!(offset(&h).0, 0.0, "Left goes back to the start");
        assert!(h.find("Synth 1").is_some());
        assert!(h.popup_open(), "paging keeps the list open");
        assert_eq!((h.s.doc.text.clone(), h.s.cursor, h.s.side), (text, cursor, None), "the keys went to the list only");
    }

    #[test]
    fn left_right_pick_the_hand_not_the_cursor() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich die Fau".into())]);
        h.frame(vec![key(Key::ArrowDown)]);
        assert!(h.s.panel_open());
        assert_eq!(h.s.side, None, "no default side");
        h.frame(vec![]);
        let drawn = (h.find("Faust"), h.find("rechts"));
        assert!(matches!(drawn, (Some(row), Some(panel)) if panel.x > row.x), "panel drawn right of the list: {drawn:?}");
        h.frame(vec![key(Key::ArrowRight)]);
        assert_eq!(h.s.cursor, "wenn ich die Fau".len(), "Right moved the cursor");
        assert_eq!(h.s.side, Some(1));
        h.frame(vec![key(Key::ArrowLeft)]);
        assert_eq!(h.s.side, Some(0));
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.fist".into(), Some("left".into())));
        let c = &h.s.doc.chips[0];
        assert!(matches!(to_pin(&c.range, &c.chip).what, PinWhat::Gesture { side: Some(ref s), .. } if s == "left"));
        assert_eq!(badge(&c.chip, &i18n::DE).as_deref(), Some("L"));
    }

    #[test]
    fn tab_without_a_side_opens_the_panel_and_inserts_nothing() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich die Fau".into())]);
        assert!(h.s.needs_choice(0));
        h.frame(vec![key(Key::Tab)]);
        assert!(h.s.doc.chips.is_empty());
        assert_eq!(h.s.doc.text, "wenn ich die Fau");
        assert!(h.s.panel_open());
        h.frame(vec![key(Key::Tab)]);
        assert!(h.s.doc.chips.is_empty(), "Tab again still needs a side");
        h.frame(vec![key(Key::ArrowRight)]);
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(gesture_of(&h), ("hand.fist".into(), Some("right".into())));
    }

    #[test]
    fn a_side_named_right_before_is_preset() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich die rechte Fau".into())]);
        assert_eq!(h.s.side, Some(1));
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(gesture_of(&h), ("hand.fist".into(), Some("right".into())));
        // "beide" is no side the panel offers: the choice stays open
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich beide Fau".into())]);
        assert!(h.s.needs_choice(0));
    }

    #[test]
    fn a_bare_finger_takes_side_then_finger_and_the_side_never_locks() {
        let mut h = Harness::de();
        // two items: the list is open already, "Finger" selected
        h.frame(vec![Event::Text("wenn ich die Fin".into())]);
        assert!(h.s.panel_open());
        h.frame(vec![]);
        assert!(h.find("Daumen").is_none(), "fingers show only after the side");
        h.frame(vec![key(Key::ArrowLeft)]);
        h.frame(vec![]);
        assert!(h.find("Daumen").is_some(), "fingers show once a side is picked");
        h.frame(vec![key(Key::ArrowDown)]);
        h.frame(vec![key(Key::ArrowDown)]);
        assert_eq!((h.s.sel, h.s.finger), (0, Some(1)), "Up/Down move the fingers, not the list");
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.finger.index".into(), Some("left".into())));
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich die Fin".into())]);
        for k in [Key::ArrowLeft, Key::ArrowDown, Key::ArrowDown, Key::ArrowDown] {
            h.frame(vec![key(k)]);
        }
        h.frame(vec![key(Key::ArrowRight)]);
        assert_eq!((h.s.side, h.s.finger), (Some(1), Some(2)), "Right switches the hand in the finger step");
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.finger.middle".into(), Some("right".into())));
        assert_eq!(badge(&h.s.doc.chips[0].chip, &i18n::DE).as_deref(), Some("Mittel R"));
        assert_eq!(badge(&h.s.doc.chips[0].chip, &i18n::EN).as_deref(), Some("middle R"));
    }

    #[test]
    fn esc_closes_the_panel_then_the_list() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("wenn ich die Fin".into())]);
        h.frame(vec![key(Key::ArrowLeft)]);
        h.frame(vec![key(Key::Escape)]);
        assert!(h.s.list_open && !h.s.panel_open());
        h.frame(vec![key(Key::ArrowDown)]);
        assert_eq!(h.s.sel, 1, "Up/Down move through the list again");
        h.frame(vec![key(Key::Escape)]);
        assert!(!h.s.list_open);
    }

    #[test]
    fn arrows_move_the_cursor_on_rows_without_variants() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("the cut".into())]);
        assert!(h.s.list_open);
        h.frame(vec![key(Key::ArrowLeft)]);
        assert_eq!(h.s.cursor, "the cu".len());
    }

    /// Every text the last frame drew.
    fn texts(h: &Harness) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in h.out.iter().flat_map(|o| &o.shapes) {
            walk(&c.shape, &mut out);
        }
        out
    }

    #[test]
    fn english_mode_offers_no_german_item_and_speaks_english() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("wenn ich die Fau".into())]);
        assert!(h.s.sugg.is_empty(), "German gesture offered: {:?}", h.s.sugg);
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(h.s.doc.text, "wenn ich die Fau", "typing German is still allowed");
        let mut h = Harness::new();
        h.frame(vec![Event::Text("when I clo".into())]);
        assert_eq!(h.s.sugg.iter().map(|s| s.chip.label()).collect::<Vec<_>>(), ["close my fist"]);
        h.frame(vec![key(Key::Tab)]);
        h.frame(vec![]);
        let drawn = texts(&h);
        for s in [i18n::EN.side_left, i18n::EN.side_right, i18n::EN.keys_side] {
            assert!(drawn.iter().any(|x| x == s), "{s:?} not drawn: {drawn:?}");
        }
        let german: Vec<&str> =
            i18n::DE.entries().into_iter().zip(i18n::EN.entries()).filter(|(d, e)| d.1 != e.1).map(|(d, _)| d.1).collect();
        assert!(!drawn.iter().any(|x| german.contains(&x.as_str())), "German text drawn: {drawn:?}");
        h.frame(vec![key(Key::ArrowRight), key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.fist".into(), Some("right".into())));
    }

    #[test]
    fn a_bare_english_finger_gets_the_finger_panel() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("when I move my fin".into())]);
        assert_eq!(h.s.sugg[0].chip.label(), "finger");
        assert!(h.s.sugg.iter().all(|s| s.chip.label() != "Finger"), "the German twin stays hidden");
        h.frame(vec![key(Key::Tab), key(Key::ArrowLeft)]);
        h.frame(vec![]);
        assert!(h.find("thumb").is_some());
    }

    #[test]
    fn switching_the_language_refilters_at_once() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("the cut".into())]);
        assert_eq!(h.s.sugg[0].via.as_deref(), Some("Cutoff"));
        h.lang = Lang::En;
        h.frame(vec![]);
        assert_eq!(h.s.sugg[0].via.as_deref(), Some("cutoff"));
        assert!(h.find(i18n::EN.placeholder).is_none(), "the field is not empty");
        let mut h = Harness::de();
        h.frame(vec![]);
        assert!(h.find(i18n::DE.placeholder).is_some(), "German placeholder");
    }

    #[test]
    fn pins_follow_chips() {
        let mut h = Harness::de();
        h.frame(vec![Event::Text("Fau".into())]);
        h.frame(vec![key(Key::Tab)]);
        h.frame(vec![key(Key::ArrowLeft)]);
        h.frame(vec![key(Key::Tab)]);
        let c = &h.s.doc.chips[0];
        let pin = to_pin(&c.range, &c.chip);
        assert_eq!(pin.bytes, (0, "Faust schließen".len()));
        assert_eq!(pin.what, PinWhat::Gesture { source: "hand.fist".into(), shape: None, side: Some("left".into()) });
    }

    fn hand_route(side: &str, index: u32) -> synthexis_parser::resolve::Route {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        R {
            source: "hand.openness".into(),
            side: Some(side.into()),
            inv: false,
            gates: Vec::new(),
            mods: Vec::new(),
            targets: vec![T { token: "T:param:fx.drive".into(), text: "drive".into(), bound: Some(Bound { index, name: format!("P{index}"), via: Via::Name, plugin: None }) }],
        }
    }

    fn reply(intent: &str, rest: &[String], routes: Vec<synthexis_parser::resolve::Route>) -> assistant::Reply {
        assistant::Reply { intent: intent.into(), rest: rest.to_vec(), routes, lines: vec!["x".into()] }
    }

    /// A reopened project builds the window before set_state: the field
    /// shows nothing until initialize() bumps the state, then the kept
    /// lines from the project, and a Keep after that keeps them saved.
    #[test]
    fn a_state_bump_reloads_the_kept_lines_and_a_keep_keeps_them() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let first = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let saved_store = Arc::new(RwLock::new(String::new()));
        let mut h = Harness::new();
        (h.s.writer, h.s.store) = (Some(first.clone()), Some(saved_store.clone()));
        h.s.apply(reply("map", &[], vec![hand_route("left", 12), hand_route("right", 13)]), Some(&key));
        h.s.keep();
        h.frame(vec![]);
        // the reopened project: the window first, then set_state and initialize()
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let routes = Arc::new(RwLock::new(String::new()));
        let mut h = Harness::new();
        (h.s.writer, h.s.store) = (Some(writer.clone()), Some(routes.clone()));
        h.frame(vec![]);
        assert!(h.s.kept.is_empty());
        routes.write().clone_from(&saved_store.read());
        saved::run(&saved::load(&routes.read()), &writer);
        writer.bump_state();
        h.frame(vec![]);
        assert_eq!(h.s.kept.iter().map(|m| m.line(&i18n::EN)).collect::<Vec<_>>(), ["left hand openness -> Topos / P12", "right hand openness -> Topos / P13"]);
        assert_eq!(h.s.kept.iter().map(|m| m.ids.len()).sum::<usize>(), 2, "the running routes are the lines'");
        h.s.apply(reply("map", &[], vec![hand_route("left", 14)]), Some(&key));
        assert!(h.s.next_line > writer.max_line() - 1);
        h.s.keep();
        h.frame(vec![]);
        assert_eq!(saved::load(&routes.read()).len(), 3, "the old lines stay saved");
        // a preset load: what was pending goes without touching the new state
        h.s.apply(reply("map", &[], vec![hand_route("right", 15)]), Some(&key));
        writer.clear_assigned();
        routes.write().clear();
        writer.bump_state();
        h.frame(vec![]);
        assert!(h.s.pending.is_empty() && h.s.kept.is_empty() && h.s.mapped.is_empty());
    }

    /// discard_pending is the Discard button: routes, their MIDI parts and
    /// the key a sentence set go.
    #[test]
    fn discard_pending_takes_back_routes_parts_and_the_key() {
        use synthexis_parser::resolve::{Route as R, Target as T};
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let note = R {
            source: "hand.pinch".into(),
            side: Some("right".into()),
            inv: false,
            gates: Vec::new(),
            mods: vec!["note:c3".into()],
            targets: vec![T { token: "T:midi.note:".into(), text: String::new(), bound: None }],
        };
        let before = writer.player.store().key;
        h.s.apply(reply("map", &["key:e".into(), "scale:major".into()], vec![note, hand_route("left", 12)]), Some(&key));
        assert_eq!((h.s.pending.len(), writer.player.store().parts.len()), (2, 1));
        h.s.discard_pending();
        assert!(h.s.pending.is_empty() && writer.assigned().is_empty());
        assert!(writer.player.store().parts.is_empty());
        assert_eq!(writer.player.store().key, before);
    }

    /// Scoped to a side, a sentence without a movement sets the side at
    /// once as a pending row; Discard puts the old action back and drops a
    /// new part, Keep keeps it and ends the scope. A sentence with a
    /// movement is a normal line; clear and unmap set the side off.
    #[test]
    fn a_scoped_sentence_sets_the_side_until_keep_or_discard() {
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let serum = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let real = crate::assign::tests::sourceless_replies();
        let said = |i: usize| reply(real[i].1, &real[i].2, real[i].3.clone());
        let wall = |side: Side| writer.pong.setup().wall(side).clone();
        h.s.set_scope(Some(Side::Top));
        h.s.apply(said(0), Some(&serum));
        assert!(matches!(wall(Side::Top), Action::Param { index: 0, .. }));
        h.frame(vec![]);
        assert!(h.find("top wall -> Serum 2 / Filter 1 Freq · range 30-80 %").is_some());
        assert!(h.click("Discard"));
        assert_eq!((wall(Side::Top), h.s.scope()), (Action::Off, Some(Side::Top)));
        // a trap roll on the right: a new part that plays at once, no finger
        h.s.set_scope(Some(Side::Right));
        h.s.apply(said(2), Some(&serum));
        let roll = match wall(Side::Right) {
            Action::Part { id } => id,
            a => return assert_eq!(a, Action::Part { id: 0 }),
        };
        assert_eq!(writer.player.store().walls, [roll]);
        assert!(writer.player.store().binds.is_empty());
        // a second sentence for the side replaces the first and its part
        h.s.apply(said(1), Some(&serum));
        let am = match wall(Side::Right) {
            Action::Part { id } => id,
            a => return assert_eq!(a, Action::Part { id: 0 }),
        };
        let store = writer.player.store();
        assert_eq!((store.parts.len(), store.get(am).map(|p| p.name.clone())), (1, Some("Am".into())));
        h.s.discard_pending();
        assert_eq!(wall(Side::Right), Action::Off);
        assert!(writer.player.store().parts.is_empty(), "the new part goes with Discard");
        let before = writer.player.store().key;
        assert_eq!(before, crate::midi::Store::default().key, "the key the sentence set goes too");
        // Keep: the side stays, the scope ends
        h.s.apply(said(2), Some(&serum));
        h.frame(vec![]);
        assert!(h.click("Keep"));
        assert!(matches!(wall(Side::Right), Action::Part { .. }));
        assert_eq!(h.s.scope(), None);
        // a sentence with a movement is a route and leaves the side alone
        h.s.set_scope(Some(Side::Right));
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&serum));
        assert_eq!((h.s.pending.len(), h.s.walls.len()), (1, 0));
        assert!(matches!(wall(Side::Right), Action::Part { .. }));
        h.s.discard_pending();
        // clear and unmap set the scoped side off, nothing else
        h.s.set_scope(Some(Side::Top));
        h.s.apply(said(0), Some(&serum));
        h.s.keep();
        h.s.set_scope(Some(Side::Right));
        h.s.apply(reply("clear", &[], Vec::new()), Some(&serum));
        assert_eq!(wall(Side::Right), Action::Off);
        assert!(matches!(wall(Side::Top), Action::Param { .. }));
        h.s.keep();
        assert!(writer.player.store().parts.is_empty(), "the replaced wall part goes with the Keep");
        h.s.set_scope(Some(Side::Top));
        h.s.apply(reply("unmap", &[], real[0].3.clone()), Some(&serum));
        assert_eq!(wall(Side::Top), Action::Off);
        // undo is the normal undo: the pending side comes back
        h.s.apply(reply("undo", &[], Vec::new()), Some(&serum));
        assert!(matches!(wall(Side::Top), Action::Param { .. }));
    }

    /// The scope shows in the strip ("Bottom wall" with the suggested
    /// routes); its x ends it, and so does Esc, but only on an empty field
    /// with nothing open.
    #[test]
    fn the_scope_chip_and_esc() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        h.s.set_scope(Some(Side::Bottom));
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        h.frame(vec![]);
        // the strip cuts the summary to the chat column (372 at 720)
        let summary = h.find("Bottom wall · Suggested route");
        assert!(summary.is_some_and(|p| h.strip.contains(p)), "{:?}", texts(&h));
        h.frame(vec![Event::Text("the cut".into())]);
        assert!(h.s.list_open);
        h.frame(vec![key(Key::Escape)]);
        assert_eq!(h.s.scope(), Some(Side::Bottom), "Esc closed the list");
        h.frame(vec![key(Key::Escape)]);
        assert_eq!(h.s.scope(), Some(Side::Bottom), "the field is not empty");
        let mut h = Harness::new();
        h.s.set_scope(Some(Side::Bottom));
        h.frame(vec![]);
        assert!(h.find("Bottom wall").is_some_and(|p| h.strip.contains(p)));
        h.frame(vec![key(Key::Escape)]);
        assert_eq!(h.s.scope(), None);
        assert!(h.focused(), "the field keeps the focus");
        let mut h = Harness::de();
        h.s.set_scope(Some(Side::Top));
        h.frame(vec![]);
        assert!(h.find("Obere Wand").is_some());
        assert!(h.click("×"));
        assert_eq!(h.s.scope(), None);
        assert!(h.find("Obere Wand").is_none());
    }

    /// Pending suggestions show inline under the strip and stay until
    /// Keep or Discard: a click elsewhere, the chevron and Esc leave them;
    /// Keep folds them; a new batch shows again.
    #[test]
    fn suggestions_show_inline_until_keep_or_discard() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        h.frame(vec![]);
        assert!(h.find("Topos / P12").is_none());
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        h.frame(vec![]);
        assert!(h.find("Topos / P12").is_some_and(|p| p.y > h.strip.bottom()), "inline under the strip");
        assert!(h.click(i18n::EN.plugin_picker));
        assert!(h.find("Topos / P12").is_some(), "a click elsewhere leaves them");
        assert!(h.popup_open(), "and reaches the picker");
        h.frame(vec![key(Key::Escape)]);
        h.frame(vec![]);
        assert!(h.click("Suggested routes: 1"));
        h.frame(vec![key(Key::Escape)]);
        assert!(h.find("Topos / P12").is_some(), "the chevron and Esc leave pending ones");
        assert!(h.click("Keep"));
        assert!(!h.s.routes_open && h.find("Topos / P12").is_none(), "Keep folds them");
        assert_eq!(h.s.kept.len(), 1);
        h.s.apply(reply("map", &[], vec![hand_route("right", 13)]), Some(&topos));
        h.frame(vec![]);
        assert!(h.find("Topos / P13").is_some(), "a new batch shows again");
        assert!(h.find("Topos / P12").is_none(), "the kept line only while unfolded");
        assert!(h.click("Discard"));
        assert!(h.find("Topos / P13").is_none() && h.s.pending.is_empty());
    }

    /// The orb between the palms (the hands' distance), a source no drives
    /// list shows: its kept line stays in the strip.
    fn orb_route(index: u32) -> synthexis_parser::resolve::Route {
        synthexis_parser::resolve::Route { source: "orb.size".into(), side: None, ..hand_route("left", index) }
    }

    /// With nothing pending the chevron unfolds the kept lines and folds
    /// them again, once per click; unfold_routes opens them too.
    #[test]
    fn the_chevron_folds_the_kept_lines() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        h.s.apply(reply("map", &[], vec![orb_route(12)]), Some(&topos));
        h.frame(vec![]);
        assert!(h.click("Keep"));
        assert!(h.find("Topos / P12").is_none());
        assert!(h.click("Kept routes: 1"));
        assert!(h.s.routes_open && h.find("Topos / P12").is_some(), "the chevron opens them");
        for _ in 0..5 {
            h.frame(vec![]);
        }
        assert!(h.click("Kept routes: 1"));
        assert!(!h.s.routes_open && h.find("Topos / P12").is_none(), "and folds them");
        h.s.unfold_routes();
        h.frame(vec![]);
        assert!(h.find("Topos / P12").is_some());
    }

    /// Noa: kept routes from the hands and the face leave the strip (their
    /// drives lists show them, each with its x); the rest stays there.
    #[test]
    fn hand_and_face_kept_lines_leave_the_strip() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        let mouth = synthexis_parser::resolve::Route { source: "mouth.open".into(), side: None, ..hand_route("left", 14) };
        h.s.apply(reply("map", &[], vec![hand_route("left", 12), mouth]), Some(&topos));
        h.frame(vec![]);
        assert!(h.click("Keep"));
        assert_eq!(h.s.kept.len(), 2);
        h.s.unfold_routes();
        h.frame(vec![]);
        assert!(h.find("Kept routes").is_none() && h.find("Topos / P12").is_none() && h.find("Topos / P14").is_none());
        assert!(!h.s.inline_shown(), "nothing for the top section to show");
        h.s.apply(reply("map", &[], vec![orb_route(13)]), Some(&topos));
        h.frame(vec![]);
        assert!(h.click("Keep"));
        assert!(h.click("Kept routes: 1"));
        assert!(h.find("Topos / P13").is_some() && h.find("Topos / P12").is_none() && h.find("Topos / P14").is_none());
        assert!(in_strip(&h.s.kept[2]) && !in_strip(&h.s.kept[0]) && !in_strip(&h.s.kept[1]));
    }

    /// The x of a kept row in a drives list: that route goes from the
    /// engine and the saved project; a line with more routes stays, marked;
    /// a pending line with the same route is left to Keep and Discard.
    #[test]
    fn the_x_in_a_drives_list_unmaps_a_kept_route() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let store = Arc::new(RwLock::new(String::new()));
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        h.s.store = Some(store.clone());
        let mut two = hand_route("right", 20);
        two.source = "hand.y".into();
        two.targets.extend(hand_route("right", 21).targets);
        h.s.apply(reply("map", &[], vec![hand_route("left", 12), two]), Some(&topos));
        h.s.keep_now();
        assert_eq!((h.s.kept.len(), writer.assigned().len()), (2, 3));
        assert!(store.read().contains("hand.openness"));
        let edge = |h: &Harness, sig: &str, p: &str| h.s.routing().edges.into_iter().find(|e| e.signal == sig && matches!(&e.target, NodeTarget::Param { name, .. } if name == p));
        let Some(open) = edge(&h, "cam0.hand.L.openness", "P12") else { return assert!(h.s.kept.is_empty(), "{:?}", h.s.routing()) };
        assert!(!open.pending);
        assert_eq!(h.s.unmap_kept(&open.signal, &open.target), 1);
        assert_eq!((h.s.kept.len(), writer.assigned().len()), (1, 2));
        assert!(!store.read().contains("hand.openness"), "the saved project drops it");
        assert!(edge(&h, "cam0.hand.L.openness", "P12").is_none());
        // one of the right hand's two targets: the line stays with the other
        let Some(p20) = edge(&h, "cam0.hand.R.y", "P20") else { return assert!(h.s.kept.is_empty(), "{:?}", h.s.routing()) };
        assert_eq!(h.s.unmap_kept(&p20.signal, &p20.target), 1);
        assert_eq!((h.s.kept.len(), writer.assigned().len()), (1, 1));
        assert!(h.s.kept[0].line(&i18n::EN).contains("partly removed") && h.s.kept[0].ids.len() == 1);
        assert!(edge(&h, "cam0.hand.R.y", "P21").is_some_and(|e| !e.pending));
        // a pending line is not a kept one
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        let Some(pending) = edge(&h, "cam0.hand.L.openness", "P12") else { return assert!(h.s.pending.is_empty()) };
        assert!(pending.pending);
        assert_eq!(h.s.unmap_kept(&pending.signal, &pending.target), 0);
        assert_eq!(h.s.pending.len(), 1);
    }

    /// A box of the Hands Routes view retargets a kept route: a new
    /// parameter of the same plugin, a new movement, a Signal. The engine
    /// and the saved project follow, the range stays, the other hand's
    /// route stays where it was.
    #[test]
    fn retarget_moves_a_kept_route_to_a_new_target_or_movement() {
        use crate::param_find::Found;
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let store = Arc::new(RwLock::new(String::new()));
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        h.s.store = Some(store.clone());
        let mut both = hand_route("both", 12);
        both.mods = vec!["lo:0.2".into()];
        h.s.apply(reply("map", &[], vec![both]), Some(&topos));
        h.s.keep_now();
        assert_eq!(writer.assigned().len(), 2);
        let edges = |h: &Harness| h.s.routing().edges.into_iter().map(|e| (e.signal, e.target, e.pending)).collect::<Vec<_>>();
        let p = |name: &str| NodeTarget::Param { plugin: "Topos".into(), name: name.into() };
        let left = "cam0.hand.L.openness";
        // a parameter of the same plugin
        let to = Found::Param { key: topos.id(), index: 30, name: "Filter 1 Freq".into() };
        assert_eq!(h.s.retarget_kept(left, &p("P12"), &to), 1);
        let now = edges(&h);
        assert!(now.contains(&(left.into(), p("Filter 1 Freq"), false)), "{now:?}");
        assert!(now.contains(&("cam0.hand.R.openness".into(), p("P12"), false)), "the right hand stays: {now:?}");
        assert!(!now.contains(&(left.into(), p("P12"), false)), "{now:?}");
        assert_eq!(writer.assigned().len(), 2, "one engine route in place of the other");
        assert!(writer.assigned().iter().all(|(_, pending, _)| !pending));
        let line = h.s.kept.iter().find(|m| m.keys.iter().any(|(s, _)| s == left));
        assert!(line.is_some_and(|m| m.route.mods.contains(&"lo:0.2".to_string()) && m.route.side.as_deref() == Some("left")), "the range stays");
        // the saved project runs the new target after a reload
        let saved = saved::load(&store.read());
        let fresh = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        saved::run(&saved, &fresh);
        let running: Vec<(String, Target)> = saved.iter().flat_map(|sv| fresh.line_routes(sv.line)).map(|(_, s, t)| (s, t)).collect();
        assert!(running.contains(&(left.into(), Target::Param { plugin: topos.clone(), index: 30 })), "{running:?}");
        assert_eq!(running.len(), 2, "{running:?}");
        // another movement on the right hand
        assert_eq!(h.s.retarget_kept("cam0.hand.R.openness", &p("P12"), &Found::Move("pinch".into())), 1);
        let now = edges(&h);
        assert!(now.contains(&("cam0.hand.R.pinch".into(), p("P12"), false)), "{now:?}");
        assert!(!now.iter().any(|(s, ..)| s == "cam0.hand.R.openness"), "{now:?}");
        // a Signal
        assert_eq!(h.s.retarget_kept(left, &p("Filter 1 Freq"), &Found::Macro(11)), 1);
        assert!(edges(&h).contains(&(left.into(), NodeTarget::Macro(11), false)), "{:?}", edges(&h));
        assert_eq!(writer.assigned().len(), 2);
        // nothing kept there: nothing changes
        assert_eq!(h.s.retarget_kept("cam0.hand.L.y", &p("P12"), &Found::Macro(2)), 0);
        assert_eq!(edges(&h).len(), 2);
    }

    /// A pending line that cannot run (a source the camera does not
    /// measure without a stand-in, no side) leaves Keep dimmed with the
    /// reason on hover, and Cmd+Enter does nothing; Discard still works.
    /// One line that runs makes Keep work again.
    #[test]
    fn keep_is_dimmed_while_nothing_can_run() {
        use synthexis_parser::resolve::Route as R;
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        let heart = R { source: "heart.bpm".into(), side: None, ..hand_route("left", 12) };
        h.s.apply(reply("map", &[], vec![heart.clone()]), Some(&topos));
        let why = h.s.keep_blocked().unwrap_or_default();
        assert!(why.starts_with("Nothing here can run yet: the camera does not measure heart bpm"), "{why}");
        h.frame(vec![]);
        assert_eq!(h.color_of("Keep"), Some(white(glass::DIM_OFF)), "Keep dimmed");
        assert!(h.click("Keep"));
        assert_eq!(h.s.pending.len(), 1, "a dimmed Keep keeps nothing");
        h.frame(vec![cmd(Key::Enter, Modifiers::COMMAND)]);
        assert_eq!(h.s.pending.len(), 1, "nor does Cmd+Enter");
        assert!(h.click("Discard"));
        assert!(h.s.pending.is_empty() && h.s.keep_blocked().is_none());
        // which hand: blocked too; with a line that runs, Keep works
        let no_side = R { side: None, ..hand_route("left", 12) };
        h.s.apply(reply("map", &[], vec![no_side.clone()]), Some(&topos));
        assert!(h.s.keep_blocked().is_some_and(|w| w.contains("which hand")));
        h.s.apply(reply("map", &[], vec![hand_route("right", 13)]), Some(&topos));
        assert!(h.s.keep_blocked().is_none());
        h.frame(vec![]);
        assert_eq!(h.color_of("Keep"), Some(white(Tok::VALUE)));
        assert!(h.click("Keep"));
        assert!(h.s.pending.is_empty() && h.s.kept.len() == 1);
    }

    /// R7: a reply about the hands asks for Hands once; a face route or a
    /// wall scope does not; Pong being on does not matter.
    #[test]
    fn a_sentence_about_fingers_asks_for_hands_once() {
        use synthexis_parser::resolve::Route as R;
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        assert!(h.s.take_open_hands());
        assert!(!h.s.take_open_hands(), "only once");
        h.s.discard();
        let face = R { source: "mouth.open".into(), side: None, ..hand_route("left", 12) };
        h.s.apply(reply("map", &[], vec![face]), Some(&topos));
        assert!(!h.s.take_open_hands(), "a face route");
        h.s.discard();
        h.s.set_scope(Some(Side::Top));
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        assert!(!h.s.take_open_hands(), "a wall is the scope");
        h.s.discard();
        h.s.set_scope(None);
        let mut setup = (*writer.pong.setup()).clone();
        setup.on = true;
        writer.pong.set_setup(setup);
        h.s.apply(reply("map", &[], vec![hand_route("right", 13)]), Some(&topos));
        assert!(h.s.take_open_hands(), "Pong on");
    }

    /// Noa's FL sentence: "when i rise my hands up i want the filter"
    /// came back as S:arm.raise side:left on Filter 1 On. It runs on both
    /// hands' height, opens Hands and shows there, pending and kept; a
    /// face route opens Head, kept or not, and a wall edit opens Pong.
    #[test]
    fn a_reply_opens_the_section_it_shows_in() {
        use synthexis_parser::resolve::{Bound, Route as R, Target as T, Via};
        let serum = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let filter = T { token: "T:toggle:filter".into(), text: "filter".into(), bound: Some(Bound { index: 7, name: "Filter 1 On".into(), via: Via::Class, plugin: None }) };
        let raise = R { source: "arm.raise".into(), side: Some("left".into()), inv: false, gates: Vec::new(), mods: Vec::new(), targets: vec![filter] };
        h.s.apply_said(reply("map", &[], vec![raise]), Some(&serum), "when i rise my hands up i want the filter");
        assert!(h.s.take_open_hands(), "a stand-in on the hands opens Hands");
        assert!(!h.s.take_open_head());
        let line = h.s.pending[0].line(&i18n::EN);
        assert_eq!(line, "hand height (both hands) -> Serum 2 / Filter 1 On · the camera does not measure arm raise, so this uses hand height");
        assert!(h.s.keep_blocked().is_none(), "it runs, Keep works");
        let edges = |h: &Harness| h.s.routing().edges.iter().map(|e| (e.signal.clone(), e.pending)).collect::<Vec<_>>();
        let both = |p: bool| vec![("cam0.hand.L.y".to_string(), p), ("cam0.hand.R.y".to_string(), p)];
        assert_eq!(edges(&h), both(true), "Hands shows it pending");
        h.s.keep();
        assert_eq!(edges(&h), both(false), "and kept");
        // a kept stand-in line opens Hands when the project opens again
        let kept = crate::saved::Saved {
            key: None,
            route: h.s.kept[0].route.clone(),
            line: 1,
            batch: 1,
            full: 2,
            partly: false,
            note: String::new(),
            keys: h.s.kept[0].keys.iter().map(|(sig, t)| (sig.clone(), crate::saved::target_id(t))).collect(),
        };
        let snap = crate::preset::Snapshot { routes: vec![kept], ..Default::default() };
        assert!(crate::preset::has_content(&snap, crate::preset::Section::Hands));
        // the face and the head open Head, not Hands
        for source in ["mouth.open", "head.yaw", "face.smile"] {
            let face = R { source: source.into(), side: None, ..hand_route("left", 12) };
            h.s.apply(reply("map", &[], vec![face]), Some(&serum));
            assert!(h.s.take_open_head() && !h.s.take_open_hands(), "{source}");
            h.s.discard();
        }
        let wink = R { source: "eye.closed".into(), side: Some("right".into()), ..hand_route("left", 12) };
        h.s.apply(reply("map", &[], vec![wink]), Some(&serum));
        assert!(h.s.take_open_head(), "an eye");
        h.s.discard();
        // a wall sentence opens Pong
        h.s.set_scope(Some(Side::Top));
        let real = crate::assign::tests::sourceless_replies();
        h.s.apply(reply(real[0].1, &real[0].2, real[0].3.clone()), Some(&serum));
        assert!(h.s.take_open_pong() && !h.s.take_open_hands() && !h.s.take_open_head());
        assert!(!h.s.take_open_pong(), "only once");
    }

    #[test]
    fn sections_for_reads_sources_and_signals() {
        let src = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sections_for(&src(&["hand.y"]), &[]), (true, false));
        assert_eq!(sections_for(&[], &["cam0.hand.L.y"]), (true, false));
        assert_eq!(sections_for(&src(&["mouth.open"]), &["cam0.mouth.open"]), (false, true));
        assert_eq!(sections_for(&[], &["cam0.eye.R.closed", "cam0.hand.R.pinch"]), (true, true));
        assert_eq!(sections_for(&src(&["heart.bpm"]), &[]), (false, false));
    }

    /// R19 data: pending and kept hand routes as edges, focus while a hand
    /// drives a parameter or a parameter chip sits in the field, a new
    /// batch id per reply and 0 when nothing is pending.
    #[test]
    fn routing_lists_pending_hand_param_edges_and_sets_focus_and_batch() {
        use synthexis_parser::resolve::Route as R;
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        assert_eq!(h.s.routing(), Routing::default());
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        let r = h.s.routing();
        let p12 = NodeTarget::Param { plugin: "Topos".into(), name: "P12".into() };
        assert_eq!(r.edges.iter().map(|e| (e.target.clone(), e.pending)).collect::<Vec<_>>(), [(p12.clone(), true)]);
        assert!(r.edges.iter().all(|e| e.signal.starts_with("cam0.hand.") && e.signal.ends_with("openness")), "{:?}", r.edges);
        assert!(r.focus && r.batch != 0, "{r:?}");
        let face = R { source: "mouth.open".into(), side: None, ..hand_route("left", 14) };
        h.s.apply(reply("map", &[], vec![face, hand_route("right", 13)]), Some(&topos));
        let r2 = h.s.routing();
        assert_eq!(r2.edges.len(), 2, "no face edge: {:?}", r2.edges);
        assert!(r2.batch != 0 && r2.batch != r.batch, "a new batch per reply");
        h.s.keep();
        let r3 = h.s.routing();
        assert_eq!((r3.batch, r3.focus), (0, false));
        assert!(r3.edges.len() == 2 && r3.edges.iter().all(|e| !e.pending), "{:?}", r3.edges);
        // a parameter chip in the field
        h.frame(vec![Event::Text("the cut".into())]);
        h.frame(vec![key(Key::Tab)]);
        assert!(h.s.routing().focus);
    }

    /// The Head section's edges: only the face and head signals, pending
    /// and then kept, never a hand's.
    #[test]
    fn face_routing_lists_only_face_and_head_edges() {
        use synthexis_parser::resolve::Route as R;
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        assert_eq!(h.s.face_routing(), Routing::default());
        let mouth = R { source: "mouth.open".into(), side: None, ..hand_route("left", 14) };
        let wink = R { source: "eye.closed".into(), side: Some("both".into()), ..hand_route("left", 15) };
        h.s.apply(reply("map", &[], vec![mouth, wink, hand_route("right", 13)]), Some(&topos));
        let r = h.s.face_routing();
        let sigs: Vec<(&str, bool)> = r.edges.iter().map(|e| (e.signal.as_str(), e.pending)).collect();
        assert_eq!(sigs, [("cam0.mouth.open", true), ("cam0.eye.L.closed", true), ("cam0.eye.R.closed", true)], "{:?}", r.edges);
        assert_eq!(r.edges[0].target, NodeTarget::Param { plugin: "Topos".into(), name: "P14".into() });
        assert!(r.focus && r.batch != 0);
        assert!(h.s.routing().edges.iter().all(|e| e.signal.starts_with("cam0.hand.")), "Hands keeps only its own");
        h.s.keep();
        let r = h.s.face_routing();
        assert!(r.edges.len() == 3 && r.edges.iter().all(|e| !e.pending) && r.batch == 0, "{r:?}");
    }

    /// While the field has the focus, Cmd+Enter keeps and Cmd+Backspace
    /// discards; the typed text stays and is not sent.
    #[test]
    fn cmd_enter_keeps_and_cmd_backspace_discards() {
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let mut h = Harness::new();
        h.s.writer = Some(Writer::new(Arc::new(crate::params::SynthexisParams::default())));
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&topos));
        h.frame(vec![Event::Text("abc".into())]);
        h.frame(vec![cmd(Key::Enter, Modifiers::COMMAND)]);
        assert!(h.s.pending.is_empty() && h.s.kept.len() == 1, "Cmd+Enter keeps");
        assert_eq!(h.s.doc.text, "abc");
        assert!(h.s.reply.is_empty(), "nothing sent");
        h.s.apply(reply("map", &[], vec![hand_route("right", 13)]), Some(&topos));
        h.frame(vec![cmd(Key::Backspace, Modifiers::COMMAND)]);
        assert!(h.s.pending.is_empty() && h.s.kept.len() == 1, "Cmd+Backspace discards");
        assert_eq!(h.s.doc.text, "abc");
        assert!(h.focused());
    }

    /// FL re-activating the plugin runs initialize() on the same state and
    /// bumps it: the pending lines and sides go as with Discard, the kept
    /// line a pending one replaced runs again. A load of other lines leaves
    /// no pending route running either.
    #[test]
    fn a_state_bump_takes_back_what_was_pending() {
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let routes = Arc::new(RwLock::new(String::new()));
        let mut h = Harness::new();
        (h.s.writer, h.s.store) = (Some(writer.clone()), Some(routes.clone()));
        let key = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&key));
        h.s.keep();
        h.frame(vec![]);
        let mut softer = hand_route("left", 12);
        softer.mods = vec!["hi:0.5".into()];
        h.s.apply(reply("modify", &[], vec![softer]), Some(&key));
        let real = crate::assign::tests::sourceless_replies();
        h.s.set_scope(Some(Side::Top));
        h.s.apply(reply(real[0].1, &real[0].2, real[0].3.clone()), Some(&key));
        h.frame(vec![]);
        assert!(h.s.kept.is_empty() && h.s.pending.len() == 1 && h.s.walls.len() == 1);
        writer.bump_state();
        h.frame(vec![]);
        assert!(h.s.pending.is_empty() && h.s.walls.is_empty());
        assert_eq!(writer.assigned().iter().map(|a| a.1).collect::<Vec<_>>(), [false], "only the kept line runs");
        assert_eq!(h.s.kept.iter().map(|m| (m.line(&i18n::EN), m.ids.len())).collect::<Vec<_>>(), [("left hand openness -> Serum 2 / P12".to_string(), 1)]);
        assert_eq!(saved::load(&routes.read()).len(), 1);
        assert_eq!(writer.pong.setup().wall(Side::Top), &Action::Off);
        // a load of other kept lines: the pending route goes, the loaded state stays
        h.s.apply(reply("map", &[], vec![hand_route("right", 13)]), Some(&key));
        routes.write().clear();
        writer.bump_state();
        h.frame(vec![]);
        assert!(writer.assigned().iter().all(|a| !a.1), "{:?}", writer.assigned());
        assert!(h.s.pending.is_empty() && h.s.kept.is_empty());
    }

    /// undo after a Keep of sides only takes back those sides (the part a
    /// side played before comes back), not an older kept line.
    #[test]
    fn undo_after_a_wall_keep_takes_back_the_sides() {
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let serum = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let real = crate::assign::tests::sourceless_replies();
        let said = |i: usize| reply(real[i].1, &real[i].2, real[i].3.clone());
        let wall = |side: Side| writer.pong.setup().wall(side).clone();
        let undo = || reply("undo", &[], Vec::new());
        h.s.apply(reply("map", &[], vec![hand_route("left", 12)]), Some(&serum));
        h.s.keep();
        h.s.set_scope(Some(Side::Top));
        h.s.apply(said(0), Some(&serum));
        h.s.keep();
        h.s.apply(undo(), Some(&serum));
        assert_eq!(h.s.kept.len(), 1, "the hand line stays");
        assert_eq!(wall(Side::Top), Action::Off);
        // a kept part, replaced by another and kept: undo brings the first back
        h.s.set_scope(Some(Side::Right));
        h.s.apply(said(2), Some(&serum));
        h.s.keep();
        let Action::Part { id: roll } = wall(Side::Right) else { return assert!(matches!(wall(Side::Right), Action::Part { .. })) };
        h.s.set_scope(Some(Side::Right));
        h.s.apply(said(1), Some(&serum));
        h.s.keep();
        let Action::Part { id: am } = wall(Side::Right) else { return assert!(matches!(wall(Side::Right), Action::Part { .. })) };
        assert!(writer.player.store().get(roll).is_none(), "the Keep dropped the part before");
        h.s.apply(undo(), Some(&serum));
        assert_eq!(wall(Side::Right), Action::Part { id: roll });
        let store = writer.player.store();
        assert!(store.get(roll).is_some() && store.get(am).is_none(), "{:?}", store.parts);
        h.s.apply(undo(), Some(&serum));
        assert_eq!(wall(Side::Right), Action::Off);
        assert!(writer.player.store().parts.is_empty());
        assert_eq!(h.s.kept.len(), 1);
        h.s.apply(undo(), Some(&serum));
        assert!(h.s.kept.is_empty());
    }

    /// A wall part made without FL's channel list waits for one
    /// (auto_channel) and says so in the wall's own words; Discard and a
    /// second sentence take it out again.
    #[test]
    fn a_wall_part_without_a_channel_waits_for_one() {
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        // an effect in focus: no generator to play on
        let fx = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let real = crate::assign::tests::sourceless_replies();
        let said = |i: usize| reply(real[i].1, &real[i].2, real[i].3.clone());
        let part = || match writer.pong.setup().wall(Side::Right) {
            Action::Part { id } => *id,
            _ => 0,
        };
        h.s.set_scope(Some(Side::Right));
        h.s.apply(said(2), Some(&fx));
        assert_eq!(writer.pong.setup().auto_channel, [part()]);
        h.frame(vec![]);
        assert!(h.find(i18n::EN.w_no_channel).is_some());
        assert!(h.find(i18n::EN.f_title).is_none(), "no finger section named");
        h.s.discard_pending();
        assert!(writer.pong.setup().auto_channel.is_empty());
        h.s.apply(said(2), Some(&fx));
        h.s.apply(said(1), Some(&fx));
        assert_eq!(writer.pong.setup().auto_channel, [part()], "the replaced part left it");
        h.s.keep();
        assert_eq!(writer.pong.setup().auto_channel, [part()], "a kept part still waits");
    }

    /// The pending wall row is drawn in the language shown.
    #[test]
    fn a_pending_wall_line_follows_the_language() {
        let writer = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut h = Harness::new();
        h.s.writer = Some(writer.clone());
        let serum = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let real = crate::assign::tests::sourceless_replies();
        h.s.set_scope(Some(Side::Top));
        h.s.apply(reply(real[0].1, &real[0].2, real[0].3.clone()), Some(&serum));
        h.frame(vec![]);
        assert!(h.find("top wall -> Serum 2 / Filter 1 Freq").is_some());
        h.lang = Lang::De;
        h.frame(vec![]);
        assert!(h.find("obere Wand -> Serum 2 / Filter 1 Freq").is_some());
        assert!(h.find("top wall").is_none());
    }
}
