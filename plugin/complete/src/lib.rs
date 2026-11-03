//! IDE-style completion for the mapping assistant's sentence field.
//!
//! Two kinds of items: parameters of the picked plugin (matched by their
//! name and, through the glossary, by phrases of their param-sense class:
//! "cut" or "Filter auf" offer "Filter 1 Freq") and gestures (glossary
//! phrases of the ontology sources and hand shapes, only where a gesture is
//! named: sentence start, "when I", "wenn ich", "with my"). An accepted
//! item becomes a chip that carries an id; on submit the chips become
//! pinned spans for the parser (synthexis_parser::pin).
//!
//! Rank = fuzzy match (fuzzy.rs) + class prior + the user's recent picks +
//! the words before the cursor (switching words rank switch parameters up).
//! A keystroke costs a prefix lookup and a fuzzy pass over the keys that
//! share the query's first letter at a word start, well under a
//! millisecond for a plugin of 2,600 parameters. Runs on the GUI thread;
//! never on the audio thread, never the neural parser.

pub mod context;
pub mod data;
pub mod doc;
pub mod fuzzy;
mod index;

use std::collections::HashMap;
use std::ops::Range;

use fuzzy::Query;
use index::Index;

/// Ghost text needs this many typed characters ...
pub const GHOST_MIN_CHARS: usize = 2;
/// ... and the best item at least this score.
pub const GHOST_MIN_SCORE: f32 = 0.8;
/// Matches below this never show.
const MIN_MATCH: f32 = 0.3;
/// A class phrase match counts this much of a name match.
const VIA_PHRASE: f32 = 0.85;
const GESTURE_BONUS: f32 = 0.05;
const WORD_BONUS: f32 = 0.03;
const PRIOR_MAX: f32 = 0.05;
const RECENT_MAX: f32 = 0.1;
const RECENT_CAP: usize = 32;
const SWITCH_UP: f32 = 0.15;
const SWITCH_DOWN: f32 = -0.05;
const NOT_MAPPABLE: f32 = -0.2;
pub const SWITCH_CLASS: &str = "ctl.switch";

#[derive(Clone, Debug, PartialEq)]
pub struct Gesture {
    pub source: String,
    /// Hand shape id when `source` is hand.shape.
    pub shape: Option<String>,
    pub phrase: String,
    /// -1 when the phrase names the source's low end ("Finger zusammen").
    pub pol: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub index: u32,
    pub name: String,
    pub class: String,
}

/// What an accepted completion pins.
#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    Param { plugin: String, index: u32, name: String, class: String },
    Gesture { source: String, shape: Option<String>, phrase: String, pol: i8 },
}

impl Chip {
    /// Stable id: "param:<plugin id>#<index>" or "gesture:<source>[/<shape>]".
    pub fn id(&self) -> String {
        match self {
            Chip::Param { plugin, index, .. } => format!("param:{plugin}#{index}"),
            Chip::Gesture { source, shape: Some(s), .. } => format!("gesture:{source}/{s}"),
            Chip::Gesture { source, .. } => format!("gesture:{source}"),
        }
    }

    /// The text the chip shows and puts into the sentence.
    pub fn label(&self) -> &str {
        match self {
            Chip::Param { name, .. } => name,
            Chip::Gesture { phrase, .. } => phrase,
        }
    }

    /// A switch parameter becomes a toggle target, any other a param target.
    pub fn is_switch(&self) -> bool {
        matches!(self, Chip::Param { class, .. } if class == SWITCH_CLASS)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub chip: Chip,
    pub score: f32,
    /// Bytes of the typed text the chip replaces.
    pub replace: Range<usize>,
    /// The class phrase that matched, when the name did not ("Cutoff").
    pub via: Option<String>,
    /// The typed text is a prefix of the label or of `via`.
    pub prefix: bool,
    /// Offered from a weak gesture position: list only, no ghost text.
    pub weak: bool,
}

impl Suggestion {
    /// The rest of the label when the typed text is its prefix (shown as
    /// ghost text after the cursor); None for a fuzzy or indirect match.
    pub fn ghost_suffix(&self, text: &str) -> Option<String> {
        let typed = text.get(self.replace.clone())?;
        let label = self.chip.label();
        let n = typed.chars().count();
        let head: String = label.chars().take(n).collect();
        (head.to_lowercase() == typed.to_lowercase()).then(|| label.chars().skip(n).collect())
    }

    /// Ghost text only continues what was typed: a prefix match of two or
    /// more characters with a good score. Fuzzy matches stay in the list.
    pub fn ghost_worthy(&self, text: &str) -> bool {
        self.prefix
            && !self.weak
            && self.score >= GHOST_MIN_SCORE
            && text
                .get(self.replace.clone())
                .is_some_and(|t| t.chars().count() >= GHOST_MIN_CHARS)
    }
}

struct Plugin {
    id: String,
    params: Vec<Param>,
    class: Vec<Option<usize>>,
    by_class: Vec<Vec<u32>>,
    index: Index,
}

pub struct Engine {
    gestures: Vec<Gesture>,
    gindex: Index,
    phrase_class: Vec<usize>,
    phrases: Vec<String>,
    pindex: Index,
    classes: Vec<String>,
    class_id: HashMap<String, usize>,
    prior: Vec<f32>,
    plugin: Option<Plugin>,
    recent: Vec<String>,
}

#[derive(Default)]
struct Scratch {
    seen: Vec<bool>,
    ids: Vec<u32>,
}

impl Engine {
    /// `phrases`: (class, phrase) from the glossary; `prior`: label count
    /// per class over the harvested plugins.
    pub fn new(
        gestures: Vec<Gesture>,
        phrases: Vec<(String, String)>,
        prior: &HashMap<String, u64>,
    ) -> Self {
        let mut classes: Vec<String> = prior.keys().cloned().collect();
        classes.extend(phrases.iter().map(|(c, _)| c.clone()));
        classes.sort();
        classes.dedup();
        let class_id: HashMap<String, usize> =
            classes.iter().enumerate().map(|(i, c)| (c.clone(), i)).collect();
        let top = prior.values().copied().max().unwrap_or(1).max(1) as f32;
        let prior = classes
            .iter()
            .map(|c| {
                let n = prior.get(c).copied().unwrap_or(0) as f32;
                PRIOR_MAX * (1.0 + n).ln() / (1.0 + top).ln()
            })
            .collect();
        let phrase_class = phrases.iter().map(|(c, _)| class_id[c]).collect();
        let phrases: Vec<String> = phrases.into_iter().map(|(_, p)| p).collect();
        Self {
            gindex: Index::new(gestures.iter().map(|g| g.phrase.as_str())),
            gestures,
            pindex: Index::new(phrases.iter().map(String::as_str)),
            phrases,
            phrase_class,
            classes,
            class_id,
            prior,
            plugin: None,
            recent: Vec::new(),
        }
    }

    /// The plugin whose parameters complete (the target picker's choice).
    pub fn set_plugin(&mut self, id: &str, params: Vec<Param>) {
        let class: Vec<Option<usize>> =
            params.iter().map(|p| self.class_id.get(&p.class).copied()).collect();
        let mut by_class = vec![Vec::new(); self.classes.len()];
        for (i, c) in class.iter().enumerate() {
            if let Some(c) = c {
                by_class[*c].push(i as u32);
            }
        }
        self.plugin = Some(Plugin {
            id: id.to_string(),
            index: Index::new(params.iter().map(|p| p.name.as_str())),
            params,
            class,
            by_class,
        });
    }

    pub fn plugin(&self) -> Option<&str> {
        self.plugin.as_ref().map(|p| p.id.as_str())
    }

    /// Remembers an accepted chip; recent picks rank up.
    pub fn picked(&mut self, chip: &Chip) {
        let id = chip.id();
        self.recent.retain(|r| *r != id);
        self.recent.push(id);
        if self.recent.len() > RECENT_CAP {
            self.recent.remove(0);
        }
    }

    fn recency(&self, id: &str) -> f32 {
        self.recent
            .iter()
            .rev()
            .position(|r| r == id)
            .map_or(0.0, |r| RECENT_MAX * (1.0 - r as f32 / RECENT_CAP as f32))
    }

    fn class_adjust(&self, class: Option<usize>, switchy: bool) -> f32 {
        let Some(c) = class else { return 0.0 };
        let name = self.classes[c].as_str();
        self.prior[c]
            + match name {
                SWITCH_CLASS if switchy => SWITCH_UP,
                SWITCH_CLASS => SWITCH_DOWN,
                "system" | "unsafe" => NOT_MAPPABLE,
                _ => 0.0,
            }
    }

    /// Top `k` completions for the word(s) before `cursor`; `floor` is the
    /// end of the last chip before the cursor (queries never reach into it).
    pub fn complete(&self, text: &str, cursor: usize, floor: usize, k: usize) -> Vec<Suggestion> {
        let spans = context::queries(text, cursor, floor);
        let Some(last) = spans.first() else { return Vec::new() };
        let clause = context::clause_start(text, cursor, floor);
        let switchy = context::switching(&text[clause..last.start]);
        let mut best: HashMap<String, Suggestion> = HashMap::new();
        let mut keep = |s: Suggestion, key: String| {
            if best.get(&key).is_none_or(|b| s.score > b.score) {
                best.insert(key, s);
            }
        };
        let mut sc = Scratch::default();
        for span in &spans {
            let q = Query::new(&text[span.start..cursor]);
            let Some(&first) = q.chars.first() else { continue };
            let first = first.to_string();
            let replace = span.start..cursor;
            let wb = WORD_BONUS * (span.words - 1) as f32;
            let pos = context::gesture_position(&text[..span.start]);
            if pos != context::GesturePos::No {
                let weak = pos == context::GesturePos::Weak;
                self.gindex.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
                for &i in &sc.ids {
                    let k = &self.gindex.keys[i as usize];
                    let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) else {
                        continue;
                    };
                    let prefix = q.is_prefix(k);
                    let g = &self.gestures[i as usize];
                    let chip = Chip::Gesture {
                        source: g.source.clone(),
                        shape: g.shape.clone(),
                        phrase: g.phrase.clone(),
                        pol: g.pol,
                    };
                    let key = format!("gesture:{}/{:?}/{}", g.source, g.shape, g.pol);
                    let score = m + GESTURE_BONUS + wb + self.recency(&chip.id());
                    keep(Suggestion { chip, score, replace: replace.clone(), via: None, prefix, weak }, key);
                }
            }
            let Some(p) = &self.plugin else { continue };
            // (param, match, via phrase, prefix)
            let mut cands: Vec<(u32, f32, Option<String>, bool)> = Vec::new();
            p.index.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
            for &i in &sc.ids {
                let k = &p.index.keys[i as usize];
                if let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) {
                    cands.push((i, m, None, q.is_prefix(k)));
                }
            }
            // best phrase per class: (match, phrase, prefix)
            let mut via: HashMap<usize, (f32, usize, bool)> = HashMap::new();
            self.pindex.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
            for &i in &sc.ids {
                let k = &self.pindex.keys[i as usize];
                let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) else {
                    continue;
                };
                let c = self.phrase_class[i as usize];
                if via.get(&c).is_none_or(|&(b, _, _)| m > b) {
                    via.insert(c, (m, i as usize, q.is_prefix(k)));
                }
            }
            for (&c, &(m, phrase, prefix)) in &via {
                for &i in &p.by_class[c] {
                    cands.push((i, m * VIA_PHRASE, Some(self.phrases[phrase].clone()), prefix));
                }
            }
            for (i, m, via, prefix) in cands {
                let par = &p.params[i as usize];
                let chip = Chip::Param {
                    plugin: p.id.clone(),
                    index: par.index,
                    name: par.name.clone(),
                    class: par.class.clone(),
                };
                let score = m + wb + self.class_adjust(p.class[i as usize], switchy) + self.recency(&chip.id());
                let key = chip.id();
                keep(Suggestion { chip, score, replace: replace.clone(), via, prefix, weak: false }, key);
            }
        }
        let mut out: Vec<Suggestion> = best.into_values().collect();
        out.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chip.label().len().cmp(&b.chip.label().len()))
                .then_with(|| param_index(&a.chip).cmp(&param_index(&b.chip)))
                .then_with(|| a.chip.label().cmp(b.chip.label()))
                .then_with(|| a.chip.id().cmp(&b.chip.id()))
        });
        contrast(&mut out);
        out.truncate(k);
        out
    }
}

/// What a list row offers: a gesture source, or a parameter class.
fn kind(c: &Chip) -> &str {
    match c {
        Chip::Param { class, .. } => class,
        Chip::Gesture { source, .. } => source,
    }
}

/// At most this many rows of one kind before the other kinds get a row.
const PER_KIND: usize = 3;

/// Moves rows past the third of one kind (seven send levels for "vol")
/// behind the other kinds, so a long list also holds the readings the user
/// may have meant. The order within a kind stays; a top 3 never changes.
fn contrast(out: &mut Vec<Suggestion>) {
    if out.len() <= PER_KIND {
        return;
    }
    let mut count: HashMap<String, usize> = HashMap::new();
    let (front, back): (Vec<Suggestion>, Vec<Suggestion>) = std::mem::take(out).into_iter().partition(|s| {
        let n = count.entry(kind(&s.chip).to_string()).or_default();
        *n += 1;
        *n <= PER_KIND
    });
    *out = front;
    out.extend(back);
}

fn param_index(c: &Chip) -> u32 {
    match c {
        Chip::Param { index, .. } => *index,
        Chip::Gesture { .. } => 0,
    }
}

#[cfg(test)]
mod tests;
