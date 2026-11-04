//! Type-to-search over a long list (Noa, 11-04: scrolling through every
//! parameter of a plugin is unusable). A text field with the matches under
//! it, ranked while typing the way the chat field's completion ranks
//! parameter names (synthexis_complete::fuzzy: the same subsequence score,
//! the same floor, shorter names first on a tie, then list order): "freq"
//! puts "Filter 1 Freq" first. Arrow keys move the highlight, Enter picks
//! it, Esc or a click outside closes. The Routes view of the Hands
//! section retargets a kept route with it (a target box: the plugin's
//! parameters and Signals 1-32; a source box: the hand movements); a Pong
//! wall's Parameter mode picks its target with it.

use std::hash::Hash;

use nih_plug_egui::egui::{self, vec2, Id, Key, Modifiers, Rect, Response, Ui};
use synthexis_complete::fuzzy::{Key as Folded, Query};

use crate::fl;
use crate::glass::{self, white, Open, Tok};
use crate::i18n::{fill, Text};
use crate::route::NUM_MACROS;

/// What a search picks.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// Parameter `index` named `name` of the plugin `key` (fl::Key id).
    Param { key: String, index: u32, name: String },
    /// A Signal macro (0-based: Signal 1 is 0).
    Macro(usize),
    /// A hand movement: the feature after `cam0.hand.<L|R>.` ("pinch").
    Move(String),
}

/// Matches below this never show (the completion engine's floor).
const MIN_MATCH: f32 = 0.3;
/// Rows the list shows before it scrolls.
const VISIBLE: usize = 8;
/// Rows listed at most; typing finds the rest.
const MAX_ROWS: usize = 100;
/// The popup's least width.
const POP_W: f32 = 240.0;

/// The hand movements a route can start from, most used first. The
/// shape is left out: it starts MIDI parts, not parameters.
const MOVES: [&str; 20] = [
    "openness",
    "fist",
    "pinch",
    "x",
    "y",
    "spread",
    "roll",
    "depth",
    "speed",
    "finger_count",
    "finger.thumb",
    "finger.index",
    "finger.middle",
    "finger.ring",
    "finger.pinky",
    "finger.thumb.tilt",
    "finger.index.tilt",
    "finger.middle.tilt",
    "finger.ring.tilt",
    "finger.pinky.tilt",
];

/// The indices of `labels` that match `query`, best first. An empty query
/// keeps the list order.
pub fn rank(query: &str, labels: &[&str]) -> Vec<usize> {
    let q = Query::new(query);
    if q.chars.is_empty() {
        return (0..labels.len()).collect();
    }
    let mut hits: Vec<(usize, f32, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let k = Folded::new(l);
            q.score(&k).filter(|s| *s >= MIN_MATCH).map(|s| (i, s, l.len()))
        })
        .collect();
    hits.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
    hits.into_iter().map(|h| h.0).collect()
}

/// A parameter's name, "#12" when FL sent none.
pub fn param_label(name: &str, index: u32) -> String {
    if name.trim().is_empty() {
        format!("#{index}")
    } else {
        name.to_string()
    }
}

/// The parameters `names` of plugin `key`, then Signals 1-32.
pub fn targets(key: Option<&fl::Key>, names: &[(u32, String)], t: &Text) -> Vec<(Found, String)> {
    let params = key.into_iter().flat_map(|k| {
        let id = k.id();
        names.iter().map(move |(i, n)| (Found::Param { key: id.clone(), index: *i, name: n.clone() }, param_label(n, *i)))
    });
    params.chain((0..NUM_MACROS).map(|i| (Found::Macro(i), fill(t.p_signal, &[&(i + 1)])))).collect()
}

/// The hand movements, named as the Routes view's source boxes are.
pub fn moves(t: &Text) -> Vec<(Found, String)> {
    MOVES.iter().map(|f| (Found::Move((*f).to_string()), crate::fingers::graph::signal_label(&format!("cam0.hand.L.{f}"), t))).collect()
}

/// The plugin a target list shows: with `plugin` (a route's plugin name)
/// the pin, FL's focused plugin or else the first instance in the project
/// that has that name; without one the pin, else the focus.
pub fn plugin_key(plugin: Option<&str>, pin: Option<fl::Key>) -> Option<fl::Key> {
    let view = fl::bridge().view();
    let mut near = pin.into_iter().chain(view.focus.clone());
    match plugin {
        Some(p) => near.find(|k| k.plugin == p).or_else(|| view.targets.iter().find(|x| x.key.plugin == p).map(|x| x.key.clone())),
        None => near.next(),
    }
}

/// The parameter names of `key` as FL sent them, asked for once per key
/// (`wanted` remembers which), or the line to show instead: no plugin,
/// still loading, FL's error.
pub fn names(key: Option<&fl::Key>, wanted: &mut Option<String>, t: &Text) -> Result<Vec<(u32, String)>, String> {
    let Some(k) = key else { return Err(t.p_no_plugin.into()) };
    let bridge = fl::bridge();
    let got = match &bridge.view().params {
        Some((pk, ps)) if pk == k => Some(ps.clone()),
        _ => None,
    };
    got.unwrap_or_else(|| {
        if wanted.as_deref() != Some(k.id().as_str()) {
            bridge.want(k);
            *wanted = Some(k.id());
        }
        Err(fill(t.p_loading, &[&k.plugin]))
    })
}

/// The id of the search's popup.
fn popup_id(id: Id) -> Id {
    id.with("popup")
}

/// Opens search `id` with an empty field that has the keyboard.
pub fn open(ui: &Ui, id: Id) {
    ui.memory_mut(|m| m.open_popup(popup_id(id)));
    ui.data_mut(|d| {
        d.insert_temp(id.with("q"), String::new());
        d.insert_temp(id.with("focus"), true);
        d.remove::<usize>(id.with("hi"));
    });
}

pub fn is_open(ui: &Ui, id: Id) -> bool {
    ui.memory(|m| m.is_popup_open(popup_id(id)))
}

/// A picker button `w` wide that shows `shown` (HINT while nothing is
/// picked) and opens the search on a click; the pick.
#[allow(clippy::too_many_arguments)]
pub fn field<T: Clone + PartialEq>(ui: &mut Ui, id: impl Hash, w: f32, shown: &str, current: Option<&T>, items: &[(T, String)], note: Option<&str>, t: &Text) -> Option<T> {
    let id = ui.make_persistent_id(Id::new(id));
    let resp = glass::picker_button(ui, id, vec2(w, Tok::CTRL_H), shown, current.is_none(), "");
    if resp.clicked() {
        if is_open(ui, id) {
            ui.memory_mut(|m| m.close_popup());
        } else {
            open(ui, id);
        }
    }
    popup(ui, id, resp.rect, &resp, current, items, note, t)
}

/// Search `id` while it is open, below or above `anchor` (closes on a
/// click outside it and outside `owner`): the field, then the matches,
/// `current` highlighted while the field is empty. `note` stands in for
/// the plugin's parameters while there are none (no plugin, loading).
/// Closes on a pick and returns it.
#[allow(clippy::too_many_arguments)]
pub fn popup<T: Clone + PartialEq>(ui: &Ui, id: Id, anchor: Rect, owner: &Response, current: Option<&T>, items: &[(T, String)], note: Option<&str>, t: &Text) -> Option<T> {
    let pid = popup_id(id);
    if !ui.memory(|m| m.is_popup_open(pid)) {
        return None;
    }
    let (qid, hid, fid) = (id.with("q"), id.with("hi"), id.with("focus"));
    let mut query: String = ui.data(|d| d.get_temp(qid)).unwrap_or_default();
    let before = query.clone();
    // the keys move the highlight, not the field's cursor
    let (down, up) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp)));
    let rows = items.len().clamp(1, VISIBLE) as f32;
    let want_h = Tok::CTRL_H + Tok::GAP + rows * Tok::POP_ROW_H + 2.0 * glass::POP_PAD_PT;
    let w = anchor.width().max(POP_W);
    let picked = glass::popup_shown(ui, pid, anchor, owner, Open::Auto, w, want_h, f32::INFINITY, |ui, max_h| {
        let field_id = id.with("field");
        let r = glass::text_field(ui, field_id, &mut query, ui.available_width(), t.pf_hint);
        if ui.data_mut(|d| d.remove_temp::<bool>(fid)).unwrap_or(false) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(Tok::GAP);
        let labels: Vec<&str> = items.iter().map(|(_, s)| s.as_str()).collect();
        let mut order = rank(&query, &labels);
        let more = order.len() > MAX_ROWS;
        order.truncate(MAX_ROWS);
        // why the plugin's parameters are missing (no plugin, loading)
        if let Some(n) = note {
            glass::pick_header(ui, n);
        }
        if order.is_empty() {
            if !items.is_empty() || note.is_none() {
                glass::pick_header(ui, t.pf_none);
            }
            return None;
        }
        let at_current = || current.and_then(|c| order.iter().position(|i| items.get(*i).is_some_and(|(v, _)| v == c)));
        let fresh = if query.is_empty() { at_current().unwrap_or(0) } else { 0 };
        let mut hi = if query == before { ui.data(|d| d.get_temp::<usize>(hid)).unwrap_or(fresh) } else { 0 };
        let moved = down || up;
        if down {
            hi += 1;
        }
        if up {
            hi = hi.saturating_sub(1);
        }
        hi = hi.min(order.len() - 1);
        ui.data_mut(|d| d.insert_temp(hid, hi));
        let list_h = (max_h - Tok::CTRL_H - Tok::GAP).max(Tok::POP_ROW_H);
        let mut pick = egui::ScrollArea::vertical()
            .id_salt(id.with("list"))
            .max_height(list_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    let mut pick = None;
                    for (k, i) in order.iter().enumerate() {
                        let Some((v, s)) = items.get(*i) else { continue };
                        let row = glass::pop_row(ui, k == hi, s);
                        if moved && k == hi {
                            ui.scroll_to_rect(row.rect, None);
                        }
                        if row.clicked() {
                            pick = Some(v.clone());
                        }
                    }
                    if more {
                        glass::pick_header(ui, t.pf_more);
                    }
                    pick
                })
                .inner
            })
            .inner;
        if enter {
            pick = order.get(hi).and_then(|i| items.get(*i)).map(|(v, _)| v.clone());
        }
        pick
    })
    .flatten();
    ui.data_mut(|d| d.insert_temp(qid, query));
    if picked.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

/// The hint line under a search (FL's error, loading) in HINT.
pub fn note_line(ui: &mut Ui, note: &str) {
    ui.label(egui::RichText::new(note).color(white(Tok::HINT)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::EN;
    use nih_plug_egui::egui::{pos2, Context, Event, Pos2, RawInput};

    const SERUM: [&str; 9] = ["Env 1 Attack", "Fine", "Filter 1 Res", "Filter 1 Freq", "Filter 2 Freq", "LFO 1 Rate", "Osc A Level", "FX Reverb Size", "Filter 1 Drive"];

    #[test]
    fn typing_freq_puts_filter_1_freq_first() {
        let mut labels: Vec<String> = SERUM.iter().map(|s| (*s).to_string()).collect();
        labels.extend((1..=32).map(|i| format!("Signal {i}")));
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = |q: &str| rank(q, &refs).into_iter().map(|i| refs[i]).collect::<Vec<_>>();
        let freq = at("freq");
        assert_eq!(freq, vec!["Filter 1 Freq", "Filter 2 Freq"], "the first filter's frequency first, nothing that only shares letters");
        assert_eq!(at("filt 1 fr").first(), Some(&"Filter 1 Freq"), "{:?}", at("filt 1 fr"));
        assert_eq!(at("res").first(), Some(&"Filter 1 Res"), "{:?}", at("res"));
        // Signals 1-32 are in the same list
        assert_eq!(at("signal 12").first(), Some(&"Signal 12"), "{:?}", at("signal 12"));
        assert_eq!(at("12").first(), Some(&"Signal 12"));
        assert_eq!(rank("", &refs), (0..refs.len()).collect::<Vec<_>>(), "an empty field keeps the order");
        assert!(rank("zzz", &refs).is_empty());
    }

    #[test]
    fn the_lists_name_parameters_signals_and_movements() {
        let key = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let items = targets(Some(&key), &[(7, "Filter 1 Freq".into()), (9, " ".into())], &EN);
        assert_eq!(items.len(), 2 + NUM_MACROS);
        assert_eq!(items[0], (Found::Param { key: key.id(), index: 7, name: "Filter 1 Freq".into() }, "Filter 1 Freq".into()));
        assert_eq!(items[1].1, "#9");
        assert_eq!(items[2], (Found::Macro(0), "Signal 1".into()));
        assert_eq!(targets(None, &[(7, "Cutoff".into())], &EN).len(), NUM_MACROS, "no plugin: the Signals");
        let m = moves(&EN);
        let names: Vec<&str> = m.iter().map(|(_, s)| s.as_str()).collect();
        for want in ["Openness", "Fist", "Pinch", "Left-right", "Up-down", "Spread", "Turn", "Index tilt", "Index shown"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
        assert_eq!(m[0].0, Found::Move("openness".into()));
        assert!(m.iter().all(|(f, _)| *f != Found::Move("shape".into())));
    }

    /// The field opens on a click with the keyboard in it; typing ranks,
    /// the arrow moves the highlight, Enter picks and closes.
    #[test]
    fn type_arrow_enter_picks() {
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let items: Vec<(usize, String)> = SERUM.iter().enumerate().map(|(i, s)| (i, (*s).to_string())).collect();
        let mut time = 0.0;
        let mut run = |events: Vec<Event>| {
            time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(720.0, 520.0))), ..Default::default() };
            let mut got = None;
            let mut open_now = false;
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    got = field(ui, "find-test", 200.0, "Pick", None, &items, None, &EN);
                    open_now = is_open(ui, ui.make_persistent_id(Id::new("find-test")));
                });
            });
            (got, open_now)
        };
        assert_eq!(run(vec![]), (None, false));
        let at = pos2(20.0, 20.0);
        let click = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        run(vec![Event::PointerMoved(at), click(true)]);
        assert_eq!(run(vec![click(false)]), (None, true), "a click opens it");
        run(vec![]);
        run(vec![Event::Text("freq".into())]);
        let key = |key, pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        run(vec![key(Key::ArrowDown, true), key(Key::ArrowDown, false)]);
        let (got, open_now) = run(vec![key(Key::Enter, true), key(Key::Enter, false)]);
        // "freq": Filter 1 Freq, then Filter 2 Freq, which the arrow took
        assert_eq!(got, Some(4), "Filter 2 Freq");
        assert!(!open_now, "a pick closes it");
    }
}
