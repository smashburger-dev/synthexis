//! Sentence field of the mapping assistant with IDE-style completion
//! (synthexis_complete): the best item shows as ghost text after the
//! cursor and Tab accepts it. From two typed characters a list of up to 8
//! items opens below the field (5 rows visible, it scrolls); the arrow keys
//! move through it, Tab or Enter picks, Esc dismisses. Enter picks only
//! after an arrow key, so Enter on a finished sentence still sends it. An accepted item becomes a chip; on Enter the sentence and
//! its chips (as pinned spans) go to the parser worker. Plain styling only;
//! the final look comes from Figma.
//!
//! Completion data: $SYNTHEXIS_COMPLETION_DIR, else
//! ~/Library/Application Support/synthexis/completion, as
//! tools/mapping/export_completion.py writes it. Loaded on its own thread;
//! queries run on the GUI thread (about 2 ms per keystroke for a plugin of
//! 2,600 parameters), never on the audio thread.

use std::path::PathBuf;

use crossbeam_channel::{bounded, Receiver};
use nih_plug_egui::egui::{
    self, emath::easing, text::CCursor, text::CCursorRange, text::LayoutJob, text::TextFormat,
    Align2, Color32, EventFilter, FontId, Id, Key, Modifiers, Order, Rect, Sense, TextEdit, Ui,
};
use parking_lot::RwLock;
use synthexis_complete::data::{self, PluginInfo};
use synthexis_complete::doc::ChipText;
use synthexis_complete::{Chip, Engine, Suggestion, GHOST_MIN_CHARS};
use synthexis_parser::{Pin, PinWhat};

use crate::assistant;
use crate::log::log;

const TOP: usize = 8;
const VISIBLE: usize = 5;
const OPEN_SECS: f32 = 0.15;
const MOVE_SECS: f32 = 0.08;
const ROW_H: f32 = 22.0;

type Loaded = Result<(Engine, Vec<PluginInfo>), String>;

pub struct Sentence {
    doc: ChipText,
    engine: Option<Engine>,
    plugins: Vec<PluginInfo>,
    loader: Option<Receiver<Loaded>>,
    status: String,
    sugg: Vec<Suggestion>,
    list_open: bool,
    /// An arrow key moved through the list; Enter now picks instead of sending.
    nav: bool,
    sel: usize,
    /// First visible row of the list.
    scroll: usize,
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
        Chip::Gesture { source, shape, .. } => PinWhat::Gesture {
            source: source.clone(),
            shape: shape.clone(),
        },
        Chip::Target { kind, .. } => PinWhat::Target { kind: kind.clone() },
    };
    Pin { bytes: (range.start, range.end), what }
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

impl Default for Sentence {
    fn default() -> Self {
        Self::new()
    }
}

impl Sentence {
    pub fn new() -> Self {
        let (tx, rx) = bounded(1);
        let spawned = std::thread::Builder::new()
            .name("synthexis-completion".into())
            .spawn(move || {
                let _ = tx.send(data::load(&data_dir()));
            });
        let status = match spawned {
            Ok(_) => "loading completion".to_string(),
            Err(e) => format!("completion loader not started: {e}"),
        };
        Self {
            doc: ChipText::default(),
            engine: None,
            plugins: Vec::new(),
            loader: Some(rx),
            status,
            sugg: Vec::new(),
            list_open: false,
            nav: false,
            sel: 0,
            scroll: 0,
            dismissed: false,
            cursor: 0,
            id: None,
        }
    }

    fn poll_loader(&mut self, picked: &RwLock<String>) {
        let Some(rx) = &self.loader else { return };
        let Ok(res) = rx.try_recv() else { return };
        self.loader = None;
        match res {
            Ok((engine, plugins)) => {
                self.engine = Some(engine);
                self.plugins = plugins;
                self.status.clear();
                let want = picked.read().clone();
                if let Some(info) = self.plugins.iter().find(|p| p.plugin_id == want).cloned() {
                    self.pick(&info, picked);
                }
            }
            Err(e) => {
                log(&format!("completion: {e}"));
                self.status = format!("no completion data ({e})");
            }
        }
    }

    fn pick(&mut self, info: &PluginInfo, picked: &RwLock<String>) {
        let Some(engine) = self.engine.as_mut() else { return };
        match data::load_plugin(&data_dir(), info) {
            Ok(params) => {
                engine.set_plugin(&info.plugin_id, params);
                *picked.write() = info.plugin_id.clone();
            }
            Err(e) => self.status = e,
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
        let typed = self.sugg.first().map_or(0, |s| self.doc.text[s.replace.clone()].chars().count());
        self.list_open = !self.dismissed && self.sugg.len() >= 2 && typed >= GHOST_MIN_CHARS;
    }

    fn select(&mut self, i: usize) {
        self.sel = i;
        self.nav = true;
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + VISIBLE {
            self.scroll = i + 1 - VISIBLE;
        }
    }

    fn accept(&mut self, ctx: &egui::Context, id: Id, i: usize) {
        let Some(s) = self.sugg.get(i).cloned() else { return };
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

    pub fn ui(&mut self, ui: &mut Ui, enabled: bool, picked: &RwLock<String>) {
        self.poll_loader(picked);
        let id = ui.make_persistent_id("synthexis-sentence");
        self.id = Some(id);
        ui.horizontal(|ui| {
            ui.label("Plugin");
            let current = picked.read().clone();
            let shown = self
                .plugins
                .iter()
                .find(|p| p.plugin_id == current)
                .map_or("none", |p| p.name.as_str())
                .to_string();
            let mut choice = None;
            egui::ComboBox::from_id_salt("synthexis-completion-plugin")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for p in &self.plugins {
                        let label = format!("{} ({})", p.name, p.n);
                        if ui.selectable_label(p.plugin_id == current, label).clicked() {
                            choice = Some(p.clone());
                        }
                    }
                });
            if let Some(p) = choice {
                self.pick(&p, picked);
            }
            if !self.status.is_empty() {
                ui.weak(&self.status);
            }
        });

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
            ui.input_mut(|i| {
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
                if (ghost || self.list_open) && i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.list_open = false;
                    self.dismissed = true;
                }
            });
        }
        if let Some(i) = take {
            self.accept(ui.ctx(), id, i);
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
            let ghost = s
                .ghost_suffix(&self.doc.text)
                .unwrap_or_else(|| format!("  → {}", s.chip.label()));
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
        if let Some(i) = clicked {
            self.accept(ui.ctx(), id, i);
            ui.memory_mut(|m| m.request_focus(id));
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
            vec![g("hand.spread", "spread my fingers"), g("hand.fist", "Faust schließen")],
            vec![("filter.cutoff".into(), "Cutoff".into())],
            &HashMap::new(),
        );
        let p = |index: u32, name: &str, class: &str| Param { index, name: name.into(), class: class.into() };
        e.set_plugin("Synth/Test", vec![p(0, "Filter 1 Freq", "filter.cutoff"), p(1, "Filter 2 Freq", "filter.cutoff")]);
        e
    }

    fn key(k: Key) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    struct Harness {
        ctx: egui::Context,
        s: Sentence,
        picked: RwLock<String>,
    }

    impl Harness {
        fn new() -> Self {
            let mut s = Sentence::new();
            s.loader = None;
            s.engine = Some(engine());
            let mut h = Self { ctx: egui::Context::default(), s, picked: RwLock::new(String::new()) };
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
            let _ = self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| s.ui(ui, true, picked));
            });
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
        h.s.engine.as_mut().unwrap().set_plugin("Synth/Test", (0..10).map(p).collect());
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

    #[test]
    fn pins_follow_chips() {
        let mut h = Harness::new();
        h.frame(vec![Event::Text("Fau".into())]);
        h.frame(vec![key(Key::Tab)]);
        let c = &h.s.doc.chips[0];
        let pin = to_pin(&c.range, &c.chip);
        assert_eq!(pin.bytes, (0, "Faust schließen".len()));
        assert_eq!(pin.what, PinWhat::Gesture { source: "hand.fist".into(), shape: None });
    }
}
