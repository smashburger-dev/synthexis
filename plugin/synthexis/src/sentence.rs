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
//! its chips (as pinned spans) go to the parser worker. Plain styling only;
//! the final look comes from Figma.
//!
//! Plugin picker: lists the plugins of the open FL project (fl.rs) as
//! "Portal, Master slot 1" and by default follows the plugin window last
//! clicked in FL. A mixer track with more than one plugin folds into its
//! own group, Sound (generators routed to it) above FX (its slots).
//! Picking an entry or "Pin" freezes the target until "Follow FL" or
//! "Unpin". The pin persists as a fl::Key id and is dropped when its slot
//! holds another plugin. Parameter names come from FL when a target is
//! picked; their classes from the exported plugin of the same name, else
//! the labels of a mapped plugin, else param-sense (sense.rs).
//!
//! Completion data: $SYNTHEXIS_COMPLETION_DIR, else
//! ~/Library/Application Support/synthexis/completion, as
//! tools/mapping/export_completion.py writes it. Loaded on its own thread;
//! queries run on the GUI thread (about 2 ms per keystroke for a plugin of
//! 2,600 parameters), never on the audio thread.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, TryRecvError};
use nih_plug_egui::egui::{
    self, emath::easing, text::CCursor, text::CCursorRange, text::LayoutJob, text::TextFormat,
    Align2, Color32, EventFilter, FontId, Id, Key, Modifiers, Order, PopupCloseBehavior, Rect, RichText, Sense,
    TextEdit, Ui,
};
use parking_lot::RwLock;
use synthexis_complete::data::{self, PluginInfo};
use synthexis_complete::doc::ChipText;
use synthexis_complete::{finger_open, Chip, Engine, Param, Suggestion, Variants, FINGERS, GHOST_MIN_CHARS, SIDES};
use synthexis_parser::{Pin, PinWhat};

use crate::assistant;
use crate::fl::{self, Link};
use crate::log::log;
use crate::sense;

const TOP: usize = 8;
const VISIBLE: usize = 5;
const OPEN_SECS: f32 = 0.15;
const MOVE_SECS: f32 = 0.08;
const ROW_H: f32 = 22.0;
/// Picker list: row height and the height the list grows to before it scrolls.
const PICK_ROW_H: f32 = 26.0;
const PICKER_H: f32 = 420.0;

type Loaded = (Result<(Engine, Vec<PluginInfo>), String>, sense::Classes, Vec<String>);

pub struct Sentence {
    doc: ChipText,
    engine: Option<Engine>,
    /// Plugins with exported completion data, for parameter classes.
    exported: Vec<PluginInfo>,
    classes: HashMap<String, HashMap<String, String>>,
    /// Labels of the mapped plugins and param-sense, and their result per target.
    labels: Arc<sense::Classes>,
    classified: HashMap<fl::Key, Vec<Param>>,
    /// param-sense runs off the GUI thread (ps-v2: about 80 ms for 2,600
    /// names); the target it works for and where its result arrives.
    classifying: Option<(fl::Key, Receiver<Vec<Param>>)>,
    loader: Option<Receiver<Loaded>>,
    status: String,
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
    pick_note: String,
    /// Where the picker button sat last frame (its list matches its width).
    picker_button: Rect,
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
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_COMPLETION_DIR") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/completion")
}

/// One picker row over the full list width: plugin name on the left, its
/// place (slot, channel or track) on the right. True when clicked.
fn pick_row(ui: &mut Ui, selected: bool, name: &str, place: &str, hover: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), PICK_ROW_H), Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 3.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, v.widgets.hovered.weak_bg_fill);
    }
    let (fg, weak) = if selected {
        (v.selection.stroke.color, v.selection.stroke.color)
    } else {
        (v.text_color(), v.weak_text_color())
    };
    let font = FontId::proportional(15.0);
    let place_w = ui.painter().layout_no_wrap(place.to_string(), font.clone(), weak).size().x;
    ui.painter().text(rect.right_center() - egui::vec2(8.0, 0.0), Align2::RIGHT_CENTER, place, font.clone(), weak);
    let name_rect = Rect::from_min_max(rect.min, egui::pos2(rect.right() - place_w - 20.0, rect.bottom()));
    ui.painter()
        .with_clip_rect(name_rect)
        .text(rect.left_center() + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, name, font, fg);
    let resp = if hover.is_empty() { resp } else { resp.on_hover_text(hover) };
    resp.clicked()
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

/// A side as the panel, the ghost text and a chip badge show it.
const SIDE_BADGES: [&str; 2] = ["L", "R"];

/// The open or made choice of a row, for the ghost text: "left/right?",
/// "left, finger?", "right, index".
fn variant_note(v: Variants, side: Option<usize>, finger: Option<usize>) -> String {
    let s = side.and_then(|i| SIDES.get(i)).map_or_else(|| "left/right?".to_string(), |s| s.to_string());
    let f = finger.and_then(|i| FINGERS.get(i)).map_or_else(|| "finger?".to_string(), |f| f.to_string());
    match (v.sided, v.finger) {
        (true, true) => format!("{s}, {f}"),
        (false, true) => f,
        _ => s,
    }
}

/// The badge after a chip: its finger when the phrase named none, its side.
fn badge(chip: &Chip) -> Option<String> {
    let Chip::Gesture { source, phrase, side, .. } = chip else { return None };
    let finger = finger_open(phrase_source(source), phrase).then(|| source.trim_start_matches("hand.finger.").to_string());
    let side = side.as_deref().and_then(|s| SIDES.iter().position(|x| *x == s)).map(|i| SIDE_BADGES[i].to_string());
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

/// Secondary text of a list row: what the item is.
fn hint(s: &Suggestion) -> String {
    match (&s.chip, &s.via) {
        (Chip::Param { class, .. }, Some(v)) => format!("{class} · \"{v}\""),
        (Chip::Param { class, .. }, None) => class.clone(),
        (Chip::Gesture { source, shape: Some(sh), .. }, _) => format!("{source}/{sh}"),
        (Chip::Gesture { source, pol, .. }, _) if *pol < 0 => format!("{source} (inverse)"),
        (Chip::Gesture { source, .. }, _) => source.clone(),
        (Chip::Target { kind, .. }, _) => match kind.as_str() {
            "midi.note" => "MIDI note".into(),
            "midi.chord" => "MIDI chord".into(),
            "midi.pattern" => "MIDI pattern".into(),
            "midi.cc" => "MIDI CC".into(),
            "macro" => "macro".into(),
            "xy" => "XY pad".into(),
            k => k.strip_prefix("midi.prop:").map_or_else(|| k.to_string(), |p| format!("MIDI {p}")),
        },
    }
}

impl Sentence {
    /// `bridge`: None runs without FL (tests).
    pub fn new(bridge: Option<&'static fl::Bridge>) -> Self {
        let (tx, rx) = bounded(1);
        let spawned = std::thread::Builder::new()
            .name("synthexis-completion".into())
            .spawn(move || {
                let dir = data_dir();
                let (labels, notes) = sense::Classes::load(&dir);
                let _ = tx.send((data::load(&dir), labels, notes));
            });
        let status = match spawned {
            Ok(_) => "loading completion".to_string(),
            Err(e) => format!("completion loader not started: {e}"),
        };
        Self {
            doc: ChipText::default(),
            engine: None,
            exported: Vec::new(),
            classes: HashMap::new(),
            labels: Arc::new(sense::Classes::default()),
            classified: HashMap::new(),
            classifying: None,
            loader: Some(rx),
            status,
            bridge,
            seen: u64::MAX,
            link: Link::Waiting,
            targets: Vec::new(),
            fl_loaded: false,
            focus: None,
            got: None,
            want: None,
            applied: None,
            pick_note: String::new(),
            picker_button: Rect::NOTHING,
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
        }
    }

    fn poll_loader(&mut self) {
        let Some(rx) = &self.loader else { return };
        let Ok((res, labels, notes)) = rx.try_recv() else { return };
        self.loader = None;
        self.labels = Arc::new(labels);
        for n in notes {
            log(&format!("completion: classes: {n}"));
        }
        match res {
            Ok((engine, plugins)) => {
                self.engine = Some(engine);
                self.exported = plugins;
                self.status.clear();
            }
            Err(e) => {
                log(&format!("completion: {e}"));
                self.status = format!("no completion data ({e})");
            }
        }
    }

    /// Parameter name -> class from the exported plugin named `plugin`.
    fn classes_of(&mut self, plugin: &str) -> &HashMap<String, String> {
        if !self.classes.contains_key(plugin) {
            let info = self.exported.iter().find(|p| p.name.eq_ignore_ascii_case(plugin));
            let map = info
                .and_then(|i| data::load_plugin(&data_dir(), i).ok())
                .map(|ps| ps.into_iter().map(|p| (p.name, p.class)).collect())
                .unwrap_or_default();
            self.classes.insert(plugin.to_string(), map);
        }
        &self.classes[plugin]
    }

    /// Takes the bridge's news, resolves the target and hands its
    /// parameters to the engine once FL sent them.
    fn sync_fl(&mut self, pin: &RwLock<String>) {
        let Some(b) = self.bridge else { return };
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
                self.pick_note = format!("pinned {} is gone, following FL", p.label());
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
                if let Some((k, rx)) = &self.classifying {
                    match rx.try_recv() {
                        Ok(params) => {
                            self.classified.insert(k.clone(), params);
                            self.classifying = None;
                        }
                        Err(TryRecvError::Disconnected) => self.classifying = None,
                        Err(TryRecvError::Empty) => {}
                    }
                }
                if !self.classified.contains_key(&want) {
                    if self.classifying.as_ref().is_some_and(|(k, _)| *k == want) {
                        self.pick_note = "reading parameter classes".into();
                        return;
                    }
                    let exported = self.classes_of(&want.plugin).clone();
                    if self.labels.needs_sense(&want.plugin, &names, &exported) {
                        let (tx, rx) = bounded(1);
                        let (labels, key) = (self.labels.clone(), want.clone());
                        let job = std::thread::Builder::new()
                            .name("synthexis-paramsense".into())
                            .spawn(move || {
                                let _ = tx.send(classify(&labels, &key, names, &exported));
                            });
                        match job {
                            Ok(_) => {
                                self.classifying = Some((want, rx));
                                self.pick_note = "reading parameter classes".into();
                            }
                            Err(e) => self.pick_note = format!("param-sense not started: {e}"),
                        }
                        return;
                    }
                    let params = classify(&self.labels, &want, names, &exported);
                    self.classified.insert(want.clone(), params);
                }
                let params = self.classified.get(&want).cloned().unwrap_or_default();
                self.pick_note = format!("{} parameters", params.len());
                if let Some(e) = self.engine.as_mut() {
                    e.set_plugin(&want.id(), params);
                }
                self.applied = Some(want);
            }
            Err(e) => self.pick_note = e,
        }
    }

    /// The project's plugins by mixer track; returns the clicked one.
    /// `current` is the pin, else the followed plugin.
    fn picker_list(&self, ui: &mut Ui, current: Option<&fl::Key>) -> Option<fl::Key> {
        let mut tracks: BTreeMap<u32, Vec<&fl::Target>> = BTreeMap::new();
        for t in &self.targets {
            tracks.entry(t.track).or_default().push(t);
        }
        let mut clicked = None;
        for (track, items) in &tracks {
            if let [only] = items.as_slice() {
                if pick_row(ui, current == Some(&only.key), &only.key.plugin, &only.track_label(), &only.key.label()) {
                    clicked = Some(only.key.clone());
                }
                continue;
            }
            let open = items.iter().any(|t| Some(&t.key) == current);
            let title = RichText::new(format!("{}   {}", items[0].track_label(), items.len())).size(15.0);
            egui::CollapsingHeader::new(title)
                .id_salt(("synthexis-picker-track", *track))
                .default_open(open)
                .show(ui, |ui| {
                    for (title, sound) in [("Sound", true), ("FX", false)] {
                        let mut part: Vec<&&fl::Target> = items.iter().filter(|t| t.is_sound() == sound).collect();
                        if part.is_empty() {
                            continue;
                        }
                        part.sort_by_key(|t| match t.key.at {
                            fl::At::Fx { slot, .. } => slot,
                            fl::At::Gen { ch } => ch,
                        });
                        ui.add_space(2.0);
                        ui.label(RichText::new(title).size(13.0).strong().color(ui.visuals().weak_text_color()));
                        for t in part {
                            let num = match t.key.at {
                                fl::At::Fx { slot, .. } => slot,
                                fl::At::Gen { ch } => ch,
                            };
                            if pick_row(ui, current == Some(&t.key), &t.key.plugin, &num.to_string(), &t.key.label()) {
                                clicked = Some(t.key.clone());
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
        }
        clicked
    }

    fn picker(&mut self, ui: &mut Ui, pin: &RwLock<String>) {
        let pinned = fl::Key::parse(&pin.read());
        let shown = match (&pinned, &self.want) {
            (Some(p), _) => p.label(),
            (None, Some(f)) => format!("{} (follows FL)", f.label()),
            (None, None) => "follows FL".to_string(),
        };
        let mut choice = None;
        ui.horizontal(|ui| {
            ui.label("Plugin");
            let width = (ui.available_width() - 70.0).clamp(240.0, 420.0);
            // A ComboBox's list keeps the height of the previous frame, so an
            // opened track group would be cut off; this list grows with its
            // rows up to PICKER_H and scrolls beyond that.
            let button = ui.add(
                egui::Button::new(RichText::new(shown))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .min_size(egui::vec2(width, 0.0)),
            );
            let popup = Id::new("synthexis-picker-popup");
            let arrow = button.rect.right_center() - egui::vec2(12.0, 0.0);
            let col = ui.visuals().text_color();
            ui.painter().add(egui::Shape::convex_polygon(
                vec![arrow + egui::vec2(-4.0, -2.0), arrow + egui::vec2(4.0, -2.0), arrow + egui::vec2(0.0, 3.0)],
                col,
                egui::Stroke::NONE,
            ));
            if button.clicked() {
                ui.memory_mut(|m| m.toggle_popup(popup));
            }
            self.picker_button = button.rect;
            // opening or closing a track group must not close the list
            egui::popup_below_widget(ui, popup, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
                // as wide as the button: the popup sets that width, the list fills it
                egui::ScrollArea::vertical()
                    .max_height(PICKER_H)
                    .min_scrolled_height(PICKER_H)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let current = pinned.as_ref().or(self.want.as_ref());
                        if pick_row(ui, pinned.is_none(), "Follow FL", "", "the plugin window last clicked in FL") {
                            choice = Some(None);
                        }
                        ui.separator();
                        if let Some(k) = self.picker_list(ui, current) {
                            choice = Some(Some(k));
                        }
                    });
            });
            if choice.is_some() {
                ui.memory_mut(|m| m.close_popup());
            }
            match (&pinned, &self.want) {
                (Some(_), _) => {
                    if ui.button("Unpin").clicked() {
                        choice = Some(None);
                    }
                }
                (None, Some(f)) => {
                    if ui.button("Pin").clicked() {
                        choice = Some(Some(f.clone()));
                    }
                }
                (None, None) => {}
            }
        });
        if let Some(c) = choice {
            *pin.write() = c.map(|k| k.id()).unwrap_or_default();
            self.pick_note.clear();
        }
        let note = match &self.link {
            _ if self.bridge.is_none() => String::new(),
            Link::Waiting => "waiting for FL".to_string(),
            Link::Down(e) => e.clone(),
            Link::Up if self.fl_loaded && self.targets.is_empty() => "no plugins in the project".to_string(),
            Link::Up if self.want.is_none() => "click a plugin window in FL".to_string(),
            Link::Up => self.pick_note.clone(),
        };
        let notes: Vec<&str> = [note.as_str(), self.status.as_str()].into_iter().filter(|n| !n.is_empty()).collect();
        if !notes.is_empty() {
            ui.weak(notes.join(" · "));
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
        self.cursor = self.doc.accept(&s);
        if let Some(e) = self.engine.as_mut() {
            e.picked(&s.chip);
        }
        set_cursor(ctx, id, &self.doc.text, self.cursor);
        self.list_open = false;
        self.sugg.clear();
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
        self.status = format!("sent, {} pinned", pins.len());
        assistant::submit(&text, pins);
        self.doc.clear();
        self.cursor = 0;
        self.sugg.clear();
        self.list_open = false;
    }

    /// `pin`: the pinned target's fl::Key id, empty while following FL.
    pub fn ui(&mut self, ui: &mut Ui, enabled: bool, pin: &RwLock<String>) {
        self.poll_loader();
        self.sync_fl(pin);
        let id = ui.make_persistent_id("synthexis-sentence");
        self.id = Some(id);
        self.picker(ui, pin);

        let has_focus = ui.memory(|m| m.has_focus(id));
        let active = enabled && has_focus && !self.sugg.is_empty();
        let at_end = self.cursor == self.doc.text.len();
        let ghost = active
            && !self.dismissed
            && self.sel == 0
            && at_end
            && self.sugg[0].ghost_worthy(&self.doc.text);
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
        let font = FontId::proportional(14.0);
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
            .font(FontId::proportional(14.0))
            .desired_width(f32::INFINITY)
            .hint_text("when I spread my fingers, open the filter")
            .lock_focus(enabled)
            .layouter(&mut layouter)
            .show(ui);
        // a small badge over the end of a chip with a picked finger or side
        if text == self.doc.text {
            for c in &self.doc.chips {
                let Some(b) = badge(&c.chip) else { continue };
                let ci = self.doc.text[..c.range.end.min(self.doc.text.len())].chars().count();
                let at = out.galley.pos_from_ccursor(CCursor::new(ci)).translate(out.galley_pos.to_vec2());
                let p = ui.painter().with_clip_rect(out.text_clip_rect.expand(8.0));
                let g = p.layout_no_wrap(b, FontId::proportional(9.0), ui.visuals().selection.stroke.color);
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
                let note = variant_note(s.variants, side, finger);
                ghost = if self.needs_choice(0) { format!("{ghost}  ({note})") } else { format!("{ghost}  · {note}") };
            }
            let pos = out.galley_pos + egui::vec2(out.galley.rect.right(), 0.0);
            ui.painter()
                .with_clip_rect(out.text_clip_rect)
                .text(pos, Align2::LEFT_TOP, ghost, FontId::proportional(14.0), ui.visuals().weak_text_color());
        }

        // the list opens in 150 ms with an ease-out; selection and scrolling
        // glide in 80 ms
        let shown = enabled && self.list_open && !self.sugg.is_empty();
        // egui drops the focus on Esc before widgets see it; while there is
        // something to dismiss, the field keeps Esc (and Tab) for itself
        if out.response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: enabled,
                        escape: show_ghost || shown,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    },
                )
            });
        }
        let t = ui
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
        let width = out.response.rect.width().min(420.0);
        let rows = self.sugg.len().min(VISIBLE);
        let full = ROW_H * rows as f32 + 8.0;
        let pos = out.response.rect.left_bottom() + egui::vec2(0.0, 2.0);
        let mut clicked = None;
        let mut sel_rect = None;
        egui::Area::new(id.with("list-area"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .show(&ctx, |ui| {
                ui.set_clip_rect(Rect::from_min_size(pos, egui::vec2(width + 16.0, full * t + 8.0)));
                ui.multiply_opacity(t);
                egui::Frame::popup(ui.style()).show(ui, |ui| {
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
                            FontId::proportional(14.0),
                            ui.visuals().strong_text_color(),
                        );
                        p.text(
                            rect.right_center() - egui::vec2(10.0, 0.0),
                            Align2::RIGHT_CENTER,
                            hint(s),
                            FontId::proportional(11.0),
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
            self.variant_panel(&ctx, ui, id, list, r, t);
        }
        if let Some(i) = clicked {
            self.take(ui.ctx(), id, i);
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// The selected row's choice, right of the list (always: the plugin
    /// window has one fixed width that fits list and panel): the hand row,
    /// and for a bare "finger" the finger row once a side is picked. The
    /// current value is highlighted; a click picks it, and a click that
    /// completes the choice makes the chip.
    fn variant_panel(&mut self, ctx: &egui::Context, ui: &Ui, id: Id, list: Rect, row: Rect, t: f32) {
        if !self.panel_open() {
            return;
        }
        let v = self.sugg[self.sel].variants;
        let fingers = v.finger && (self.side.is_some() || !v.sided);
        let at = egui::pos2(list.right() + 4.0, row.top() - 6.0);
        let hint = match (v.sided, fingers) {
            (true, true) => "← → hand   ↑ ↓ finger   Enter",
            (true, false) => "← left   → right   Enter",
            _ => "↑ ↓ finger   Enter",
        };
        let mut picked = None;
        egui::Area::new(id.with("variant-area"))
            .order(Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                ui.multiply_opacity(t);
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let mut axis = |ui: &mut Ui, name: &str, vals: &[&str], cur: Option<usize>, k: usize| {
                        ui.horizontal(|ui| {
                            ui.add_sized([44.0, 22.0], egui::Label::new(RichText::new(name).weak().size(12.0)));
                            for (j, val) in vals.iter().enumerate() {
                                if ui.selectable_label(cur == Some(j), RichText::new(*val).size(13.0)).clicked() {
                                    picked = Some((k, j));
                                }
                            }
                        });
                    };
                    if v.sided {
                        axis(ui, "Hand", &SIDES, self.side, 0);
                    }
                    if fingers {
                        axis(ui, "Finger", &FINGERS, self.finger, 1);
                    }
                    ui.label(RichText::new(hint).weak().size(11.0));
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
        let mut e = Engine::new(
            vec![g("hand.spread", "spread my fingers"), g("hand.fist", "Faust schließen"), g("hand.finger.index", "Finger")],
            vec![("filter.cutoff".into(), "Cutoff".into())],
            &HashMap::new(),
        )
        .with_sided(vec!["hand.fist".into(), "hand.finger.index".into()]);
        let p = |index: u32, name: &str, class: &str| Param { index, name: name.into(), class: class.into() };
        e.set_plugin("Synth/Test", vec![p(0, "Filter 1 Freq", "filter.cutoff"), p(1, "Filter 2 Freq", "filter.cutoff")]);
        e
    }

    fn key(k: Key) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
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
        out: Option<egui::FullOutput>,
    }

    impl Harness {
        fn new() -> Self {
            let mut s = Sentence::new(None);
            s.loader = None;
            s.engine = Some(engine());
            let mut h = Self { ctx: egui::Context::default(), s, picked: RwLock::new(String::new()), out: None };
            h.frame(vec![]);
            let id = h.s.id;
            h.ctx.memory_mut(|m| id.into_iter().for_each(|id| m.request_focus(id)));
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let raw = RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                ..Default::default()
            };
            let (s, picked) = (&mut self.s, &self.picked);
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| s.ui(ui, true, picked));
            }));
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

    /// An unmapped plugin: param-sense runs on its own thread, the field
    /// shows a note meanwhile, and the classes then reach the engine.
    #[test]
    fn unmapped_plugin_is_classified_off_the_gui_thread() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let b = fl::Bridge::detached();
        let mut h = Harness::new();
        h.s.bridge = Some(b);
        h.s.labels = Arc::new(sense::Classes::load(&root.join("../assistant/completion")).0);
        let key = fl::Key { at: fl::At::Fx { track: 2, slot: 0 }, plugin: "Totally Unknown Synth".into() };
        let names: Vec<(u32, String)> =
            ["VCF Cutoff", "VCF Resonance", "Amp Attack", "Delay Mix"].iter().enumerate().map(|(i, n)| (i as u32, n.to_string())).collect();
        b.update(|v| {
            v.link = fl::Link::Up;
            v.loaded = true;
            v.targets = vec![fl::Target { key: key.clone(), track: 2, track_name: String::new() }];
            v.focus = Some(key.clone());
            v.params = Some((key.clone(), Ok(names)));
        });
        h.frame(vec![]);
        assert_eq!(h.s.pick_note, "reading parameter classes");
        for _ in 0..200 {
            if h.s.applied.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.frame(vec![]);
        }
        assert_eq!(h.s.applied, Some(key.clone()));
        let classes: Vec<String> = h.s.classified[&key].iter().map(|p| p.class.clone()).collect();
        assert_eq!(classes[0], "filter.cutoff");
        assert!(classes.iter().all(|c| !c.is_empty()), "{classes:?}");
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

        // the pinned plugin leaves its slot: pin dropped, follow FL again
        b.update(|v| v.targets = target(&proq).into_iter().collect());
        h.frame(vec![]);
        assert!(h.picked.read().is_empty());
        assert_eq!(h.s.want, Some(proq.clone()));
        assert_eq!(h.s.applied, None, "Pro-Q 3 names not sent yet");
    }

    #[test]
    fn picker_groups_stay_open_and_rows_pin() {
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
        assert!(h.click("follows FL"), "combo button not drawn");
        assert!(h.popup_open());
        assert!(h.find("Pro-Q 3").is_some(), "single-plugin track shows as a row");
        assert!(h.find("Portal").is_none(), "Master starts folded");
        // a user opens Master, closes it, opens it again: the list stays up
        for _ in 0..3 {
            assert!(h.click("Master   3"));
            assert!(h.popup_open(), "toggling a group closed the list");
        }
        for row in ["Sound", "Serum 2", "FX", "ValhallaSupermassive", "Portal", "Pro-Q 3", "Track 3"] {
            assert!(h.find(row).is_some(), "{row} not visible in the opened list");
        }
        let list = h.ctx.memory(|m| m.area_rect(Id::new("synthexis-picker-popup")));
        let button = h.s.picker_button;
        assert!(list.is_some_and(|r| (r.width() - button.width()).abs() < 1.0), "list {list:?}, button {button:?}");
        assert!(h.click("Portal"));
        assert_eq!(*h.picked.read(), "fx:0:1:Portal");
        assert!(!h.popup_open(), "picking a row closes the list");
    }

    #[test]
    fn left_right_pick_the_hand_not_the_cursor() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("wenn ich die Fau".into())]);
        h.frame(vec![key(Key::ArrowDown)]);
        assert!(h.s.panel_open());
        assert_eq!(h.s.side, None, "no default side");
        h.frame(vec![]);
        let drawn = (h.find("Faust"), h.find("right"));
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
        assert_eq!(badge(&c.chip).as_deref(), Some("L"));
    }

    #[test]
    fn tab_without_a_side_opens_the_panel_and_inserts_nothing() {
        let mut h = Harness::new();
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
        let mut h = Harness::new();
        h.frame(vec![Event::Text("wenn ich die rechte Fau".into())]);
        assert_eq!(h.s.side, Some(1));
        h.frame(vec![key(Key::Tab)]);
        assert_eq!(gesture_of(&h), ("hand.fist".into(), Some("right".into())));
        // "beide" is no side the panel offers: the choice stays open
        let mut h = Harness::new();
        h.frame(vec![Event::Text("wenn ich beide Fau".into())]);
        assert!(h.s.needs_choice(0));
    }

    #[test]
    fn a_bare_finger_takes_side_then_finger_and_the_side_never_locks() {
        let mut h = Harness::new();
        // two items: the list is open already, "Finger" selected
        h.frame(vec![Event::Text("wenn ich die Fin".into())]);
        assert!(h.s.panel_open());
        h.frame(vec![]);
        assert!(h.find("thumb").is_none(), "fingers show only after the side");
        h.frame(vec![key(Key::ArrowLeft)]);
        h.frame(vec![]);
        assert!(h.find("thumb").is_some(), "fingers show once a side is picked");
        h.frame(vec![key(Key::ArrowDown)]);
        h.frame(vec![key(Key::ArrowDown)]);
        assert_eq!((h.s.sel, h.s.finger), (0, Some(1)), "Up/Down move the fingers, not the list");
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.finger.index".into(), Some("left".into())));
        let mut h = Harness::new();
        h.frame(vec![Event::Text("wenn ich die Fin".into())]);
        for k in [Key::ArrowLeft, Key::ArrowDown, Key::ArrowDown, Key::ArrowDown] {
            h.frame(vec![key(k)]);
        }
        h.frame(vec![key(Key::ArrowRight)]);
        assert_eq!((h.s.side, h.s.finger), (Some(1), Some(2)), "Right switches the hand in the finger step");
        h.frame(vec![key(Key::Enter)]);
        assert_eq!(gesture_of(&h), ("hand.finger.middle".into(), Some("right".into())));
        assert_eq!(badge(&h.s.doc.chips[0].chip).as_deref(), Some("middle R"));
    }

    #[test]
    fn esc_closes_the_panel_then_the_list() {
        let mut h = Harness::new();
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

    #[test]
    fn pins_follow_chips() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("Fau".into())]);
        h.frame(vec![key(Key::Tab)]);
        h.frame(vec![key(Key::ArrowLeft)]);
        h.frame(vec![key(Key::Tab)]);
        let c = &h.s.doc.chips[0];
        let pin = to_pin(&c.range, &c.chip);
        assert_eq!(pin.bytes, (0, "Faust schließen".len()));
        assert_eq!(pin.what, PinWhat::Gesture { source: "hand.fist".into(), shape: None, side: Some("left".into()) });
    }
}
