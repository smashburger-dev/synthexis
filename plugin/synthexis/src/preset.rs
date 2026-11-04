//! Presets (design pong-presets.md, section 8; ui-redesign.md R5): a
//! snapshot of the kept sentence lines, the MIDI store, the Pong setup,
//! the picker pin and the open sections, applied without a window
//! (profiles reuse it), saved as one JSON file per preset. Built-ins
//! ("Hands", "Pong", "Head") live in code, listed first, and can be neither
//! overwritten nor deleted. The window's preset column is `top`.

pub mod profile;
pub mod top;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::fl;
use crate::log::log;
use crate::midi;
use crate::pong::arena::{Life, Side};
use crate::pong::Pong;
use crate::pong::setup::{Action, Hands, How, Setup, Trigger};
use crate::saved::{self, Saved};
use crate::writer::Writer;

pub const VERSION: u32 = 1;

/// Built-in preset names in list order; a user preset cannot take one
/// (folded).
const BUILTIN_NAMES: [&str; 3] = ["Hands", "Pong", "Head"];

/// A feature section of the window. Each has the built-in preset of its
/// name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Hands,
    Pong,
    /// Mouth, eyes and the head's turn, nod and tilt (head_ui.rs).
    Head,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Hands, Section::Pong, Section::Head];

    /// The name of its built-in preset ("Hands", "Pong", "Head").
    pub fn builtin_name(self) -> &'static str {
        match self {
            Section::Hands => BUILTIN_NAMES[0],
            Section::Pong => BUILTIN_NAMES[1],
            Section::Head => BUILTIN_NAMES[2],
        }
    }

    /// The section whose built-in preset is `name` (folded).
    pub fn of_builtin(name: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| fold(s.builtin_name()) == fold(name))
    }
}

/// What a preset holds. Every field has a serde default, so a preset from
/// before Pong loads with the default setup, and one from before the
/// sections opens what it plays (`opens`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// Kept sentence lines (params `routes`).
    pub routes: Vec<Saved>,
    pub midi: midi::Store,
    pub pong: Setup,
    /// The picker pin (fl::Key id); None follows FL's focus.
    pub pin: Option<String>,
    /// The open sections when it was saved; `capture` leaves it empty
    /// (the window's view holds them).
    #[serde(default)]
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub version: u32,
    /// The name as typed; the file name is only its slug.
    pub name: String,
    pub snapshot: Snapshot,
}

impl Default for Preset {
    fn default() -> Self {
        Preset { version: VERSION, name: String::new(), snapshot: Snapshot::default() }
    }
}

/// The instance's state now.
pub fn capture(writer: &Writer) -> Snapshot {
    let pin = writer.params.target_pin.read().trim().to_string();
    Snapshot {
        routes: saved::load(&writer.params.routes.read()),
        midi: writer.player.store(),
        pong: (*writer.pong.setup()).clone(),
        pin: (!pin.is_empty()).then_some(pin),
        sections: Vec::new(),
    }
}

// --- sections ---------------------------------------------------------------

/// A bind signal of the hands ("cam0.hand.R.pinch", "cam0.hand.L.shape").
fn hand_signal(signal: &str) -> bool {
    signal.starts_with("cam0.hand.")
}

/// A kept line whose source is a hand ("hand.openness", "hand.finger.index")
/// or that runs on a hand signal (a stand-in: arm.raise reads hand.y).
fn hand_line(s: &Saved) -> bool {
    s.route.source.starts_with("hand.") || s.keys.iter().any(|k| hand_signal(&k.0))
}

/// Ontology sources of the face and head (signal.rs FACE_IDS without
/// "cam0.").
const HEAD_SOURCES: [&str; 4] = ["mouth.", "face.", "eye.", "head."];

/// A bind signal of the face or head ("cam0.mouth.open", "cam0.eye.L.closed").
pub fn head_signal(signal: &str) -> bool {
    signal.strip_prefix("cam0.").is_some_and(|s| HEAD_SOURCES.iter().any(|h| s.starts_with(h)))
}

/// A kept line whose source is the face or head ("mouth.open", "head.roll").
pub fn head_source(source: &str) -> bool {
    HEAD_SOURCES.iter().any(|h| source.starts_with(h))
}

fn head_line(s: &Saved) -> bool {
    head_source(&s.route.source) || s.keys.iter().any(|k| head_signal(&k.0))
}

/// What `s` plays in section `sec`: Hands a hand bind or a kept line with
/// a hand source; Pong on or a wall that is not Off; Head a face or head
/// bind or a kept line with a face or head source.
pub fn has_content(s: &Snapshot, sec: Section) -> bool {
    match sec {
        Section::Hands => s.midi.binds.iter().any(|b| hand_signal(&b.signal)) || s.routes.iter().any(hand_line),
        Section::Pong => s.pong.on || s.pong.walls.iter().any(|w| *w != Action::Off),
        Section::Head => s.midi.binds.iter().any(|b| head_signal(&b.signal)) || s.routes.iter().any(head_line),
    }
}

/// The sections `s` plays, in window order.
pub fn sections_of(s: &Snapshot) -> Vec<Section> {
    Section::ALL.into_iter().filter(|sec| has_content(s, *sec)).collect()
}

/// The sections to open for `s`: its saved ones, then every section it
/// plays that they miss, so the open list is never smaller than the
/// content (R5.7, R5.9).
pub fn opens(s: &Snapshot) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for sec in s.sections.iter().copied().chain(sections_of(s)) {
        if !out.contains(&sec) {
            out.push(sec);
        }
    }
    out
}

/// `now` with section `sec` from its built-in: Hands and Head bring nothing;
/// Pong's part (setup and walls) is replaced by the built-in's, whose
/// wall parts get fresh ids in `now`'s store. Finger parts, binds, kept
/// lines, the pin and the key stay.
pub fn with_section(now: Snapshot, sec: Section) -> Snapshot {
    let mut s = match sec {
        Section::Hands | Section::Head => now,
        Section::Pong => {
            let mut s = without_section(now, Section::Pong);
            let built = pong().snapshot;
            let mut map: Vec<(u64, u64)> = Vec::new();
            for old in built.pong.wall_parts() {
                if let Some(part) = built.midi.get(old) {
                    // above every id in use, also one a hand-edited store
                    // left past next_id
                    let id = s.midi.parts.iter().map(|p| p.id + 1).fold(s.midi.next_id, u64::max);
                    s.midi.next_id = id + 1;
                    s.midi.parts.push(midi::Part { id, ..part.clone() });
                    map.push((old, id));
                }
            }
            let new_id = |old: u64| map.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
            let mut setup = built.pong;
            for w in &mut setup.walls {
                if let Action::Part { id } = w {
                    match new_id(*id) {
                        Some(n) => *id = n,
                        None => *w = Action::Off,
                    }
                }
            }
            setup.auto_channel = setup.auto_channel.iter().filter_map(|id| new_id(*id)).collect();
            s.midi.walls = setup.wall_parts();
            s.pong = setup;
            s
        }
    };
    if !s.sections.contains(&sec) {
        s.sections.push(sec);
    }
    s
}

/// `now` without what section `sec` plays (closing it, R5.6). Hands: the
/// hand binds go, their parts unless a wall or another bind plays them,
/// and the kept lines with a hand source. Head: the same for face and
/// head binds and lines. Pong: the default setup, and its wall parts
/// unless a bind plays them. Everything else stays.
pub fn without_section(now: Snapshot, sec: Section) -> Snapshot {
    let mut s = now;
    match sec {
        Section::Hands | Section::Head => {
            let hands = sec == Section::Hands;
            let signal = |s: &str| if hands { hand_signal(s) } else { head_signal(s) };
            let line = |l: &Saved| if hands { hand_line(l) } else { head_line(l) };
            let gone: Vec<u64> = s.midi.binds.iter().filter(|b| signal(&b.signal)).map(|b| b.part).collect();
            s.midi.binds.retain(|b| !signal(&b.signal));
            let walls = s.pong.wall_parts();
            let binds = &s.midi.binds;
            s.midi.parts.retain(|p| !gone.contains(&p.id) || walls.contains(&p.id) || binds.iter().any(|b| b.part == p.id));
            s.routes.retain(|l| !line(l));
        }
        Section::Pong => {
            let gone = s.pong.wall_parts();
            let binds = &s.midi.binds;
            s.midi.parts.retain(|p| !gone.contains(&p.id) || binds.iter().any(|b| b.part == p.id));
            s.midi.walls.clear();
            s.pong = Setup::default();
        }
    }
    s.sections.retain(|x| *x != sec);
    s
}

/// Replaces the instance's state with `snap` and returns the state before
/// (the window keeps it for Undo load). Pending sentence lines are the
/// window's: its Load runs the field's Discard first. Before FL's
/// inventory the plugin ids stay as saved; `follow` moves them on the
/// first inventory.
pub fn apply(writer: &Writer, snap: &Snapshot) -> Result<Snapshot, String> {
    apply_on(writer, snap, fl::bridge())
}

/// apply with the bridge given (tests: a detached one).
pub(crate) fn apply_on(writer: &Writer, snap: &Snapshot, bridge: &fl::Bridge) -> Result<Snapshot, String> {
    apply_with(writer, snap, bridge, &mut || {})
}

/// apply_on with `between` run between its steps (tests: what the camera
/// and Pong threads do meanwhile). The sources go before the notes stop:
/// the routes first, then Pong off, so no edge or hit plays a note after
/// stop_all.
fn apply_with(writer: &Writer, snap: &Snapshot, bridge: &fl::Bridge, between: &mut dyn FnMut()) -> Result<Snapshot, String> {
    let (mut lines, mut store, mut setup, mut pin) = validate(snap)?;
    let before = capture(writer);
    between();
    writer.clear_assigned();
    between();
    // off clears the arena, ends the hits and sets the ramps to lo; the
    // fingers it unpauses have no routes left to play again
    writer.pong.set_setup(Setup { on: false, ..(*writer.pong.setup()).clone() });
    between();
    writer.player.stop_all();
    between();
    renumber(&mut lines, writer.max_line());
    let (loaded, targets, channels) = {
        let v = bridge.view();
        (v.loaded, v.targets.clone(), v.channels.clone())
    };
    if loaded {
        relocate_all(&mut lines, &mut setup, &mut pin, &targets);
        store.rebind_channels(&channels);
    }
    set_unplaced(writer, (!loaded).then(|| pin.clone()));
    // the writer's next rack check (or the first inventory) rebinds the
    // channels and gives the auto-channel parts theirs, also when the
    // inventory is already there
    writer.reset_fl_gen();
    // the parts before the lines, so a line's part route plays the new part
    writer.set_midi(store);
    between();
    saved::run(&lines, writer);
    between();
    // a Pong tick that read the old setup before it went off
    writer.pong.clear();
    writer.pong.set_setup(setup);
    *writer.params.routes.write() = if lines.is_empty() { String::new() } else { saved::store(&lines) };
    *writer.params.target_pin.write() = pin.unwrap_or_default();
    writer.bump_state();
    Ok(before)
}

/// Instances whose last load ran before FL's inventory, by their Pong
/// (it lives as long as the writer), with the pin that load set.
static UNPLACED: Mutex<Vec<(Weak<Pong>, Option<String>)>> = Mutex::new(Vec::new());

/// Marks the instance as loaded before the inventory (`Some(pin)`) or
/// not; gone instances leave the list.
fn set_unplaced(writer: &Writer, mark: Option<Option<String>>) {
    let me = Arc::downgrade(&writer.pong);
    let mut all = UNPLACED.lock();
    all.retain(|(p, _)| p.strong_count() > 0 && !p.ptr_eq(&me));
    if let Some(pin) = mark {
        all.push((me, pin));
    }
}

/// FL's inventory is there (design section 8, step 5): after a load that
/// ran before it, the kept lines, parameter walls and pin move to where
/// their plugins are now and the moved lines run again. Once per such
/// load, nothing while the inventory is missing. A pin the field dropped
/// as gone meanwhile comes back moved; one the user set since stays.
/// Main queue, like the writer's rack check.
pub fn follow(writer: &Writer, bridge: &fl::Bridge) {
    let targets = {
        let v = bridge.view();
        if !v.loaded {
            return;
        }
        v.targets.clone()
    };
    let loaded_pin = {
        let me = Arc::downgrade(&writer.pong);
        let mut all = UNPLACED.lock();
        let Some(i) = all.iter().position(|(p, _)| p.ptr_eq(&me)) else { return };
        all.swap_remove(i).1
    };
    let now_pin = writer.params.target_pin.read().trim().to_string();
    let mut pin = loaded_pin.filter(|p| now_pin.is_empty() || *p == now_pin);
    let old = saved::load(&writer.params.routes.read());
    let mut lines = old.clone();
    let setup = writer.pong.setup();
    let mut moved_setup = (*setup).clone();
    relocate_all(&mut lines, &mut moved_setup, &mut pin, &targets);
    let moved: Vec<Saved> = lines.iter().zip(&old).filter(|(a, b)| a != b).map(|(a, _)| a.clone()).collect();
    for s in &moved {
        let ids: Vec<u64> = writer.line_routes(s.line).into_iter().map(|(id, _, _)| id).collect();
        writer.remove(&ids);
    }
    saved::run(&moved, writer);
    if !moved.is_empty() {
        *writer.params.routes.write() = saved::store(&lines);
    }
    let walls = moved_setup.walls != setup.walls;
    if walls {
        writer.pong.set_setup(moved_setup);
    }
    let repin = pin.filter(|p| *p != now_pin);
    if let Some(p) = &repin {
        *writer.params.target_pin.write() = p.clone();
    }
    if !moved.is_empty() || walls || repin.is_some() {
        writer.bump_state();
    }
}

/// Err when apply would refuse `snap` (checked before the window drops
/// anything, such as the chat field's pending lines).
pub(crate) fn check(snap: &Snapshot) -> Result<(), String> {
    validate(snap).map(|_| ())
}

/// The snapshot parsed into what apply sets, everything checked before
/// anything is removed: the setup clamped, walls and auto channels of
/// parts the store does not have dropped (logged).
fn validate(snap: &Snapshot) -> Result<(Vec<Saved>, midi::Store, Setup, Option<String>), String> {
    let mut store = snap.midi.clone();
    let mut ids: Vec<u64> = store.parts.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("two MIDI parts have the id {}", w[0]));
    }
    store.next_id = store.next_id.max(ids.last().map_or(1, |m| m + 1));
    let mut setup = snap.pong.clone().clamped();
    for (side, w) in Side::ALL.iter().zip(setup.walls.iter_mut()) {
        if let Action::Part { id } = *w {
            if store.get(id).is_none() {
                log(&format!("preset: {side:?} wall dropped, its MIDI part {id} is missing"));
                *w = Action::Off;
            }
        }
    }
    setup.auto_channel.retain(|id| store.get(*id).is_some());
    store.walls = setup.wall_parts();
    let pin = snap.pin.clone().filter(|p| !p.trim().is_empty());
    Ok((snap.routes.clone(), store, setup, pin))
}

/// Line ids above `above` (any line in use), batch ids from 1, both in
/// their old order, so lines of one batch stay one batch.
fn renumber(lines: &mut [Saved], above: u64) {
    let sorted = |f: fn(&Saved) -> u64, lines: &[Saved]| {
        let mut v: Vec<u64> = lines.iter().map(f).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let old_lines = sorted(|s| s.line, lines);
    let old_batches = sorted(|s| s.batch, lines);
    let rank = |v: &[u64], x: u64| v.iter().position(|&y| y == x).unwrap_or(0) as u64 + 1;
    for s in lines {
        s.line = above + rank(&old_lines, s.line);
        s.batch = rank(&old_batches, s.batch);
    }
}

/// Where the plugin of `id` is now, when its place holds another plugin
/// (or none): the only plugin of that name, on the same mixer track first.
/// None when it is still there, gone or ambiguous.
fn relocate(id: &str, targets: &[fl::Target]) -> Option<fl::Key> {
    let old = fl::Key::parse(id)?;
    if targets.iter().any(|t| t.key == old) {
        return None;
    }
    let named: Vec<&fl::Target> = targets.iter().filter(|t| t.key.plugin == old.plugin).collect();
    let track = match old.at {
        fl::At::Fx { track, .. } => Some(track),
        fl::At::Gen { .. } => None,
    };
    let here: Vec<&fl::Target> = named.iter().copied().filter(|t| Some(t.track) == track).collect();
    match (here.as_slice(), named.as_slice()) {
        ([t], _) | ([], [t]) => Some(t.key.clone()),
        _ => {
            log(&format!("preset: {id} not found in the project, left as it is"));
            None
        }
    }
}

/// Every plugin id of the snapshot moved to where that plugin is now.
fn relocate_all(lines: &mut [Saved], setup: &mut Setup, pin: &mut Option<String>, targets: &[fl::Target]) {
    for s in lines.iter_mut() {
        let mut ids: Vec<String> = s.key.iter().cloned().collect();
        ids.extend(s.route.targets.iter().filter_map(|t| t.bound.as_ref()?.plugin.clone()));
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let (Some(from), Some(to)) = (fl::Key::parse(&id), relocate(&id, targets)) else { continue };
            saved::rebind(s, &from, &to);
        }
    }
    for w in &mut setup.walls {
        if let Action::Param { key, .. } = w {
            if let Some(to) = relocate(key, targets) {
                *key = to.id();
            }
        }
    }
    if let Some(p) = pin.as_mut() {
        if let Some(to) = relocate(p, targets) {
            *p = to.id();
        }
    }
}

// --- library --------------------------------------------------------------

/// One row of the preset picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The saved preset's file; None for a built-in.
    pub file: Option<PathBuf>,
    pub builtin: bool,
    /// A saved preset whose name folds to a built-in's (made before that
    /// built-in existed): listed after the built-ins, loaded and deleted
    /// by its file.
    pub twin: bool,
}

/// ~/Library/Application Support/synthexis/presets/, or $SYNTHEXIS_PRESETS.
pub(crate) fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_PRESETS").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/presets")
}

/// The built-ins first, then the user's presets by name (twins tagged).
pub fn list() -> Vec<Entry> {
    list_in(&dir())
}

/// The saved preset in `path` (a picker row).
pub fn load_file(path: &Path) -> Result<Preset, String> {
    read_file(path).and_then(|p| if p.name.trim().is_empty() { Err(format!("{} has no name", path.display())) } else { Ok(p) })
}

/// Moves the saved preset in `path` to presets/deleted/ (a twin of a
/// built-in too).
pub fn delete_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("no preset file {}", path.display()));
    }
    let dir = path.parent().map_or_else(dir, Path::to_path_buf);
    let deleted = dir.join("deleted");
    std::fs::create_dir_all(&deleted).map_err(|e| format!("deleted folder not made: {e}"))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "preset".into());
    std::fs::rename(path, free(&deleted, &stem)).map_err(|e| format!("preset not moved: {e}"))
}

/// Names compare trimmed and case-folded.
pub(crate) fn fold(name: &str) -> String {
    name.trim().to_lowercase()
}

pub(crate) fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.iter().any(|b| fold(b) == fold(name))
}

fn read_file(path: &Path) -> Result<Preset, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Preset>(&s).map_err(|e| e.to_string()))
}

/// The user's presets as (file, preset), a file per name; a broken file is
/// logged and skipped, so is a second file with a name already read. A
/// preset named like a built-in (a twin) stays in the list.
fn files(dir: &Path) -> Vec<(PathBuf, Preset)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json") && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    paths.sort();
    let mut out: Vec<(PathBuf, Preset)> = Vec::new();
    for path in paths {
        match read_file(&path) {
            Ok(p) if p.name.trim().is_empty() => log(&format!("preset: {} has no name, skipped", path.display())),
            Ok(p) if out.iter().any(|(_, q)| fold(&q.name) == fold(&p.name)) => {
                log(&format!("preset: {} repeats the name \"{}\", skipped", path.display(), p.name));
            }
            Ok(p) => out.push((path, p)),
            Err(e) => log(&format!("preset: {} not read ({e})", path.display())),
        }
    }
    out
}

pub(crate) fn list_in(dir: &Path) -> Vec<Entry> {
    let mut user: Vec<(PathBuf, String)> = files(dir).into_iter().map(|(f, p)| (f, p.name)).collect();
    user.sort_by_key(|(_, n)| fold(n));
    let built = builtins().into_iter().map(|p| Entry { name: p.name, file: None, builtin: true, twin: false });
    built.chain(user.into_iter().map(|(f, n)| Entry { twin: is_builtin(&n), name: n, file: Some(f), builtin: false })).collect()
}

#[cfg(test)]
fn load_in(dir: &Path, name: &str) -> Result<Preset, String> {
    if let Some(p) = builtin(name) {
        return Ok(p);
    }
    files(dir).into_iter().map(|(_, p)| p).find(|p| fold(&p.name) == fold(name)).ok_or_else(|| format!("no preset named \"{}\"", name.trim()))
}

pub(crate) fn save_in(dir: &Path, name: &str, snap: &Snapshot) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a preset needs a name".into());
    }
    if is_builtin(name) {
        return Err(format!("\"{name}\" is a built-in preset"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("presets folder not made: {e}"))?;
    let path = match files(dir).into_iter().find(|(_, p)| !is_builtin(&p.name) && fold(&p.name) == fold(name)) {
        Some((path, _)) => path,
        None => free(dir, &slug(name)),
    };
    let preset = Preset { version: VERSION, name: name.to_string(), snapshot: snap.clone() };
    let json = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    write_atomic(dir, &path, &json)
}

#[cfg(test)]
fn delete_in(dir: &Path, name: &str) -> Result<(), String> {
    if is_builtin(name) {
        return Err(format!("\"{}\" is a built-in preset", name.trim()));
    }
    let Some((path, _)) = files(dir).into_iter().find(|(_, p)| fold(&p.name) == fold(name)) else {
        return Err(format!("no preset named \"{}\"", name.trim()));
    };
    delete_file(&path)
}

/// The file name of a preset name: lower case ASCII letters and digits,
/// accents dropped ("Bässe" -> "basse"), the rest one '-'.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'ñ' => "n",
            'ç' => "c",
            'ß' => "ss",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => {
                if !out.is_empty() && !out.ends_with('-') {
                    out.push('-');
                }
                continue;
            }
        };
        out.push_str(plain);
    }
    let s: String = out.trim_end_matches('-').chars().take(60).collect();
    if s.is_empty() { "preset".into() } else { s }
}

/// `<stem>.json` in `dir`, else `<stem>-2.json`, `-3` ... (the file holds
/// another name).
fn free(dir: &Path, stem: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let file = if n == 1 { format!("{stem}.json") } else { format!("{stem}-{n}.json") };
        let path = dir.join(file);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

static TMP: AtomicU64 = AtomicU64::new(0);

/// Writes through a tmp file (pid and a counter in its name) and a
/// rename, so a crash never leaves half a preset.
fn write_atomic(dir: &Path, path: &Path, text: &str) -> Result<(), String> {
    let tmp = dir.join(format!(".preset.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, text).map_err(|e| format!("preset not written: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("preset not written: {e}")
    })
}

// --- built-ins ------------------------------------------------------------

/// Hands, Pong, Head (`BUILTIN_NAMES` order).
pub fn builtins() -> Vec<Preset> {
    vec![hands(), pong(), head()]
}

/// The built-in preset `name` (folded): "Hands" (empty), "Pong", "Head"
/// (empty).
#[cfg(test)]
pub fn builtin(name: &str) -> Option<Preset> {
    builtins().into_iter().find(|p| fold(&p.name) == fold(name))
}

/// "Hands": the Hands section open, nothing played yet. Picking it never
/// adds content (R5.3).
fn hands() -> Preset {
    let snapshot = Snapshot { sections: vec![Section::Hands], ..Snapshot::default() };
    Preset { version: VERSION, name: BUILTIN_NAMES[0].into(), snapshot }
}

/// "Head": the Head section open, nothing played yet. Like Hands, picking
/// it never adds content; the person says what the face does.
fn head() -> Preset {
    let snapshot = Snapshot { sections: vec![Section::Head], ..Snapshot::default() };
    Preset { version: VERSION, name: BUILTIN_NAMES[2].into(), snapshot }
}

/// "Pong", the stress test: the right hand points and shoots, bounces
/// never end, up to 32 at once. Left and right walls ramp Signal 11 and
/// 12 (link them in FL), the top wall plays "Pong top" (C6 in FL's
/// naming), the bottom one "Pong bottom", I-V-vi-IV, one chord per hit.
/// Both parts have no channel and wait in `auto_channel` for FL's
/// channel list. A drum channel (FPC) would want hat 42 and kick 36: that
/// is for the writer's auto-channel step (fit_to_channel) if it does it;
/// this preset does not rely on it.
fn pong() -> Preset {
    let mut store = midi::Store::default();
    let top = store.add("Pong top", midi::Play::Note { pitch: 72, vel: 100 }, None);
    let chords = midi::Play::Chords { degrees: vec![0, 4, 5, 3], size: 3, octave: 4, vel: 96, strum: 0 };
    let bottom = store.add("Pong bottom", chords, None);
    for id in [top, bottom] {
        if let Some(p) = store.get_mut(id) {
            p.start = midi::Start::Now;
        }
    }
    store.walls = vec![top, bottom];
    let ramp = How::default();
    let mut walls: [Action; 4] = Default::default();
    walls[Side::Top.index()] = Action::Part { id: top };
    walls[Side::Bottom.index()] = Action::Part { id: bottom };
    walls[Side::Left.index()] = Action::Macro { index: 10, how: ramp };
    walls[Side::Right.index()] = Action::Macro { index: 11, how: ramp };
    let pong = Setup {
        on: true,
        hands: Hands::Right,
        trigger: Trigger::Point,
        speed: 1.0,
        life: Life { bounces: None, seconds: None },
        max: 32,
        walls,
        auto_channel: vec![top, bottom],
        ..Setup::default()
    };
    let snapshot = Snapshot { routes: Vec::new(), midi: store, pong, pin: None, sections: vec![Section::Pong] };
    Preset { version: VERSION, name: BUILTIN_NAMES[1].into(), snapshot }
}

#[cfg(test)]
mod tests {
    use synthexis_parser::resolve;

    use super::*;
    use crate::params::SynthexisParams;
    use crate::pong::setup::Mode;

    /// A fresh folder under the temp dir, unique per test and process.
    pub(super) fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("synthexis-presets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    pub(super) fn json_files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|r| r.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    pub(super) fn topos(track: u32, slot: u32) -> fl::Key {
        fl::Key { at: fl::At::Fx { track, slot }, plugin: "Topos".into() }
    }

    /// A kept line "left hand openness drives the drive" on `key`.
    pub(super) fn line(key: &fl::Key, line: u64) -> Saved {
        let route = resolve::Route {
            source: "hand.openness".into(),
            side: Some("left".into()),
            inv: false,
            gates: Vec::new(),
            mods: Vec::new(),
            targets: vec![resolve::Target {
                token: "T:param:fx.drive".into(),
                text: String::new(),
                bound: Some(resolve::Bound { index: 12, name: "Drive".into(), via: resolve::Via::Name, plugin: None }),
            }],
        };
        let (rs, _) = crate::assign::routes(&route, Some(key));
        let keys = rs.iter().map(|r| (r.signal.clone(), saved::target_id(&r.target))).collect();
        Saved { key: Some(key.id()), route, line, batch: line, full: 1, partly: false, note: String::new(), keys }
    }

    pub(super) fn writer() -> Arc<Writer> {
        Writer::new(Arc::new(SynthexisParams::default()))
    }

    /// A writer with two kept lines, a part with a wall, Pong on.
    fn busy() -> Arc<Writer> {
        let w = writer();
        let lines = vec![line(&topos(0, 0), 4), line(&topos(1, 2), 9)];
        *w.params.routes.write() = saved::store(&lines);
        saved::run(&lines, &w);
        let mut store = midi::Store::default();
        let id = store.add("Kick", midi::Play::Note { pitch: 36, vel: 110 }, None);
        w.set_midi(store);
        let mut setup = Setup { on: true, max: 8, ..Setup::default() };
        setup.walls[Side::Top.index()] = Action::Part { id };
        w.pong.set_setup(setup);
        *w.params.target_pin.write() = topos(0, 0).id();
        w
    }

    /// The user's preset names as listed, built-ins left out.
    fn saved_names(d: &Path) -> Vec<String> {
        list_in(d).into_iter().filter(|e| !e.builtin).map(|e| e.name).collect()
    }

    fn snap() -> Snapshot {
        Snapshot { routes: vec![line(&topos(0, 0), 3)], ..Snapshot::default() }
    }

    #[test]
    fn a_saved_preset_lists_and_loads_as_it_was() {
        let d = folder("round-trip");
        assert!(save_in(&d, "  Night set ", &snap()).is_ok());
        assert!(save_in(&d, "Ambient", &Snapshot::default()).is_ok());
        assert_eq!(saved_names(&d), ["Ambient", "Night set"]);
        assert_eq!(load_in(&d, "night SET").map(|p| (p.name, p.snapshot)), Ok(("Night set".to_string(), snap())));
        // a same name overwrites its own file
        assert!(save_in(&d, "night set", &Snapshot::default()).is_ok());
        assert_eq!(json_files(&d), ["ambient.json", "night-set.json"]);
        assert_eq!(load_in(&d, "Night set").map(|p| p.snapshot), Ok(Snapshot::default()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_with_one_slug_keep_their_own_files() {
        let d = folder("slugs");
        for name in ["Bässe", "Basse", "Pong!", "Pong?"] {
            assert!(save_in(&d, name, &Snapshot::default()).is_ok(), "{name}");
        }
        assert_eq!(json_files(&d), ["basse-2.json", "basse.json", "pong-2.json", "pong.json"]);
        assert_eq!(saved_names(&d).len(), 4);
        // "pong!" is "Pong!" folded: it overwrites, no third pong file
        assert!(save_in(&d, "pong!", &snap()).is_ok());
        assert_eq!(json_files(&d).len(), 4);
        assert_eq!(load_in(&d, "Pong!").map(|p| p.snapshot), Ok(snap()));
        assert_eq!(load_in(&d, "Pong?").map(|p| p.snapshot), Ok(Snapshot::default()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn built_in_names_and_empty_names_are_refused() {
        let d = folder("builtins");
        assert!(save_in(&d, " pong ", &snap()).is_err());
        assert!(save_in(&d, "   ", &snap()).is_err());
        assert!(delete_in(&d, "PONG").is_err());
        assert!(json_files(&d).is_empty());
        assert_eq!(load_in(&d, "pong").map(|p| p.snapshot.pong.on), Ok(true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn delete_moves_the_file_to_deleted_and_a_broken_file_is_skipped() {
        let d = folder("delete");
        assert!(save_in(&d, "Old", &snap()).is_ok());
        assert!(std::fs::write(d.join("broken.json"), "{not json").is_ok());
        assert_eq!(saved_names(&d), ["Old"]);
        assert!(delete_in(&d, "old").is_ok());
        assert_eq!(json_files(&d), ["broken.json"]);
        assert_eq!(json_files(&d.join("deleted")), ["old.json"]);
        assert!(delete_in(&d, "Old").is_err());
        // a second delete of a new "Old" keeps the first in deleted/
        assert!(save_in(&d, "Old", &snap()).is_ok());
        assert!(delete_in(&d, "Old").is_ok());
        assert_eq!(json_files(&d.join("deleted")), ["old-2.json", "old.json"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_old_preset_without_pong_loads_the_default_setup() {
        let p: Preset = serde_json::from_str(r#"{"name":"Old","snapshot":{"routes":[],"midi":{"version":1,"key":{"root":0,"scale":"major"},"parts":[],"next_id":1}}}"#).unwrap_or_default();
        assert_eq!(p.name, "Old");
        assert_eq!((p.version, p.snapshot.pong, p.snapshot.pin), (VERSION, Setup::default(), None));
    }

    #[test]
    fn capture_then_apply_restores_the_state_and_returns_the_one_before() {
        let a = busy();
        let snap = capture(&a);
        assert_eq!(snap.routes.len(), 2);
        let b = writer();
        let empty = capture(&b);
        let before = apply_on(&b, &snap, fl::Bridge::detached()).unwrap_or_default();
        assert_eq!(before, empty);
        let got = capture(&b);
        assert_eq!(got.routes.len(), 2);
        assert_eq!(got.midi, snap.midi);
        assert_eq!(got.pong, snap.pong);
        assert_eq!(got.pin, snap.pin);
        // lines renumbered from 1, each running its route
        assert_eq!(got.routes.iter().map(|s| (s.line, s.batch)).collect::<Vec<_>>(), [(1, 1), (2, 2)]);
        assert!(b.has_line(1) && b.has_line(2));
        assert_eq!(b.max_line(), 2);
        // Undo load: the before snapshot puts the empty state back
        assert!(apply_on(&b, &before, fl::Bridge::detached()).is_ok());
        assert!(!b.has_line(1) && !b.has_line(2));
        assert_eq!(capture(&b).pong, empty.pong);
    }

    #[test]
    fn the_built_in_pong_replaces_kept_lines_and_sets_its_walls() {
        let w = busy();
        assert!(w.has_line(4) && w.has_line(9));
        assert!(apply_on(&w, &pong().snapshot, fl::Bridge::detached()).is_ok());
        assert!(!w.has_line(4) && !w.has_line(9));
        assert_eq!(w.max_line(), 0);
        assert!(w.params.routes.read().is_empty());
        assert!(w.params.target_pin.read().is_empty());
        let got = capture(&w);
        assert_eq!(got.midi.parts.len(), 2);
        assert_eq!(got.midi.walls, got.pong.wall_parts());
        assert!(got.pong.on && got.pong.max == 32 && got.pong.life.bounces.is_none());
        assert_eq!(got.pong.walls[Side::Left.index()], Action::Macro { index: 10, how: How::default() });
        assert!(matches!(got.pong.walls[Side::Right.index()], Action::Macro { index: 11, how } if how.mode == Mode::Ramp));
    }

    #[test]
    fn the_built_in_pong_plays_c6_on_top_and_i_v_vi_iv_below() {
        let p = pong();
        assert_eq!(builtin("PONG"), Some(p.clone()));
        let s = &p.snapshot;
        let part = |side: Side| match s.pong.walls[side.index()] {
            Action::Part { id } => s.midi.get(id).cloned(),
            _ => None,
        };
        let (Some(top), Some(bottom)) = (part(Side::Top), part(Side::Bottom)) else {
            return assert!(part(Side::Top).is_some() && part(Side::Bottom).is_some(), "walls without parts");
        };
        assert!(matches!(top.play, midi::Play::Note { pitch, .. } if midi::note_name(pitch) == "C6"));
        assert!(matches!(&bottom.play, midi::Play::Chords { degrees, size: 3, octave: 4, .. } if *degrees == [0, 4, 5, 3]));
        assert_eq!((top.start, bottom.start, top.channel, bottom.channel), (midi::Start::Now, midi::Start::Now, None, None));
        assert_eq!(s.pong.auto_channel, [top.id, bottom.id]);
        assert_eq!(s.midi.walls, [top.id, bottom.id]);
        assert_eq!(s.pong.clone().clamped(), s.pong);
    }

    #[test]
    fn a_line_whose_plugin_moved_follows_it_to_the_only_one_of_that_name() {
        let bridge = fl::Bridge::detached();
        let target = |key: fl::Key, track| fl::Target { key, track, track_name: String::new() };
        let limiter = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Fruity Limiter".into() };
        bridge.update(|v| {
            v.loaded = true;
            v.targets = vec![target(limiter, 0), target(topos(3, 1), 3)];
        });
        let w = writer();
        let snap = Snapshot { routes: vec![line(&topos(0, 0), 5)], pin: Some(topos(0, 0).id()), ..Snapshot::default() };
        assert!(apply_on(&w, &snap, bridge).is_ok());
        let got = capture(&w);
        assert_eq!(got.routes.first().and_then(|s| s.key.clone()), Some(topos(3, 1).id()));
        assert_eq!(got.pin, Some(topos(3, 1).id()));
        let running = w.line_routes(1);
        assert_eq!(running.len(), 1);
        assert!(matches!(&running[0].2, crate::route::Target::Param { plugin, index: 12 } if *plugin == topos(3, 1)));
    }

    #[test]
    fn a_load_before_the_inventory_moves_lines_walls_and_pin_on_the_first_one() {
        let bridge = fl::Bridge::detached();
        let w = writer();
        let mut snap = Snapshot { routes: vec![line(&topos(0, 0), 5)], pin: Some(topos(0, 0).id()), ..Snapshot::default() };
        snap.pong.walls[Side::Left.index()] = Action::Param { key: topos(0, 0).id(), index: 3, name: "Mix".into(), how: How::default() };
        assert!(apply_on(&w, &snap, bridge).is_ok());
        assert_eq!(capture(&w).pin, Some(topos(0, 0).id()), "no inventory: left as it is");
        let gen = w.state_gen();
        // the field drops the pin as gone when the inventory comes first
        w.params.target_pin.write().clear();
        let target = |key: fl::Key, track| fl::Target { key, track, track_name: String::new() };
        let limiter = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Fruity Limiter".into() };
        bridge.update(|v| {
            v.loaded = true;
            v.targets = vec![target(limiter, 0), target(topos(3, 1), 3)];
        });
        follow(&w, bridge);
        let got = capture(&w);
        assert_eq!(got.routes.first().and_then(|s| s.key.clone()), Some(topos(3, 1).id()));
        assert_eq!(got.pin, Some(topos(3, 1).id()));
        assert!(matches!(&got.pong.walls[Side::Left.index()], Action::Param { key, .. } if *key == topos(3, 1).id()));
        let running = w.line_routes(1);
        assert_eq!(running.len(), 1);
        assert!(matches!(&running[0].2, crate::route::Target::Param { plugin, index: 12 } if *plugin == topos(3, 1)));
        assert!(w.state_gen() > gen, "the window reloads");
        // once: a later inventory leaves what the user does since alone
        w.params.target_pin.write().clear();
        follow(&w, bridge);
        assert!(w.params.target_pin.read().is_empty());
    }

    struct Mute;

    impl midi::Out for Mute {
        fn notes(&self, _: &[(u32, u8, u8)]) {}
    }

    impl crate::pong::Sink for Mute {
        fn param(&self, _: &fl::Key, _: u32, _: f32) {}
    }

    #[test]
    fn a_load_leaves_no_note_and_no_shot_without_a_source_whatever_runs_between_its_steps() {
        let w = Writer::with_parts(Arc::new(SynthexisParams::default()), midi::Player::manual(Arc::new(Mute)), Arc::new(Mute));
        let pad = |store: &mut midi::Store| {
            let id = store.add("Pad", midi::Play::Note { pitch: 60, vel: 100 }, Some(midi::Channel { index: 0, name: "Pad".into() }));
            if let Some(p) = store.get_mut(id) {
                (p.mode, p.start) = (midi::Mode::Latch, midi::Start::Now);
            }
            id
        };
        let mut store = midi::Store::default();
        let id = pad(&mut store);
        store.binds.push(midi::Bind { signal: "cam0.hand.R.pinch".into(), trigger: crate::route::Trigger::Above, part: id });
        w.set_midi(store);
        w.pong.set_setup(Setup { on: true, max: 8, ..Setup::default() });
        // the new store has a part of the same id on the same channel, no bind
        let mut next = midi::Store::default();
        pad(&mut next);
        let snap = Snapshot { midi: next, pong: Setup { on: true, max: 8, ..Setup::default() }, ..Snapshot::default() };
        // a finger holds every bind (the camera thread's on edge), the
        // Pong thread shoots
        let mut between = || {
            for (route, _, t) in w.line_routes(crate::writer::BIND_LINE) {
                if let crate::route::Target::Part { id, .. } = t {
                    w.player.trigger(route, id, true);
                }
            }
            w.pong.test_shots(4);
        };
        assert!(apply_with(&w, &snap, fl::Bridge::detached(), &mut between).is_ok());
        assert_eq!(w.player.playing(), Vec::<u64>::new(), "stuck after load");
        assert!(w.pong.view().shots.is_empty(), "shots of the old setup");
    }

    #[test]
    fn a_wall_without_its_part_is_dropped_and_two_parts_with_one_id_refuse() {
        let mut s = Snapshot::default();
        s.pong.walls[0] = Action::Part { id: 42 };
        s.pong.auto_channel = vec![42];
        let r = validate(&s);
        assert!(r.is_ok());
        let Ok((_, store, setup, _)) = r else { return };
        assert_eq!((setup.walls[0].clone(), setup.auto_channel.len(), store.walls.len()), (Action::Off, 0, 0));
        s.midi.add("A", midi::Play::Note { pitch: 60, vel: 100 }, None);
        let twin = s.midi.parts[0].clone();
        s.midi.parts.push(twin);
        assert!(validate(&s).is_err());
    }

    /// A store with a finger part (bound to the right index), a part a
    /// finger and the top wall share, an object-bound part and the wall
    /// itself; kept lines from a hand and from an object (no section).
    pub(super) fn mixed() -> Snapshot {
        let mut s = Snapshot { pin: Some(topos(2, 1).id()), ..Snapshot::default() };
        s.midi.key = midi::Key { root: 2, scale: midi::Scale::Dorian };
        let note = |p| midi::Play::Note { pitch: p, vel: 100 };
        let finger = s.midi.add("Finger", note(60), None);
        let shared = s.midi.add("Shared", note(62), None);
        let face = s.midi.add("Object", note(64), None);
        let shape = crate::route::Trigger::Shape { mask: 0b01000, care: 0b01000 };
        s.midi.binds.push(midi::Bind { signal: "cam0.hand.R.shape".into(), trigger: shape, part: finger });
        s.midi.binds.push(midi::Bind { signal: "cam0.hand.L.pinch".into(), trigger: crate::route::Trigger::Above, part: shared });
        s.midi.binds.push(midi::Bind { signal: "cam0.object.x".into(), trigger: crate::route::Trigger::Above, part: face });
        s.pong.walls[Side::Top.index()] = Action::Part { id: shared };
        s.midi.walls = s.pong.wall_parts();
        let mut object = line(&topos(0, 0), 2);
        object.route.source = "object.x".into();
        // its engine routes read the object, as assign makes them
        object.keys.iter_mut().for_each(|k| k.0 = "cam0.object.x".into());
        s.routes = vec![line(&topos(0, 0), 1), object];
        s
    }

    fn ids(s: &Snapshot) -> Vec<u64> {
        s.midi.parts.iter().map(|p| p.id).collect()
    }

    #[test]
    fn builtins_are_hands_then_pong_then_head() {
        let d = folder("builtins-order");
        let names: Vec<(String, bool, bool)> = list_in(&d).into_iter().map(|e| (e.name, e.builtin, e.file.is_none())).collect();
        assert_eq!(names, [("Hands".to_string(), true, true), ("Pong".into(), true, true), ("Head".into(), true, true)]);
        let hands = builtin(" hands ").map(|p| p.snapshot);
        assert_eq!(hands.as_ref().map(sections_of), Some(vec![]), "Hands brings no content");
        assert_eq!(hands.map(|s| s.sections), Some(vec![Section::Hands]));
        assert_eq!(builtin("Pong").map(|p| p.snapshot.sections), Some(vec![Section::Pong]));
        assert_eq!(builtin("Pong"), Some(pong()));
        assert_eq!(builtin("Ambient"), None);
        assert!(save_in(&d, "HANDS", &snap()).is_err(), "Hands is a built-in name now");
    }

    #[test]
    fn a_builtin_keeps_the_pin_and_the_key() {
        let now = mixed();
        for got in [with_section(now.clone(), Section::Pong), with_section(now.clone(), Section::Hands), without_section(now.clone(), Section::Pong)] {
            assert_eq!((got.pin.clone(), got.midi.key), (now.pin.clone(), now.midi.key));
        }
        // applied too: the writer keeps them
        let w = writer();
        assert!(apply_on(&w, &with_section(now.clone(), Section::Pong), fl::Bridge::detached()).is_ok());
        let got = capture(&w);
        assert_eq!((got.pin, got.midi.key), (now.pin, now.midi.key));
    }

    #[test]
    fn old_presets_open_the_sections_with_content() {
        let json = |pong: &str, binds: &str| {
            format!(r#"{{"name":"Old","snapshot":{{"routes":[],"midi":{{"version":1,"key":{{"root":0,"scale":"major"}},"parts":[],"next_id":1,"binds":[{binds}]}},"pong":{pong}}}}}"#)
        };
        let read = |j: String| serde_json::from_str::<Preset>(&j).map(|p| p.snapshot);
        let pinch = r#"{"signal":"cam0.hand.R.pinch","trigger":"above","part":1}"#;
        let both = read(json(r#"{"on":true}"#, pinch));
        assert!(both.is_ok(), "{both:?}");
        let both = both.unwrap_or_default();
        assert!(both.sections.is_empty());
        assert_eq!(opens(&both), [Section::Hands, Section::Pong]);
        assert_eq!(opens(&read(json(r#"{"on":false}"#, "")).unwrap_or_default()), []);
        assert_eq!(opens(&read(json(r#"{"on":false,"walls":[{"kind":"off"},{"kind":"part","id":3},{"kind":"off"},{"kind":"off"}]}"#, "")).unwrap_or_default()), [Section::Pong]);
        // a kept line with a hand source opens Hands
        let lines = Snapshot { routes: vec![line(&topos(0, 0), 1)], ..Snapshot::default() };
        assert_eq!(opens(&lines), [Section::Hands]);
    }

    #[test]
    fn opens_is_never_smaller_than_the_content() {
        let content = mixed();
        let cases = [vec![], vec![Section::Pong], vec![Section::Hands], vec![Section::Pong, Section::Hands]];
        for saved in cases {
            for base in [content.clone(), Snapshot::default(), pong().snapshot, without_section(content.clone(), Section::Hands)] {
                let s = Snapshot { sections: saved.clone(), ..base };
                let o = opens(&s);
                assert!(sections_of(&s).iter().all(|sec| o.contains(sec)), "{saved:?} -> {o:?}");
                assert_eq!(o[..saved.len()], saved[..], "saved order first");
                let mut d = o.clone();
                d.dedup();
                assert_eq!(d, o, "no section twice");
            }
        }
    }

    #[test]
    fn with_section_pong_keeps_finger_parts_and_binds() {
        let now = without_section(mixed(), Section::Pong);
        let got = with_section(now.clone(), Section::Pong);
        let built = pong().snapshot;
        // the finger parts and binds, the lines, the pin and the key stay
        assert!(ids(&now).iter().all(|id| got.midi.get(*id) == now.midi.get(*id)));
        assert_eq!((&got.midi.binds, &got.routes), (&now.midi.binds, &now.routes));
        // two new wall parts, ids not used before
        let walls = got.pong.wall_parts();
        assert_eq!(walls.len(), 2);
        assert!(walls.iter().all(|id| !ids(&now).contains(id)), "{walls:?} vs {:?}", ids(&now));
        assert_eq!(got.midi.parts.len(), now.midi.parts.len() + 2);
        let names: Vec<String> = walls.iter().filter_map(|id| got.midi.get(*id).map(|p| p.name.clone())).collect();
        assert_eq!(names, ["Pong top", "Pong bottom"]);
        assert_eq!((got.pong.auto_channel.clone(), got.midi.walls.clone()), (walls.clone(), walls));
        assert_eq!(Setup { walls: built.pong.walls.clone(), auto_channel: built.pong.auto_channel.clone(), ..got.pong.clone() }, built.pong);
        assert!(validate(&got).is_ok());
        assert!(got.midi.next_id > *got.pong.wall_parts().iter().max().unwrap_or(&0));
        // a second merge (Reload) replaces the walls, it does not add more
        let again = with_section(got.clone(), Section::Pong);
        assert_eq!(again.midi.parts.len(), got.midi.parts.len());
    }

    #[test]
    fn without_section_hands_drops_only_hand_binds_parts_and_lines_and_keeps_wall_parts() {
        let now = mixed();
        let (finger, shared, face) = (ids(&now)[0], ids(&now)[1], ids(&now)[2]);
        let got = without_section(now.clone(), Section::Hands);
        assert_eq!(ids(&got), [shared, face], "the finger's part goes, the wall's and the object's stay");
        assert!(got.midi.get(finger).is_none());
        assert_eq!(got.midi.binds.iter().map(|b| b.signal.as_str()).collect::<Vec<_>>(), ["cam0.object.x"]);
        assert_eq!(got.routes.iter().map(|l| l.route.source.as_str()).collect::<Vec<_>>(), ["object.x"]);
        assert_eq!((&got.pong, &got.midi.walls, got.midi.key, &got.pin), (&now.pong, &now.midi.walls, now.midi.key, &now.pin));
        assert!(!has_content(&got, Section::Hands) && has_content(&got, Section::Pong));
        // Pong closed: the default setup; the shared part stays for nobody
        // but a bind would keep it, here the finger bind is gone already
        let no_pong = without_section(now.clone(), Section::Pong);
        assert_eq!(no_pong.pong, Setup::default());
        assert_eq!(ids(&no_pong), ids(&now), "every wall part a bind still plays stays");
        assert!(no_pong.midi.walls.is_empty());
        let neither = without_section(got, Section::Pong);
        assert_eq!(ids(&neither), [face]);
    }

    #[test]
    fn head_holds_the_face_and_head_binds_and_lines() {
        let mut now = mixed();
        let note = midi::Play::Note { pitch: 40, vel: 100 };
        let kick = now.midi.add("Kick", note.clone(), None);
        let both = now.midi.add("Both", note, None);
        now.midi.binds.push(midi::Bind { signal: "cam0.eye.L.closed".into(), trigger: crate::route::Trigger::Above, part: kick });
        now.midi.binds.push(midi::Bind { signal: "cam0.mouth.open".into(), trigger: crate::route::Trigger::Above, part: both });
        now.midi.binds.push(midi::Bind { signal: "cam0.hand.R.pinch".into(), trigger: crate::route::Trigger::Above, part: both });
        let mut tilt = line(&topos(0, 0), 3);
        tilt.route.source = "head.roll".into();
        now.routes.push(tilt);
        assert!(!has_content(&mixed(), Section::Head), "an object is no head");
        assert!(has_content(&now, Section::Head));
        assert_eq!(opens(&now), [Section::Hands, Section::Pong, Section::Head]);
        let got = without_section(now.clone(), Section::Head);
        assert!(!has_content(&got, Section::Head) && has_content(&got, Section::Hands) && has_content(&got, Section::Pong));
        assert!(got.midi.get(kick).is_none(), "the eye's part goes");
        assert!(got.midi.get(both).is_some(), "a part a hand also plays stays");
        assert_eq!(got.routes.len(), 2, "the hand and the object line stay");
        assert_eq!(with_section(got.clone(), Section::Head).routes, got.routes, "the built-in brings nothing");
        assert!(head_signal("cam0.head.yaw") && head_signal("cam0.face.smile") && !head_signal("cam0.hand.L.x") && !head_signal("head.yaw"));
        assert_eq!(builtin("head").map(|p| p.snapshot.sections), Some(vec![Section::Head]));
        assert_eq!(Section::of_builtin("HEAD"), Some(Section::Head));
    }

    #[test]
    fn a_flick_preset_keeps_its_trigger() {
        let d = folder("flick");
        let mut s = pong().snapshot;
        s.pong.trigger = Trigger::Flick;
        s.sections = vec![Section::Pong, Section::Hands];
        let json = serde_json::to_string(&s).unwrap_or_default();
        assert_eq!(serde_json::from_str::<Snapshot>(&json).ok(), Some(s.clone()));
        assert!(json.contains(r#""sections":["pong","hands"]"#), "{json}");
        assert!(save_in(&d, "Flicks", &s).is_ok());
        let path = list_in(&d).into_iter().find_map(|e| e.file);
        let back = path.as_deref().map(load_file);
        assert_eq!(back.map(|r| r.map(|p| p.snapshot.pong.trigger)), Some(Ok(Trigger::Flick)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_saved_hands_twin_is_listed_loaded_and_deleted_by_file() {
        let d = folder("twin");
        assert!(std::fs::create_dir_all(&d).is_ok());
        let twin = Preset { name: "Hands".into(), snapshot: snap(), ..Preset::default() };
        let path = d.join("hands.json");
        assert!(std::fs::write(&path, serde_json::to_string(&twin).unwrap_or_default()).is_ok());
        assert!(save_in(&d, "Ambient", &Snapshot::default()).is_ok());
        let e = list_in(&d);
        let rows: Vec<(&str, bool, bool)> = e.iter().map(|e| (e.name.as_str(), e.builtin, e.twin)).collect();
        assert_eq!(rows, [("Hands", true, false), ("Pong", true, false), ("Head", true, false), ("Ambient", false, false), ("Hands", false, true)]);
        assert_eq!(e[4].file.as_deref(), Some(path.as_path()));
        // by file: the saved one, not the built-in
        assert_eq!(load_file(&path).map(|p| p.snapshot), Ok(snap()));
        assert_eq!(load_in(&d, "hands").map(|p| p.snapshot.routes.len()), Ok(0), "by name the built-in wins");
        assert!(delete_in(&d, "Hands").is_err(), "by name a built-in stays refused");
        assert!(delete_file(&path).is_ok());
        assert_eq!(json_files(&d.join("deleted")), ["hands.json"]);
        assert!(list_in(&d).iter().all(|e| !e.twin));
        assert!(load_file(&path).is_err() && delete_file(&path).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
