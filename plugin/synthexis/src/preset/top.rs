//! The preset column of the top section (ui-redesign.md R4, R5): the
//! preset picker, "+", row 2 (Save as, Undo, the name field), closing a
//! section and the toast. `View` is the open sections and the picked
//! preset, kept in params `view` so FL saves them with the project.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nih_plug_egui::egui::{self, vec2, Align, Id, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use serde::{Deserialize, Serialize};

use super::{capture, fold, has_content, is_builtin, sections_of, with_section, without_section, Entry, Section, Snapshot};
use crate::fl;
use crate::glass::{self, Tok};
use crate::i18n::{fill, Text};
use crate::log::log;
use crate::params::SynthexisParams;
use crate::writer::Writer;

/// The version written into params `view`.
const VIEW_VERSION: u32 = 1;
/// How long Undo stays once the user changed the state after an action.
const UNDO_IDLE: Duration = Duration::from_secs(10);
/// `tick` does its work at most this often.
const TICK: Duration = Duration::from_secs(1);
/// Popup lists grow to this and scroll beyond (2.7).
const POP_H: f32 = 420.0;
/// An info toast is dropped this long after it showed (it has faded).
const TOAST_GONE: f32 = 5.0;
/// The padding `glass::quiet` adds to its text.
const QUIET_PAD: f32 = 16.0;

/// The open sections in window order, the picked preset (a built-in's
/// name, "Hands", "Pong", "Head", or a saved preset's) and the open
/// sections folded to their header. Profiles keep one each (profile.rs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub sections: Vec<Section>,
    pub preset: Option<String>,
    pub folded: Vec<Section>,
}

/// params `view`: `{"version":1,"sections":["hands"],"preset":"Hands"}`,
/// with `"folded":["pong"]` while a section is folded.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    version: u32,
    sections: Vec<Section>,
    preset: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    folded: Vec<Section>,
}

impl Default for Stored {
    fn default() -> Self {
        Stored { version: VIEW_VERSION, sections: Vec::new(), preset: None, folded: Vec::new() }
    }
}

impl View {
    /// The view saved with the project; empty (old projects) or unreadable
    /// gives the default, which the window grows by the content
    /// (`with_content`).
    pub fn read(params: &SynthexisParams) -> View {
        let json = params.view.read().clone();
        if json.trim().is_empty() {
            return View::default();
        }
        match serde_json::from_str::<Stored>(&json) {
            Ok(s) => {
                let mut sections: Vec<Section> = Vec::new();
                for sec in s.sections {
                    if !sections.contains(&sec) {
                        sections.push(sec);
                    }
                }
                let mut folded: Vec<Section> = Vec::new();
                for sec in s.folded {
                    if !folded.contains(&sec) {
                        folded.push(sec);
                    }
                }
                View { sections, preset: s.preset.filter(|p| !p.trim().is_empty()), folded }
            }
            Err(e) => {
                log(&format!("view: not read ({e})"));
                View::default()
            }
        }
    }

    pub fn write(&self, params: &SynthexisParams) {
        let s = Stored { version: VIEW_VERSION, sections: self.sections.clone(), preset: self.preset.clone(), folded: self.folded.clone() };
        *params.view.write() = serde_json::to_string(&s).unwrap_or_default();
    }

    /// The open list grown by every section `s` plays that it misses
    /// (R5.7, R5.9): content always shows.
    pub fn with_content(mut self, s: &Snapshot) -> View {
        for sec in sections_of(s) {
            if !self.sections.contains(&sec) {
                self.sections.push(sec);
            }
        }
        self
    }

    /// Whether open section `sec` shows only its header.
    pub fn is_folded(&self, sec: Section) -> bool {
        self.folded.contains(&sec)
    }

    /// Folds `sec` to its header, or unfolds it.
    pub fn set_folded(&mut self, sec: Section, folded: bool) {
        self.folded.retain(|s| *s != sec);
        if folded {
            self.folded.push(sec);
        }
    }
}

/// What the column's methods work on. `discard` drops the chat field's
/// pending lines; it runs right before a state change, never when a load
/// fails.
pub struct Ctx<'a> {
    pub writer: &'a Writer,
    pub params: &'a SynthexisParams,
    pub t: &'a Text,
    pub discard: &'a mut dyn FnMut(),
}

struct Toast {
    text: String,
    error: bool,
    at: Instant,
}

/// What Undo puts back (R5.4).
struct Undo {
    before: Snapshot,
    view: View,
    file: Option<PathBuf>,
    loaded: Option<Snapshot>,
    /// The state right after the action; a different one starts the 10 s.
    after: Snapshot,
    /// What the action loaded, opened or closed (the hover text).
    what: String,
    changed_at: Option<Instant>,
}

enum Act {
    Pick(Entry),
    Delete(Entry),
}

/// The preset column's state; lives in the window's state as long as the
/// plugin instance.
#[derive(Default)]
pub struct Presets {
    /// `preset::list` as read last (read again after a save or delete).
    list: Option<Vec<Entry>>,
    /// The name field's text while saving.
    saving: Option<String>,
    /// Focus the name field on its next frame.
    focus: bool,
    toast: Option<Toast>,
    undo: Option<Undo>,
    /// The built-in row that asks for a second click, with its text.
    armed: Option<(Section, String)>,
    /// The file of the picked saved preset (a twin is told apart by it).
    file: Option<PathBuf>,
    /// The state right after the picked saved preset was loaded or saved.
    loaded: Option<Snapshot>,
    /// The state differs from `loaded` (checked by `tick`).
    edited: bool,
    last_tick: Option<Instant>,
    /// Tests: the presets folder and the FL bridge.
    dir: Option<PathBuf>,
    bridge: Option<&'static fl::Bridge>,
}

/// The section's name as the window shows it.
fn section_name(sec: Section, t: &Text) -> &'static str {
    match sec {
        Section::Hands => t.preset_hands,
        Section::Pong => t.preset_pong,
        Section::Head => t.preset_head,
    }
}

/// Two states compare without their saved sections (the view has those).
fn same(a: &Snapshot, b: &Snapshot) -> bool {
    a.routes == b.routes && a.midi == b.midi && a.pong == b.pong && a.pin == b.pin
}

/// The width `glass::quiet` takes for `text`.
fn quiet_w(ui: &Ui, text: &str) -> f32 {
    glass::galley(ui, text, Tok::BODY).size().x + QUIET_PAD
}

impl Presets {
    fn dir(&self) -> PathBuf {
        self.dir.clone().unwrap_or_else(super::dir)
    }

    fn bridge(&self) -> &'static fl::Bridge {
        self.bridge.unwrap_or_else(fl::bridge)
    }

    fn entries(&mut self) -> Vec<Entry> {
        let dir = self.dir.clone();
        self.list.get_or_insert_with(|| dir.as_deref().map_or_else(super::list, super::list_in)).clone()
    }

    fn say(&mut self, text: String, error: bool) {
        self.toast = Some(Toast { text, error, at: Instant::now() });
    }

    /// The picked preset as the picker shows it, and whether that is the
    /// placeholder (or the edited marker, drawn dimmer).
    fn label(&self, view: &View, t: &Text) -> (String, bool) {
        let Some(name) = view.preset.as_deref() else { return (t.preset_picker.into(), true) };
        let shown = match (Section::of_builtin(name), &self.file) {
            (Some(sec), None) => section_name(sec, t).to_string(),
            (Some(_), Some(_)) => fill(t.preset_saved_twin, &[&name]),
            (None, _) => name.to_string(),
        };
        if self.edited {
            (fill(t.preset_edited, &[&shown]), true)
        } else {
            (shown, false)
        }
    }

    /// The picked preset is a saved one: its file, found by name after a
    /// project load.
    fn resolve_file(&mut self, view: &View) {
        if self.file.is_some() {
            return;
        }
        let Some(name) = view.preset.clone().filter(|n| !is_builtin(n)) else { return };
        self.file = self.entries().into_iter().find(|e| !e.builtin && !e.twin && fold(&e.name) == fold(&name)).and_then(|e| e.file);
    }

    fn selected(&self, e: &Entry, view: &View) -> bool {
        match (&e.file, &self.file) {
            (Some(f), Some(mine)) => f == mine,
            (None, None) => view.preset.as_deref().is_some_and(|p| fold(p) == fold(&e.name)),
            _ => false,
        }
    }

    /// Applies `next` (pending lines discarded first) and makes `new_view`
    /// the view, written to params right after; Undo goes back to now and
    /// the toast says `msg`. When `next` plays what the state plays now
    /// and `always` is false, only the view changes. False (an error
    /// toast) when the state was refused.
    fn change(&mut self, next: &Snapshot, new_view: View, view: &mut View, msg: String, always: bool, cx: &mut Ctx) -> bool {
        let now = capture(cx.writer);
        let before = if !always && same(next, &now) {
            now
        } else {
            if let Err(e) = super::check(next) {
                self.say(e, true);
                return false;
            }
            (cx.discard)();
            match super::apply_on(cx.writer, next, self.bridge()) {
                Ok(before) => before,
                Err(e) => {
                    self.say(e, true);
                    return false;
                }
            }
        };
        self.undo = Some(Undo {
            before,
            view: view.clone(),
            file: self.file.clone(),
            loaded: self.loaded.clone(),
            after: capture(cx.writer),
            what: msg.clone(),
            changed_at: None,
        });
        *view = new_view;
        view.write(cx.params);
        self.say(msg, false);
        true
    }

    fn run_undo(&mut self, view: &mut View, cx: &mut Ctx) {
        let Some(u) = self.undo.take() else { return };
        if !same(&capture(cx.writer), &u.before) {
            (cx.discard)();
            if let Err(e) = super::apply_on(cx.writer, &u.before, self.bridge()) {
                self.say(e, true);
                self.undo = Some(u);
                return;
            }
        }
        *view = u.view;
        view.write(cx.params);
        (self.file, self.loaded, self.edited) = (u.file, u.loaded, false);
        self.say(cx.t.p_load_undone.into(), false);
    }

    /// The top bar's profile `to` (0-based) was clicked (profile.rs): the
    /// state and the view go into the active profile, `to`'s come in, the
    /// chat field's pending lines dropped first. Undo, the picked file and
    /// the edited marker belong to the profile left, so they go; the toast
    /// names the profile. The active one again does nothing.
    pub fn switch_profile(&mut self, to: usize, view: &mut View, cx: &mut Ctx) {
        if super::profile::Profiles::read(cx.params).active == to {
            return;
        }
        match super::profile::switch_on(cx.writer, view, to, self.bridge(), &mut *cx.discard) {
            Ok(s) => {
                *view = s.view;
                (self.undo, self.file, self.loaded, self.edited, self.armed, self.saving) = (None, None, None, false, None, None);
                let n = to + 1;
                self.say(fill(if s.fresh { cx.t.profile_fresh } else { cx.t.profile_switched }, &[&n]), false);
            }
            Err(e) => self.say(e, true),
        }
    }

    // --- picker ---------------------------------------------------------------

    /// The preset picker (R4, R5.2, R5.3): built-ins first ("Built-in"),
    /// then "Saved presets" with the two-click Delete.
    pub fn picker(&mut self, ui: &mut Ui, size: Vec2, view: &mut View, cx: &mut Ctx) {
        self.resolve_file(view);
        let t = cx.t;
        let id = Id::new("preset-picker");
        let (text, dim) = self.label(view, t);
        let button = glass::picker_button(ui, id, size, &text, dim, "");
        let entries = self.entries();
        let armed = self.armed.clone();
        let rows = |ui: &mut Ui| {
            let mut act = None;
            let mut header = false;
            for e in &entries {
                if !e.builtin && !header {
                    glass::pick_header(ui, t.preset_saved);
                    header = true;
                }
                // an armed row asks over its whole width, the full text on hover
                let arm = match (&armed, Section::of_builtin(&e.name)) {
                    (Some((a, text)), Some(sec)) if e.builtin && *a == sec => Some(text.as_str()),
                    _ => None,
                };
                let name = match (e.builtin, Section::of_builtin(&e.name)) {
                    (true, Some(sec)) => arm.map_or_else(|| section_name(sec, t).to_string(), String::from),
                    _ if e.twin => fill(t.preset_saved_twin, &[&e.name]),
                    _ => e.name.clone(),
                };
                let place = if e.builtin && arm.is_none() { t.preset_builtin } else { "" };
                let delete = (!e.builtin).then_some((t.delete, t.delete_arm));
                let r = glass::pick_row(ui, id.with(("row", &e.name, &e.file)), self.selected(e, view), true, &name, place, arm.unwrap_or(""), delete);
                if r.delete {
                    act = Some(Act::Delete(e.clone()));
                } else if r.clicked {
                    act = Some(Act::Pick(e.clone()));
                }
            }
            act
        };
        match glass::picker_popup_up(ui, id, &button, POP_H, rows).flatten() {
            Some(Act::Pick(e)) => {
                if self.pick(&e, view, cx) {
                    ui.memory_mut(|m| m.close_popup());
                }
            }
            Some(Act::Delete(e)) => self.delete(&e, view, cx),
            None => {}
        }
        if !ui.memory(|m| m.is_popup_open(id.with("popup"))) {
            self.armed = None;
        }
    }

    /// A picker row was clicked; false while it waits for its second click.
    fn pick(&mut self, e: &Entry, view: &mut View, cx: &mut Ctx) -> bool {
        match (e.builtin, Section::of_builtin(&e.name), &e.file) {
            (true, Some(sec), _) => self.pick_builtin(sec, view, cx),
            (false, _, Some(path)) => {
                self.armed = None;
                self.pick_saved(path, view, cx);
                true
            }
            _ => true,
        }
    }

    /// A saved preset replaces the whole state, as Load did (R5.3).
    fn pick_saved(&mut self, path: &Path, view: &mut View, cx: &mut Ctx) {
        let again = self.file.as_deref() == Some(path) && view.preset.is_some();
        let p = match super::load_file(path) {
            Ok(p) => p,
            Err(e) => {
                self.list = None;
                return self.say(e, true);
            }
        };
        let shown = if is_builtin(&p.name) { fill(cx.t.preset_saved_twin, &[&p.name]) } else { p.name.clone() };
        let new_view = View { sections: super::opens(&p.snapshot), preset: Some(p.name.clone()), ..view.clone() };
        let msg = fill(if again { cx.t.reloaded } else { cx.t.p_loaded }, &[&shown]);
        // a saved preset replaces everything, also when it plays the same
        if self.change(&p.snapshot, new_view, view, msg, true, cx) {
            self.file = Some(path.to_path_buf());
            self.loaded = Some(capture(cx.writer));
            self.edited = false;
        }
    }

    /// A built-in: it becomes the only open section, the others close (a
    /// second click when one of them plays something); Pong brings its
    /// part only into an empty Pong; the shown Pong reloads (R5.3).
    fn pick_builtin(&mut self, sec: Section, view: &mut View, cx: &mut Ctx) -> bool {
        let t = cx.t;
        let name = section_name(sec, t);
        let now = capture(cx.writer);
        let open = view.clone().with_content(&now).sections;
        let shown = self.file.is_none() && view.preset.as_deref().and_then(Section::of_builtin) == Some(sec);
        if open == [sec] && shown {
            self.armed = None;
            if sec == Section::Pong {
                let next = with_section(now, sec);
                let new_view = View { sections: open, ..view.clone() };
                self.change(&next, new_view, view, fill(t.reloaded, &[&name]), false, cx);
            }
            return true;
        }
        let others: Vec<Section> = open.iter().copied().filter(|s| *s != sec).collect();
        let losing: Vec<&str> = others.iter().filter(|s| has_content(&now, **s)).map(|s| section_name(*s, t)).collect();
        if !losing.is_empty() && self.armed.as_ref().map(|a| a.0) != Some(sec) {
            self.armed = Some((sec, fill(t.replace_arm, &[&losing.join(", ")])));
            return false;
        }
        self.armed = None;
        let mut next = others.iter().fold(now.clone(), |s, o| without_section(s, *o));
        let merge = sec == Section::Pong && !has_content(&next, sec);
        if merge {
            next = with_section(next, sec);
        }
        let new_view = View { sections: vec![sec], preset: Some(sec.builtin_name().into()), ..view.clone() };
        let msg = fill(if merge { t.p_loaded } else { t.section_opened }, &[&name]);
        if self.change(&next, new_view, view, msg, false, cx) {
            (self.file, self.loaded, self.edited) = (None, None, false);
        }
        true
    }

    fn delete(&mut self, e: &Entry, view: &mut View, cx: &Ctx) {
        let Some(path) = &e.file else { return };
        match super::delete_file(path) {
            Ok(()) => {
                self.list = None;
                if self.file.as_ref() == Some(path) {
                    (self.file, self.loaded, self.edited) = (None, None, false);
                    view.preset = None;
                    view.write(cx.params);
                }
                self.say(fill(cx.t.p_deleted, &[&e.name]), false);
            }
            Err(err) => self.say(err, true),
        }
    }

    // --- "+" ------------------------------------------------------------------

    /// "+" (R5.5): Hands, Pong and Head, an open one greyed out. HINT while no
    /// section is open, VALUE with the selection edge once one is.
    pub fn plus(&mut self, ui: &mut Ui, view: &mut View, cx: &mut Ctx) {
        let t = cx.t;
        let id = Id::new("preset-plus");
        let button = glass::plus_button(ui, !view.sections.is_empty(), t.add_section);
        let open = view.sections.clone();
        let rows = |ui: &mut Ui| {
            let mut picked = None;
            for sec in Section::ALL {
                let on = open.contains(&sec);
                let hover = if on { t.section_open } else { "" };
                if glass::pick_row(ui, id.with(sec), false, !on, section_name(sec, t), "", hover, None).clicked {
                    picked = Some(sec);
                }
            }
            picked
        };
        if let Some(sec) = glass::picker_popup_up(ui, id, &button, POP_H, rows).flatten() {
            ui.memory_mut(|m| m.close_popup());
            self.add(sec, view, cx);
        }
    }

    /// Adds `sec` after the open sections: an empty Pong gets the
    /// built-in's part (with Undo); anything else only opens.
    fn add(&mut self, sec: Section, view: &mut View, cx: &mut Ctx) {
        if view.sections.contains(&sec) {
            return;
        }
        let name = section_name(sec, cx.t);
        let mut new_view = view.clone();
        new_view.sections.push(sec);
        new_view.set_folded(sec, false);
        let now = capture(cx.writer);
        let merge = sec == Section::Pong && !has_content(&now, sec);
        let next = if merge { with_section(now, sec) } else { now };
        let msg = fill(if merge { cx.t.p_loaded } else { cx.t.section_opened }, &[&name]);
        self.change(&next, new_view, view, msg, false, cx);
    }

    // --- row 2 ----------------------------------------------------------------

    /// Row 2 under the picker, `width` (the picker's) by `CTRL_H`: Save as
    /// at the left and Undo at the right; while saving the name field and
    /// Save or Overwrite (R4).
    pub fn row2(&mut self, ui: &mut Ui, width: f32, view: &mut View, cx: &mut Ctx) {
        let t = cx.t;
        let (row, _) = ui.allocate_exact_size(vec2(width, Tok::CTRL_H), Sense::hover());
        let child = |ui: &mut Ui, layout: Layout| {
            let mut c = ui.new_child(UiBuilder::new().max_rect(row).layout(layout));
            c.spacing_mut().item_spacing.x = Tok::GAP;
            c
        };
        if let Some(mut name) = self.saving.take() {
            let over = self.entries().iter().any(|e| !e.builtin && !e.twin && fold(&e.name) == fold(&name));
            let label = if over { t.overwrite } else { t.save };
            let field_w = (width - quiet_w(ui, label) - Tok::GAP).max(0.0);
            let mut c = child(ui, Layout::left_to_right(Align::Center));
            let field = glass::text_field(&mut c, Id::new("preset-save-name"), &mut name, field_w, t.p_name_hint);
            if std::mem::take(&mut self.focus) {
                field.request_focus();
            }
            let button = glass::quiet(&mut c, label);
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if button.clicked() || enter {
                self.save(name, view, cx);
            } else if !(esc || field.clicked_elsewhere() && !button.contains_pointer()) {
                self.saving = Some(name);
            }
            return;
        }
        let mut left = child(ui, Layout::left_to_right(Align::Center));
        // its text starts at the column edge, like the plugin note
        if glass::quiet_lead(&mut left, t.save_as).clicked() {
            let picked = self.file.as_ref().and(view.preset.clone()).or_else(|| view.preset.clone().filter(|p| !is_builtin(p)));
            self.saving = Some(picked.unwrap_or_default());
            self.focus = true;
        }
        if let Some(what) = self.undo.as_ref().map(|u| u.what.clone()) {
            let mut right = child(ui, Layout::right_to_left(Align::Center));
            if glass::quiet(&mut right, t.undo).on_hover_text(fill(t.undo_hover, &[&format!("\"{what}\"")])).clicked() {
                self.run_undo(view, cx);
            }
        }
    }

    /// Saves the state and the open sections as `name`; a built-in or an
    /// empty name keeps the field open with an error toast.
    fn save(&mut self, name: String, view: &mut View, cx: &mut Ctx) {
        let t = cx.t;
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            self.saving = Some(name);
            return self.say(t.p_need_name.into(), true);
        }
        if is_builtin(&trimmed) {
            self.saving = Some(name);
            return self.say(fill(t.p_builtin_name, &[&trimmed]), true);
        }
        let mut snap = capture(cx.writer);
        snap.sections = view.sections.clone();
        if let Err(e) = super::save_in(&self.dir(), &trimmed, &snap) {
            self.saving = Some(name);
            return self.say(e, true);
        }
        self.list = None;
        self.file = self.entries().into_iter().find(|e| !e.builtin && !e.twin && fold(&e.name) == fold(&trimmed)).and_then(|e| e.file);
        view.preset = Some(trimmed.clone());
        view.write(cx.params);
        (self.undo, self.loaded, self.edited) = (None, Some(capture(cx.writer)), false);
        self.say(fill(t.p_saved, &[&trimmed]), false);
    }

    // --- close, tick, toast ---------------------------------------------------

    /// Closes `s` (the confirming click on its header's `x`, R5.6): what
    /// it plays goes, with Undo; toast "Closed X".
    pub fn close(&mut self, view: &mut View, s: Section, cx: &mut Ctx) {
        let name = section_name(s, cx.t);
        let next = without_section(capture(cx.writer), s);
        let mut new_view = view.clone();
        new_view.sections.retain(|x| *x != s);
        new_view.set_folded(s, false);
        if self.file.is_none() && new_view.preset.as_deref().and_then(Section::of_builtin) == Some(s) {
            new_view.preset = None;
        }
        self.change(&next, new_view, view, fill(cx.t.section_closed, &[&name]), false, cx);
    }

    /// Text for the close arm ("Close Hands? 3 parts go").
    pub fn close_text(&self, s: Section, cx: &Ctx) -> String {
        match s {
            Section::Hands => {
                let now = capture(cx.writer);
                let parts = now.midi.parts.len();
                let gone = parts - without_section(now, s).midi.parts.len().min(parts);
                fill(cx.t.close_arm_hands, &[&gone])
            }
            Section::Pong => cx.t.close_arm_pong.into(),
            Section::Head => {
                let now = capture(cx.writer);
                let lines = now.routes.len() + now.midi.binds.len();
                let after = without_section(now, s);
                fill(cx.t.close_arm_head, &[&lines.saturating_sub(after.routes.len() + after.midi.binds.len())])
            }
        }
    }

    /// Once a second: drops Undo per R5.4 and refreshes the edited marker.
    pub fn tick(&mut self, now: Instant, cx: &Ctx) {
        if self.last_tick.is_some_and(|l| now.saturating_duration_since(l) < TICK) {
            return;
        }
        self.last_tick = Some(now);
        if self.undo.is_none() && self.loaded.is_none() {
            return;
        }
        let cur = capture(cx.writer);
        if let Some(u) = &mut self.undo {
            if !same(&cur, &u.after) {
                let since = *u.changed_at.get_or_insert(now);
                if now.saturating_duration_since(since) >= UNDO_IDLE {
                    self.undo = None;
                }
            }
        }
        self.edited = self.loaded.as_ref().is_some_and(|b| !same(b, &cur));
    }

    /// The preset and section messages (2.9) at the bottom left of `window`.
    pub fn toast(&mut self, ctx: &egui::Context, window: Rect) {
        let Some(t) = &self.toast else { return };
        let age = t.at.elapsed().as_secs_f32();
        if glass::toast(ctx, window, &t.text, t.error, age) || (!t.error && age > TOAST_GONE) {
            self.toast = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nih_plug_egui::egui::{pos2, Color32, Event, Modifiers, Pos2, RawInput};

    use super::super::tests::{folder, json_files, line, mixed, topos, writer};
    use super::super::{load_file, opens, save_in};
    use super::*;
    use crate::i18n::{DE, EN};
    use crate::pong::arena::Side;
    use crate::pong::setup::{Action, Setup};
    use crate::pong::run::Input;
    use crate::saved;

    /// The picker's width at W 1200 (2.5).
    const P: f32 = 204.0;

    /// One instance's column: the writer, the column, the view and how
    /// often the field's pending lines were dropped.
    struct St {
        w: Arc<Writer>,
        p: Presets,
        view: View,
        discards: u32,
        t: &'static Text,
    }

    impl St {
        fn new(name: &str) -> St {
            let p = Presets { dir: Some(folder(&format!("top-{name}"))), bridge: Some(fl::Bridge::detached()), ..Presets::default() };
            St { w: writer(), p, view: View::default(), discards: 0, t: &EN }
        }

        /// Runs `f` with a Ctx over this instance.
        fn with<R>(&mut self, f: impl FnOnce(&mut Presets, &mut View, &mut Ctx) -> R) -> R {
            let n = &mut self.discards;
            let mut d = || *n += 1;
            let mut cx = Ctx { writer: &self.w, params: &self.w.params, t: self.t, discard: &mut d };
            f(&mut self.p, &mut self.view, &mut cx)
        }

        /// The column as the window lays it out: picker and "+" in row 1,
        /// row 2 under the picker.
        fn draw(&mut self, ui: &mut Ui) {
            self.draw_at(ui, P);
        }

        fn draw_at(&mut self, ui: &mut Ui, width: f32) {
            self.with(|p, view, cx| {
                ui.horizontal(|ui| {
                    p.picker(ui, vec2(width, Tok::TOP_H), view, cx);
                    p.plus(ui, view, cx);
                });
                p.row2(ui, width, view, cx);
            });
        }

        fn tick(&mut self, now: Instant) {
            self.with(|p, _, cx| p.tick(now, cx));
        }

        fn toast(&self) -> Option<(String, bool)> {
            self.p.toast.as_ref().map(|t| (t.text.clone(), t.error))
        }

        fn state(&self) -> Snapshot {
            capture(&self.w)
        }
    }

    struct Kit {
        ctx: egui::Context,
        time: f64,
        out: Option<egui::FullOutput>,
    }

    /// A painted text: string, rect, colour.
    type Painted = (String, Rect, Color32);

    impl Kit {
        fn new() -> Self {
            let ctx = egui::Context::default();
            glass::style(&ctx);
            Kit { ctx, time: 0.0, out: None }
        }

        fn frame(&mut self, events: Vec<Event>, st: &mut St) {
            self.frame_with(events, &mut |ui| st.draw(ui));
        }

        fn frame_with(&mut self, events: Vec<Event>, f: &mut dyn FnMut(&mut Ui)) {
            self.time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(self.time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))), ..Default::default() };
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| f(ui));
            }));
        }

        fn click(&mut self, at: Pos2, st: &mut St) {
            self.click_with(at, &mut |ui| st.draw(ui));
        }

        fn click_with(&mut self, at: Pos2, f: &mut dyn FnMut(&mut Ui)) {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame_with(vec![Event::PointerMoved(at), button(true)], f);
            self.frame_with(vec![button(false)], f);
            self.frame_with(vec![], f);
        }

        /// Clicks the text `needle` (the last drawn: a popup row over
        /// its picker's label); false when it is not drawn.
        fn click_text(&mut self, needle: &str, st: &mut St) -> bool {
            match self.texts().into_iter().rev().find(|(s, _, _)| s == needle).map(|(_, r, _)| r) {
                Some(r) => {
                    self.click(r.center(), st);
                    true
                }
                None => false,
            }
        }

        fn texts(&self) -> Vec<Painted> {
            let mut v = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk(&c.shape, &mut v);
            }
            v
        }

        fn names(&self) -> Vec<String> {
            self.texts().into_iter().map(|(s, _, _)| s).collect()
        }

        fn find(&self, needle: &str) -> Option<Rect> {
            self.texts().into_iter().find(|(s, _, _)| s == needle).map(|(_, r, _)| r)
        }

        fn alpha(&self, needle: &str) -> Option<f32> {
            self.texts().into_iter().find(|(s, _, _)| s == needle).map(|(_, _, c)| f32::from(c.a()) / 255.0)
        }

        /// The bounding rect of everything painted.
        fn bounds(&self) -> Rect {
            let mut r = Rect::NOTHING;
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                let b = c.shape.visual_bounding_rect();
                if b.is_positive() {
                    r = r.union(b);
                }
            }
            r
        }
    }

    fn walk(s: &egui::Shape, out: &mut Vec<Painted>) {
        match s {
            egui::Shape::Text(t) => {
                let r = Rect::from_min_size(t.pos + t.galley.rect.min.to_vec2(), t.galley.rect.size());
                out.push((t.galley.text().to_string(), r, t.override_text_color.unwrap_or(t.fallback_color)));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }

    /// Opens the picker (its label `label`) and clicks the row `row`.
    fn pick(k: &mut Kit, st: &mut St, label: &str, row: &str) {
        k.frame(vec![], st);
        if !k.ctx.memory(|m| m.is_popup_open(Id::new("preset-picker").with("popup"))) {
            assert!(k.click_text(label, st), "picker {label:?} not drawn: {:?}", k.names());
        }
        assert!(k.click_text(row, st), "row {row:?} not drawn: {:?}", k.names());
    }

    /// A kept line no section holds ("the object's x drives the drive").
    fn other_line() -> saved::Saved {
        let mut l = line(&topos(0, 0), 7);
        l.route.source = "object.x".into();
        l
    }

    /// Kept lines as their routes, without line and batch ids (apply
    /// renumbers them).
    fn routes_of(s: &Snapshot) -> Vec<String> {
        s.routes.iter().map(|l| format!("{:?}", l.route)).collect()
    }

    /// The same configuration; apply renumbers the kept lines.
    fn alike(a: &Snapshot, b: &Snapshot) -> bool {
        routes_of(a) == routes_of(b) && a.midi == b.midi && a.pong == b.pong && a.pin == b.pin
    }

    fn near(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 0.01)
    }

    fn state_with(w: &Writer, s: &Snapshot) {
        assert!(super::super::apply_on(w, s, fl::Bridge::detached()).is_ok());
    }

    #[test]
    fn view_round_trips_and_an_empty_field_derives_from_content() {
        let st = St::new("view");
        let params = &st.w.params;
        assert_eq!(View::read(params), View::default(), "a new instance");
        let v = View { sections: vec![Section::Hands], preset: Some("Hands".into()), ..Default::default() };
        v.write(params);
        assert_eq!(*params.view.read(), r#"{"version":1,"sections":["hands"],"preset":"Hands"}"#);
        assert_eq!(View::read(params), v);
        let two = View { sections: vec![Section::Pong, Section::Hands], preset: None, ..Default::default() };
        two.write(params);
        assert_eq!(View::read(params), two);
        // unreadable or a section twice: still a view
        *params.view.write() = "{".into();
        assert_eq!(View::read(params), View::default());
        *params.view.write() = r#"{"version":1,"sections":["pong","pong"]}"#.into();
        assert_eq!(View::read(params).sections, [Section::Pong]);
        // an old project: empty, the window opens what the state plays
        *params.view.write() = String::new();
        state_with(&st.w, &mixed());
        let got = View::read(params).with_content(&st.state());
        assert_eq!(got.sections, [Section::Hands, Section::Pong]);
        assert_eq!(got.sections, opens(&st.state()));
        // a saved order stays first
        let v = View { sections: vec![Section::Pong], preset: None, ..Default::default() }.with_content(&st.state());
        assert_eq!(v.sections, [Section::Pong, Section::Hands]);
    }

    /// A folded section is saved with the view, so a reopened project
    /// shows it folded; a view without folds writes the JSON of before.
    #[test]
    fn folded_sections_are_saved_with_the_view() {
        let st = St::new("folded");
        let params = &st.w.params;
        let mut v = View { sections: vec![Section::Hands, Section::Pong], preset: None, ..Default::default() };
        v.set_folded(Section::Pong, true);
        v.set_folded(Section::Pong, true);
        assert!(v.is_folded(Section::Pong) && !v.is_folded(Section::Hands));
        v.write(params);
        assert_eq!(*params.view.read(), r#"{"version":1,"sections":["hands","pong"],"preset":null,"folded":["pong"]}"#);
        assert_eq!(View::read(params), v);
        v.set_folded(Section::Pong, false);
        v.write(params);
        assert_eq!(*params.view.read(), r#"{"version":1,"sections":["hands","pong"],"preset":null}"#);
        assert!(!View::read(params).is_folded(Section::Pong));
        // an old project's view has no folds
        *params.view.write() = r#"{"version":1,"sections":["head"]}"#.into();
        assert!(View::read(params).folded.is_empty());
    }

    #[test]
    fn the_plus_menu_greys_open_sections() {
        let mut st = St::new("plus");
        let mut k = Kit::new();
        k.frame(vec![], &mut st);
        assert!(near(k.alpha("+"), Tok::HINT), "nothing open: HINT");
        st.view.sections = vec![Section::Hands];
        k.frame(vec![], &mut st);
        assert!(near(k.alpha("+"), Tok::VALUE), "a section open: VALUE");
        assert!(k.click_text("+", &mut st));
        // the popup has faded in
        for _ in 0..30 {
            k.frame(vec![], &mut st);
        }
        let rows = k.names();
        assert!(rows.contains(&"Hands".to_string()) && rows.contains(&"Pong".to_string()), "{rows:?}");
        let hands = k.texts().into_iter().filter(|(s, _, _)| s == "Hands").map(|(_, _, c)| f32::from(c.a()) / 255.0).fold(1.0_f32, f32::min);
        assert!((hands - Tok::OFF).abs() < 0.01, "the open Hands row is greyed: {hands}");
        assert!(near(k.alpha("Pong"), Tok::VALUE));
        // a click on the grey row does nothing
        let at = k.texts().into_iter().filter(|(s, _, _)| s == "Hands").map(|(_, r, _)| r).next_back();
        k.click(at.map_or(Pos2::ZERO, |r| r.center()), &mut st);
        assert_eq!(st.view.sections, [Section::Hands]);
        // Pong is added after Hands; the empty Pong gets the built-in's part
        if k.find("Pong").is_none() {
            assert!(k.click_text("+", &mut st));
        }
        assert!(k.click_text("Pong", &mut st), "{:?}", k.names());
        assert_eq!(st.view.sections, [Section::Hands, Section::Pong]);
        assert_eq!(*st.w.params.view.read(), r#"{"version":1,"sections":["hands","pong"],"preset":null}"#, "written in the same call");
        assert_eq!(st.state().pong.wall_parts().len(), 2);
        assert_eq!(st.toast(), Some(("Loaded Pong".into(), false)));
        assert!(st.p.undo.is_some(), "+ offers Undo");
    }

    #[test]
    fn save_as_writes_the_open_sections() {
        let mut st = St::new("save-sections");
        let mut k = Kit::new();
        st.with(|p, view, cx| p.add(Section::Pong, view, cx));
        st.with(|p, view, cx| p.add(Section::Hands, view, cx));
        assert_eq!(st.view.sections, [Section::Pong, Section::Hands]);
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save_as, &mut st), "{:?}", k.names());
        assert_eq!(st.p.saving.as_deref(), Some(""), "no saved preset picked: empty");
        k.frame(vec![Event::Text("Two sections".into())], &mut st);
        assert_eq!(st.p.saving.as_deref(), Some("Two sections"), "the field has focus");
        let enter = Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        k.frame(vec![enter], &mut st);
        assert_eq!(st.toast(), Some(("Saved Two sections".into(), false)));
        assert!(st.p.saving.is_none());
        let file = st.p.file.clone();
        let back = file.as_deref().map(load_file);
        assert_eq!(back.map(|r| r.map(|p| p.snapshot.sections)), Some(Ok(vec![Section::Pong, Section::Hands])));
        assert_eq!(st.view.preset.as_deref(), Some("Two sections"));
        assert!(st.w.params.view.read().contains(r#""preset":"Two sections""#));
        assert!(st.p.undo.is_none(), "a save ends Undo");
    }

    #[test]
    fn save_as_is_visible_in_every_state() {
        let mut st = St::new("save-visible");
        let mut k = Kit::new();
        k.frame(vec![], &mut st);
        assert!(k.find(EN.save_as).is_some() && k.find(EN.undo).is_none(), "idle: {:?}", k.names());
        pick(&mut k, &mut st, EN.preset_picker, "Pong");
        k.frame(vec![], &mut st);
        assert!(k.find(EN.save_as).is_some() && k.find(EN.undo).is_some(), "with Undo: {:?}", k.names());
        // the picker popup open does not hide it
        assert!(k.click_text("Pong", &mut st));
        assert!(k.find(EN.preset_hands).is_some() && k.find(EN.save_as).is_some(), "{:?}", k.names());
        k.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], &mut st);
        // the popup leaves the hit test a frame later
        k.frame(vec![], &mut st);
        // saving: the field and its button take the row
        assert!(k.click_text(EN.save_as, &mut st));
        assert!(k.find(EN.p_name_hint).is_some() && k.find(EN.save).is_some() && k.find(EN.save_as).is_none(), "{:?}", k.names());
        // a click elsewhere cancels, Save as is back
        k.click(pos2(600.0, 500.0), &mut st);
        k.frame(vec![], &mut st);
        assert!(st.p.saving.is_none());
        assert!(k.find(EN.save_as).is_some(), "{:?}", k.names());
        // edited
        st.p.edited = true;
        k.frame(vec![], &mut st);
        assert!(k.find(EN.save_as).is_some());
    }

    #[test]
    fn row2_fits_in_english_and_german_at_170_and_204() {
        for t in [&EN, &DE] {
            for width in [170.0_f32, 204.0] {
                let mut st = St::new("row2");
                st.t = t;
                assert!(save_in(&st.p.dir(), "Live set", &Snapshot::default()).is_ok());
                st.with(|p, view, cx| p.add(Section::Hands, view, cx));
                assert!(st.p.undo.is_some());
                let states: [(&str, Option<&str>, Vec<&str>); 4] = [
                    ("idle", None, vec![t.save_as]),
                    ("undo", None, vec![t.save_as, t.undo]),
                    ("saving", Some(""), vec![t.p_name_hint, t.save]),
                    ("overwrite", Some("Live set"), vec!["Live set", t.overwrite]),
                ];
                for (what, saving, want) in states {
                    if what == "idle" {
                        st.p.undo = None;
                    } else if what == "undo" {
                        st.with(|p, view, cx| p.add(Section::Pong, view, cx));
                    }
                    st.p.saving = saving.map(String::from);
                    let mut k = Kit::new();
                    let label = if what == "overwrite" { t.overwrite } else { t.save };
                    let mut field_w = 0.0;
                    let mut f = |ui: &mut Ui| {
                        field_w = width - quiet_w(ui, label) - Tok::GAP;
                        st.with(|p, view, cx| p.row2(ui, width, view, cx));
                    };
                    k.frame_with(vec![], &mut f);
                    k.frame_with(vec![], &mut f);
                    let row = Rect::from_min_size(Pos2::ZERO, vec2(width, Tok::CTRL_H));
                    let b = k.bounds();
                    assert!(row.expand(0.5).contains_rect(b), "{} {what} at {width}: {b:?} past {row:?}", t.save);
                    let texts: Vec<Painted> = want.iter().filter_map(|w| k.texts().into_iter().find(|(s, _, _)| s == w)).collect();
                    assert_eq!(texts.len(), want.len(), "{} {what} at {width}: {:?}", t.save, k.names());
                    // Save as starts at the column edge, like the plugin note
                    if let (None, Some((_, a, _))) = (saving, texts.first()) {
                        assert!((a.left() - row.left()).abs() <= 0.5, "{} at {width}: {a:?} vs {row:?}", t.save_as);
                    }
                    // left to right with the gap between, no text cut
                    if let [(_, a, _), (_, b, _)] = texts.as_slice() {
                        assert!(a.right() + Tok::GAP <= b.left(), "{} {what} at {width}: {a:?} {b:?}", t.save);
                    }
                    if saving.is_some() {
                        assert!(field_w >= 48.0, "{} {what} at {width}: field {field_w}", t.save);
                    }
                }
                let _ = std::fs::remove_dir_all(st.p.dir());
            }
        }
    }

    #[test]
    fn the_builtin_name_is_refused() {
        let mut st = St::new("builtin-name");
        let mut k = Kit::new();
        st.p.saving = Some("  pong ".into());
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save, &mut st), "{:?}", k.names());
        assert_eq!(st.toast(), Some((fill(EN.p_builtin_name, &[&"pong"]), true)));
        assert_eq!(st.p.saving.as_deref(), Some("  pong "), "the field stays open");
        assert!(json_files(&st.p.dir()).is_empty());
        st.p.saving = Some("HANDS".into());
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save, &mut st));
        assert_eq!(st.toast(), Some((fill(EN.p_builtin_name, &[&"HANDS"]), true)));
        assert!(json_files(&st.p.dir()).is_empty());
    }

    #[test]
    fn save_then_the_picker_lists_the_new_name() {
        let mut st = St::new("save-list");
        let mut k = Kit::new();
        st.p.saving = Some("Live set".into());
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save, &mut st), "{:?}", k.names());
        assert_eq!(st.toast(), Some(("Saved Live set".into(), false)));
        k.frame(vec![], &mut st);
        assert!(k.find("Live set").is_some(), "the picker shows it: {:?}", k.names());
        assert!(k.click_text("Live set", &mut st));
        let names = k.names();
        let at = |n: &str| names.iter().position(|s| s == n);
        assert!(at(EN.preset_hands) < at(EN.preset_saved) && at(EN.preset_saved) < names.iter().rposition(|s| s == "Live set"), "{names:?}");
        assert_eq!(names.iter().filter(|s| *s == EN.preset_builtin).count(), 3, "{names:?}");
        // the field starts with the picked name; it overwrites
        k.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], &mut st);
        // the popup leaves the hit test a frame later
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save_as, &mut st));
        assert_eq!(st.p.saving.as_deref(), Some("Live set"));
        assert!(k.find(EN.overwrite).is_some(), "{:?}", k.names());
        assert_eq!(json_files(&st.p.dir()), ["live-set.json"]);
    }

    #[test]
    fn a_failed_load_keeps_the_pending_lines() {
        let mut st = St::new("failed-load");
        let mut k = Kit::new();
        assert!(save_in(&st.p.dir(), "Gone soon", &Snapshot::default()).is_ok());
        state_with(&st.w, &mixed());
        let before = st.state();
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.preset_picker, &mut st));
        assert!(k.find("Gone soon").is_some(), "{:?}", k.names());
        assert!(std::fs::remove_file(st.p.dir().join("gone-soon.json")).is_ok());
        assert!(k.click_text("Gone soon", &mut st));
        assert_eq!(st.discards, 0, "nothing loaded, nothing discarded");
        assert!(st.toast().is_some_and(|t| t.1), "{:?}", st.toast());
        assert!(alike(&st.state(), &before));
        assert!(st.p.undo.is_none() && st.view.preset.is_none());
        // a preset that loads drops them first
        assert!(save_in(&st.p.dir(), "Here", &Snapshot::default()).is_ok());
        st.p.list = None;
        pick(&mut k, &mut st, EN.preset_picker, "Here");
        assert_eq!(st.discards, 1);
        assert_eq!(st.toast(), Some(("Loaded Here".into(), false)));
    }

    #[test]
    fn picking_hands_keeps_pong_parts_until_the_second_click() {
        let mut st = St::new("hands-arm");
        let mut k = Kit::new();
        let mut start = super::super::builtin("Pong").map(|p| p.snapshot).unwrap_or_default();
        start.routes = vec![other_line()];
        start.pin = Some(topos(3, 1).id());
        start.midi.key = crate::midi::Key { root: 4, scale: crate::midi::Scale::Dorian };
        state_with(&st.w, &start);
        st.view = View { sections: vec![Section::Pong], preset: Some("Pong".into()), ..Default::default() };
        let before = st.state();
        pick(&mut k, &mut st, EN.preset_pong, EN.preset_hands);
        let arm = fill(EN.replace_arm, &[&"Pong"]);
        assert!(k.find(&arm).is_some(), "the row asks: {:?}", k.names());
        assert!(alike(&st.state(), &before) && st.view.sections == [Section::Pong], "nothing changed yet");
        assert!(st.p.undo.is_none());
        assert!(k.click_text(&arm, &mut st), "{:?}", k.names());
        let got = st.state();
        assert_eq!(got.pong, Setup::default(), "Pong went");
        assert!(got.midi.parts.is_empty(), "with its wall parts");
        assert_eq!(routes_of(&got), routes_of(&before), "the object line stays");
        assert_eq!((got.pin.clone(), got.midi.key), (before.pin.clone(), before.midi.key), "pin and key stay");
        assert_eq!(st.view, View { sections: vec![Section::Hands], preset: Some("Hands".into()), ..Default::default() });
        assert_eq!(View::read(&st.w.params), st.view);
        assert_eq!(st.toast(), Some(("Opened Hands".into(), false)));
        assert!(k.find(&arm).is_none());
        // an empty Pong closes without asking
        let mut st = St::new("hands-no-arm");
        st.view.sections = vec![Section::Pong];
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_hands);
        assert_eq!(st.view.sections, [Section::Hands]);
    }

    #[test]
    fn head_opens_from_the_picker_and_its_lines_go_only_on_the_second_click() {
        let mut st = St::new("head-pick");
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_head);
        assert_eq!(st.view, View { sections: vec![Section::Head], preset: Some("Head".into()), ..Default::default() });
        assert_eq!(st.toast(), Some(("Opened Head".into(), false)));
        // a tilt line belongs to Head: picking Hands asks first
        let mut start = Snapshot::default();
        let mut tilt = line(&topos(0, 0), 4);
        tilt.route.source = "head.roll".into();
        start.routes = vec![tilt, other_line()];
        state_with(&st.w, &start);
        let before = st.state();
        pick(&mut k, &mut st, EN.preset_head, EN.preset_hands);
        let arm = fill(EN.replace_arm, &[&"Head"]);
        assert!(k.find(&arm).is_some(), "the row asks: {:?}", k.names());
        assert!(alike(&st.state(), &before), "nothing changed yet");
        assert!(k.click_text(&arm, &mut st), "{:?}", k.names());
        assert_eq!(routes_of(&st.state()), routes_of(&Snapshot { routes: vec![other_line()], ..Snapshot::default() }), "the tilt went, the object line stayed");
        assert_eq!(st.view.sections, [Section::Hands]);
    }

    #[test]
    fn picking_pong_merges_only_into_an_empty_pong() {
        let mut st = St::new("pong-merge");
        let mut k = Kit::new();
        let mut start = mixed();
        start.pong = Setup::default();
        start.midi.walls.clear();
        state_with(&st.w, &start);
        let before = st.state();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_pong);
        assert!(k.find(&fill(EN.replace_arm, &[&"Hands"])).is_some(), "Hands plays: a second click");
        assert!(k.click_text(&fill(EN.replace_arm, &[&"Hands"]), &mut st));
        let got = st.state();
        let built = super::super::builtin("Pong").map(|p| p.snapshot.pong).unwrap_or_default();
        assert_eq!(got.pong.wall_parts().len(), 2);
        assert_eq!(Setup { walls: built.walls.clone(), auto_channel: built.auto_channel.clone(), ..got.pong.clone() }, built);
        assert_eq!(routes_of(&got).len(), 1, "the hand line went, the object line stayed: {:?}", routes_of(&got));
        assert_eq!((got.pin.clone(), got.midi.key), (before.pin.clone(), before.midi.key));
        assert_eq!(st.view, View { sections: vec![Section::Pong], preset: Some("Pong".into()), ..Default::default() });
        assert_eq!(st.toast(), Some(("Loaded Pong".into(), false)));
        // a Pong with content only opens
        let mut st = St::new("pong-keep");
        let mut k = Kit::new();
        let mut mine = Snapshot::default();
        let id = mine.midi.add("Mine", crate::midi::Play::Note { pitch: 50, vel: 90 }, None);
        mine.pong = Setup { on: true, max: 3, ..Setup::default() };
        mine.pong.walls[Side::Left.index()] = Action::Part { id };
        state_with(&st.w, &mine);
        let before = st.state();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_pong);
        assert!(alike(&st.state(), &before), "my Pong stays");
        assert_eq!(st.view.sections, [Section::Pong]);
        assert_eq!(st.toast(), Some(("Opened Pong".into(), false)));
    }

    #[test]
    fn repicking_pong_reloads_it() {
        let mut st = St::new("pong-reload");
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_pong);
        let loaded = st.state();
        assert_eq!(st.toast(), Some(("Loaded Pong".into(), false)));
        let mut edit = (*st.w.pong.setup()).clone();
        edit.max = 2;
        edit.walls[Side::Left.index()] = Action::Off;
        st.w.pong.set_setup(edit);
        assert!(!alike(&st.state(), &loaded));
        pick(&mut k, &mut st, EN.preset_pong, EN.preset_pong);
        let got = st.state();
        assert_eq!(st.toast(), Some(("Reloaded Pong".into(), false)));
        assert_eq!(got.pong.walls[Side::Left.index()], loaded.pong.walls[Side::Left.index()]);
        assert_eq!(got.pong.max, loaded.pong.max);
        assert_eq!(got.midi.parts.len(), loaded.midi.parts.len(), "the walls' parts are replaced, not added");
        assert!(st.p.undo.is_some());
        assert_eq!(st.view.sections, [Section::Pong]);
        // Hands shown alone: nothing to reload
        let mut st = St::new("hands-reload");
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_hands);
        assert_eq!(st.toast(), Some(("Opened Hands".into(), false)));
        st.p.toast = None;
        pick(&mut k, &mut st, EN.preset_hands, EN.preset_hands);
        assert!(st.toast().is_none() && st.view.sections == [Section::Hands]);
    }

    #[test]
    fn close_needs_two_clicks_then_undo_restores_state_and_sections() {
        let mut st = St::new("close");
        let mut k = Kit::new();
        state_with(&st.w, &mixed());
        st.view = View { sections: vec![Section::Hands, Section::Pong], preset: None, ..Default::default() };
        let before = st.state();
        let hands = st.with(|p, _, cx| p.close_text(Section::Hands, cx));
        assert_eq!(hands, "Close Hands? 1 parts go", "the finger's own part; the shared one a wall plays stays");
        let pong = st.with(|p, _, cx| p.close_text(Section::Pong, cx));
        assert_eq!(pong, EN.close_arm_pong);
        let head = |ui: &mut Ui, st: &mut St| {
            let text = st.with(|p, _, cx| p.close_text(Section::Pong, cx));
            let (_, confirmed) = glass::section_head(ui, Id::new("pong-head"), "Pong", Some(("", &text)), true);
            if confirmed {
                st.with(|p, view, cx| p.close(view, Section::Pong, cx));
            }
            st.draw(ui);
        };
        k.frame_with(vec![], &mut |ui| head(ui, &mut st));
        let x = k.find("×").map_or(Pos2::ZERO, |r| r.center());
        k.click_with(x, &mut |ui| head(ui, &mut st));
        assert!(k.find(&pong).is_some(), "armed: {:?}", k.names());
        assert!(alike(&st.state(), &before) && st.view.sections.len() == 2, "one click closes nothing");
        k.click_with(x, &mut |ui| head(ui, &mut st));
        let got = st.state();
        assert_eq!(got.pong, Setup::default());
        assert_eq!(st.view.sections, [Section::Hands]);
        assert_eq!(View::read(&st.w.params).sections, [Section::Hands]);
        assert_eq!(st.toast(), Some(("Closed Pong".into(), false)));
        assert_eq!(got.midi.binds, before.midi.binds, "the hands keep playing");
        let undo = k.find(EN.undo);
        assert!(undo.is_some(), "{:?}", k.names());
        k.click_with(undo.map_or(Pos2::ZERO, |r| r.center()), &mut |ui| head(ui, &mut st));
        assert!(alike(&st.state(), &before), "Undo puts it back");
        assert_eq!(st.view.sections, [Section::Hands, Section::Pong]);
        assert_eq!(View::read(&st.w.params), st.view);
        assert_eq!(st.toast(), Some((EN.p_load_undone.into(), false)));
        assert!(st.p.undo.is_none());
        // Hands closes the same way
        st.with(|p, view, cx| p.close(view, Section::Hands, cx));
        assert!(!has_content(&st.state(), Section::Hands) && st.view.sections == [Section::Pong]);
        assert_eq!(st.toast(), Some(("Closed Hands".into(), false)));
    }

    #[test]
    fn a_profile_switch_drops_pending_lines_and_undo_and_comes_back_with_the_preset() {
        let mut st = St::new("profiles");
        state_with(&st.w, &mixed());
        st.view = View { sections: vec![Section::Hands, Section::Pong], preset: Some("Pong".into()), ..Default::default() };
        st.view.write(&st.w.params);
        let before = st.state();
        st.with(|p, view, cx| p.close(view, Section::Hands, cx));
        assert!(st.p.undo.is_some());
        let one = st.state();
        let one_view = st.view.clone();
        let discards = st.discards;
        st.with(|p, view, cx| p.switch_profile(1, view, cx));
        assert_eq!(st.discards, discards + 1, "pending lines dropped");
        assert!(st.p.undo.is_none(), "profile 1's Undo stays behind");
        assert_eq!(st.view, View::default());
        assert_eq!(st.toast(), Some(("Profile 2: empty, set it up fresh".into(), false)));
        assert!(st.state().midi.parts.is_empty() && !st.state().pong.on);
        // the active one again: nothing
        st.with(|p, view, cx| p.switch_profile(1, view, cx));
        assert_eq!(st.discards, discards + 1);
        st.with(|p, view, cx| p.switch_profile(0, view, cx));
        assert_eq!(st.toast(), Some(("Profile 1".into(), false)));
        assert!(alike(&st.state(), &one) && !alike(&st.state(), &before));
        assert_eq!(st.view, one_view);
        assert_eq!(View::read(&st.w.params), one_view);
        st.t = &DE;
        st.with(|p, view, cx| p.switch_profile(2, view, cx));
        assert_eq!(st.toast(), Some(("Profil 3: leer, neu einrichten".into(), false)));
    }

    #[test]
    fn undo_survives_two_seconds_of_pong_play() {
        let mut st = St::new("undo-play");
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_pong);
        assert!(st.p.undo.is_some());
        let t0 = Instant::now();
        st.tick(t0);
        let ns = crate::midi::now_ns();
        st.w.pong.tick(ns, &Input { live: true, ..Input::default() });
        st.w.pong.test_shots(4);
        for i in 0..1000_u64 {
            let now = ns + i * 2_000_000;
            st.w.pong.tick(now, &Input { live: true, ..Input::default() });
            st.w.player.tick(now);
            if i % 250 == 0 {
                st.tick(t0 + Duration::from_millis(i * 2 + 1000));
            }
        }
        let v = st.w.pong.view();
        assert!(v.hits > 0 || !v.shots.is_empty(), "Pong played: {:?} {:?}", v.hits, v.shots);
        for s in [2, 3, 12, 30] {
            st.tick(t0 + Duration::from_secs(s));
        }
        assert!(st.p.undo.as_ref().is_some_and(|u| u.changed_at.is_none()), "play is no change");
        k.frame(vec![], &mut st);
        assert!(k.find(EN.undo).is_some());
    }

    #[test]
    fn undo_goes_10_s_after_a_user_edit() {
        let mut st = St::new("undo-edit");
        let mut k = Kit::new();
        pick(&mut k, &mut st, EN.preset_picker, EN.preset_pong);
        let t0 = Instant::now();
        st.tick(t0);
        st.w.pong.set_setup(Setup { max: 2, ..(*st.w.pong.setup()).clone() });
        st.tick(t0 + Duration::from_millis(500));
        assert!(st.p.undo.as_ref().is_some_and(|u| u.changed_at.is_none()), "once a second");
        st.tick(t0 + Duration::from_secs(1));
        st.tick(t0 + Duration::from_secs(10));
        assert!(st.p.undo.is_some(), "9 s after the edit");
        st.tick(t0 + Duration::from_secs(11));
        assert!(st.p.undo.is_none(), "10 s after the edit");
        k.frame(vec![], &mut st);
        assert!(k.find(EN.undo).is_none() && k.find(EN.save_as).is_some());
        // the next action ends the old Undo at once
        pick(&mut k, &mut st, EN.preset_pong, EN.preset_pong);
        assert!(st.p.undo.is_some());
        st.p.saving = Some("Kept".into());
        k.frame(vec![], &mut st);
        assert!(k.click_text(EN.save, &mut st));
        assert!(st.p.undo.is_none(), "a save ends it");
    }

    #[test]
    fn a_saved_preset_reads_edited_after_a_change() {
        let mut st = St::new("edited");
        let mut k = Kit::new();
        assert!(save_in(&st.p.dir(), "Live set", &mixed()).is_ok());
        pick(&mut k, &mut st, EN.preset_picker, "Live set");
        assert_eq!(st.toast(), Some(("Loaded Live set".into(), false)));
        assert_eq!(st.view.sections, [Section::Hands, Section::Pong], "an old file opens what it plays");
        let t0 = Instant::now();
        st.tick(t0);
        k.frame(vec![], &mut st);
        assert!(k.find("Live set").is_some(), "{:?}", k.names());
        st.w.pong.set_setup(Setup { max: 2, ..(*st.w.pong.setup()).clone() });
        st.tick(t0 + Duration::from_secs(1));
        k.frame(vec![], &mut st);
        let edited = fill(EN.preset_edited, &[&"Live set"]);
        assert!(k.find(&edited).is_some(), "{:?}", k.names());
        assert!(k.alpha(&edited).is_some_and(|a| a < Tok::VALUE), "dimmer than a picked name");
        // re-picking loads the saved version again
        pick(&mut k, &mut st, &edited, "Live set");
        assert_eq!(st.toast(), Some(("Reloaded Live set".into(), false)));
        st.tick(t0 + Duration::from_secs(2));
        k.frame(vec![], &mut st);
        assert!(k.find("Live set").is_some() && k.find(&edited).is_none(), "{:?}", k.names());
        // a built-in never reads edited
        pick(&mut k, &mut st, "Live set", EN.preset_pong);
        if let Some(arm) = k.find(&fill(EN.replace_arm, &[&"Hands"])) {
            k.click(arm.center(), &mut st);
        }
        st.w.pong.set_setup(Setup { max: 3, ..(*st.w.pong.setup()).clone() });
        st.tick(t0 + Duration::from_secs(3));
        k.frame(vec![], &mut st);
        assert!(k.find(EN.preset_pong).is_some() && !st.p.edited, "{:?}", k.names());
        let _ = std::fs::remove_dir_all(st.p.dir());
    }
}
