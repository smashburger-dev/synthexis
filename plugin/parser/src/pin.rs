//! Pinned spans from the sentence field's chips, enforced on a decoded
//! frame. The user picked these parts while typing, so they win over what
//! the parser guessed:
//!
//! - a parameter chip goes to a target, not a whole route: one route may
//!   drive several targets (fan-out, "resonance and cutoff"). It takes the
//!   free target whose words it covers (the BIO head's name groups,
//!   align.rs; the parser's other targets of the same words go), else a
//!   free target of its class, else the only free parameter or toggle
//!   target when it is the last chip, else one more target in the nearest
//!   route that has a source; that target becomes T:param:<class>
//!   (T:toggle for a switch) and carries the parameter id. The words decide
//!   first because a real parameter's class often differs from the
//!   parser's (probes/pin-hill: with a class that matches nothing, chip_edit
//!   0.608 by class against 0.834 by words);
//! - a gesture chip goes to the route that already has its source; the
//!   others go in sentence order to the free routes when their counts are
//!   equal, else each to the free route nearest to it (by its spans and
//!   pinned parameter); the route's S: token becomes the chip's source (and
//!   shape:, and side: when the panel picked one);
//! - a target chip (MIDI note, chord, pattern, CC, generator property,
//!   macro, XY pad) goes to the nearest route with a free target token, one
//!   of its own kind first; that token becomes T:<kind>: (T:midi.prop:<prop>
//!   for a property) and keeps its value when the kind was already right;
//! - a chip no route takes gets a route of its own.
//!
//! Runs after decoding, never inside the grammar: the parser still reads
//! the chips as plain words.

use crate::{align, Item, Tagged};

#[derive(Clone, Debug, PartialEq)]
pub enum PinWhat {
    /// A parameter of the picked plugin; a switch parameter is a toggle.
    Param { id: String, class: String, switch: bool },
    /// A gesture source, with a shape id when the source is hand.shape.
    /// `side`: left, right or both, picked in the variant panel; None keeps
    /// the side the parser read from the sentence.
    Gesture { source: String, shape: Option<String>, side: Option<String> },
    /// A target that is no plugin parameter: midi.note, midi.chord,
    /// midi.pattern, midi.cc, macro, xy, or midi.prop:<prop> with its
    /// property.
    Target { kind: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// Byte range of the chip in the parsed sentence.
    pub bytes: (usize, usize),
    pub what: PinWhat,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    /// Items between <route> and </route>.
    pub items: Vec<Item>,
    /// Pinned parameter id per item: Some on a pinned target token.
    pub pinned: Vec<Option<String>>,
    pub source_pinned: bool,
    /// Pinned source per item: true on an S: token a gesture chip set.
    src_pinned: Vec<bool>,
    /// True on a T: token a target chip set.
    tgt_pinned: Vec<bool>,
    /// Bytes from the first to the last span or pinned chip.
    anchor: Option<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Item(Item),
    Route(Route),
}

/// A frame with its routes split out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub parts: Vec<Part>,
}

fn tok(it: &Item) -> &str {
    match it {
        Item::Token(t) => t,
        Item::Span { .. } => "",
    }
}

impl Route {
    fn new(items: Vec<Item>) -> Self {
        let pinned = vec![None; items.len()];
        let src_pinned = vec![false; items.len()];
        let tgt_pinned = vec![false; items.len()];
        let mut r = Self { items, pinned, src_pinned, tgt_pinned, ..Self::default() };
        let spans: Vec<(usize, usize)> = r
            .items
            .iter()
            .filter_map(|it| match it {
                Item::Span { bytes, .. } => Some(*bytes),
                Item::Token(_) => None,
            })
            .collect();
        spans.into_iter().for_each(|b| r.widen(b));
        r
    }

    fn widen(&mut self, b: (usize, usize)) {
        self.anchor = Some(self.anchor.map_or(b, |a| (a.0.min(b.0), a.1.max(b.1))));
    }

    /// Bytes between the anchor and `b`; 0 when they overlap.
    fn gap(&self, b: (usize, usize)) -> usize {
        self.anchor
            .map_or(usize::MAX, |a| a.0.saturating_sub(b.1).max(b.0.saturating_sub(a.1)))
    }

    /// Pinned parameter ids in item order.
    pub fn params(&self) -> impl Iterator<Item = &str> {
        self.pinned.iter().flatten().map(String::as_str)
    }

    fn insert(&mut self, i: usize, it: Item) {
        self.items.insert(i, it);
        self.pinned.insert(i, None);
        self.src_pinned.insert(i, false);
        self.tgt_pinned.insert(i, false);
    }

    fn remove(&mut self, i: usize) {
        self.items.remove(i);
        self.pinned.remove(i);
        self.src_pinned.remove(i);
        self.tgt_pinned.remove(i);
    }

    /// Source slots a gesture chip can take: the unpinned S: tokens (a
    /// fan-in route has several); a route without any S: has one slot.
    fn free_sources(&self) -> Vec<Option<usize>> {
        let s: Vec<usize> = (0..self.items.len()).filter(|&i| tok(&self.items[i]).starts_with("S:")).collect();
        if s.is_empty() {
            return if self.source_pinned { Vec::new() } else { vec![None] };
        }
        s.into_iter().filter(|&i| !self.src_pinned[i]).map(Some).collect()
    }

    /// The unpinned S: token that already names the chip's movement.
    fn own_source(&self, source: &str, shape: Option<&str>) -> Option<usize> {
        let src = fold(source).0;
        self.free_sources().into_iter().flatten().find(|&i| {
            tok(&self.items[i]).strip_prefix("S:").is_some_and(|s| fold(s).0 == src)
                && shape.is_none_or(|sh| self.shape_of(i).is_some_and(|j| tok(&self.items[j]) == format!("shape:{sh}")))
        })
    }

    /// The tokens right after the S: token at `i` that belong to it (side,
    /// shape, object); a later gate's side: is the gate's.
    fn attrs_of(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        (i + 1..self.items.len())
            .take_while(|&j| ["side:", "shape:", "object:"].iter().any(|p| tok(&self.items[j]).starts_with(p)))
    }

    /// The side: token that belongs to the S: token at `i`.
    fn side_of(&self, i: usize) -> Option<usize> {
        self.attrs_of(i).find(|&j| tok(&self.items[j]).starts_with("side:"))
    }

    /// The shape: token that belongs to the S: token at `i`.
    fn shape_of(&self, i: usize) -> Option<usize> {
        self.attrs_of(i).find(|&j| tok(&self.items[j]).starts_with("shape:"))
    }

    pub(crate) fn is_param_target(t: &str) -> bool {
        t.starts_with("T:param:") || t.starts_with("T:toggle:")
    }

    /// Unpinned parameter and toggle targets.
    fn free_targets(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.pinned[i].is_none() && Self::is_param_target(tok(&self.items[i])))
            .collect()
    }

    /// A free target of the chip's class (any toggle for a switch).
    fn target_of_class(&self, class: &str, switch: bool) -> Option<usize> {
        self.free_targets().into_iter().find(|&i| {
            let t = tok(&self.items[i]);
            if switch {
                t.starts_with("T:toggle:")
            } else {
                t.strip_prefix("T:param:") == Some(class)
            }
        })
    }

    /// An unpinned T: token for a target chip of `kind`: one of that kind
    /// first, else the first of any kind; with whether the kind matched.
    fn target_for_kind(&self, kind: &str) -> Option<(usize, bool)> {
        let free: Vec<usize> = (0..self.items.len())
            .filter(|&i| self.pinned[i].is_none() && !self.tgt_pinned[i] && tok(&self.items[i]).starts_with("T:"))
            .collect();
        let same = free.iter().find(|&&i| same_target(tok(&self.items[i]), kind));
        same.map(|&i| (i, true)).or_else(|| free.first().map(|&i| (i, false)))
    }

    /// Pins the target token at `at` (None: a new one after the sources) to
    /// the chip's kind.
    fn set_target(&mut self, at: Option<usize>, kind: &str, bytes: (usize, usize)) {
        let token = if kind.contains(':') { format!("T:{kind}") } else { format!("T:{kind}:") };
        let i = match at {
            Some(i) if same_target(tok(&self.items[i]), kind) => i,
            Some(i) => {
                self.items[i] = Item::Token(token);
                i
            }
            None => {
                let i = self.after_source();
                self.insert(i, Item::Token(token));
                i
            }
        };
        self.tgt_pinned[i] = true;
        self.widen(bytes);
    }

    fn has_source(&self) -> bool {
        self.items.iter().any(|it| tok(it).starts_with("S:"))
    }

    /// Position after the leading source tokens.
    fn after_source(&self) -> usize {
        self.items
            .iter()
            .take_while(|it| ["S:", "side:", "shape:", "object:"].iter().any(|p| tok(it).starts_with(p)))
            .count()
    }

    /// Pins the target at `at` (None: a new target after the sources).
    fn set_param(&mut self, target: Option<usize>, id: &str, class: &str, switch: bool, bytes: (usize, usize)) {
        let token = if switch {
            // keep the parser's effect family if it already chose a toggle
            match target.map(|i| tok(&self.items[i])) {
                Some(t) if t.starts_with("T:toggle:") => t.to_string(),
                _ => "T:toggle:".to_string(),
            }
        } else {
            format!("T:param:{class}")
        };
        let i = match target {
            Some(i) => {
                self.items[i] = Item::Token(token);
                i
            }
            None => {
                let i = self.after_source();
                self.insert(i, Item::Token(token));
                i
            }
        };
        self.pinned[i] = Some(id.to_string());
        self.widen(bytes);
    }

    /// Sets the source slot (an S: token, or None: a new one) to the
    /// chip's source, folded like conventions.py (closing the fist is the
    /// hand's openness inverted). INV flips with the fold when the chip
    /// replaces another movement; when the slot already names the same
    /// movement only its own fold counts.
    fn set_source(&mut self, slot: Option<usize>, source: &str, shape: Option<&str>, side: Option<&str>) {
        let (src, chip_flip) = fold(source);
        let flip = match slot.map(|i| fold(&tok(&self.items[i])[2..])) {
            Some((c, own_flip)) if c == src => own_flip,
            _ => chip_flip,
        };
        let s = Item::Token(format!("S:{src}"));
        let i = match slot {
            Some(i) => {
                self.items[i] = s;
                i
            }
            None => {
                self.insert(0, s);
                0
            }
        };
        self.src_pinned[i] = true;
        if let Some(sh) = shape {
            let t = Item::Token(format!("shape:{sh}"));
            match self.shape_of(i) {
                Some(j) => self.items[j] = t,
                None => self.insert(i + 1, t),
            }
        }
        // the side right after its S: token, as the parser writes it
        if let Some(sd) = side {
            let t = Item::Token(format!("side:{sd}"));
            match self.side_of(i) {
                Some(j) => self.items[j] = t,
                None => self.insert(i + 1, t),
            }
        }
        if flip {
            match self.items.iter().position(|it| tok(it) == "INV") {
                Some(k) => self.remove(k),
                None => self.insert(self.items.len(), Item::Token("INV".into())),
            }
        }
        self.source_pinned = true;
    }
}

/// "T:midi.cc:74" -> "midi.cc".
fn target_kind(t: &str) -> &str {
    t.strip_prefix("T:").and_then(|r| r.split(':').next()).unwrap_or("")
}

/// The token already is the chip's target: the same kind, and for a kind
/// with a value (midi.prop:velocity) the same value.
fn same_target(t: &str, kind: &str) -> bool {
    if kind.contains(':') {
        t.strip_prefix("T:") == Some(kind)
    } else {
        target_kind(t) == kind
    }
}

/// conventions.FOLD: (folded source, whether INV flips).
fn fold(source: &str) -> (&str, bool) {
    match source {
        "hand.fist" => ("hand.openness", true),
        "eye.closed" => ("eye.open", true),
        s => (s, false),
    }
}

impl Frame {
    pub fn from_items(items: &[Item]) -> Self {
        let mut parts = Vec::new();
        let mut cur: Option<Vec<Item>> = None;
        for it in items {
            match (tok(it), cur.as_mut()) {
                ("<route>", None) => cur = Some(Vec::new()),
                ("</route>", Some(_)) => {
                    if let Some(r) = cur.take() {
                        parts.push(Part::Route(Route::new(r)));
                    }
                }
                (_, Some(r)) => r.push(it.clone()),
                (_, None) => parts.push(Part::Item(it.clone())),
            }
        }
        if let Some(r) = cur {
            parts.push(Part::Route(Route::new(r)));
        }
        Self { parts }
    }

    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.parts.iter().filter_map(|p| match p {
            Part::Route(r) => Some(r),
            Part::Item(_) => None,
        })
    }

    fn routes_mut(&mut self) -> Vec<&mut Route> {
        self.parts
            .iter_mut()
            .filter_map(|p| match p {
                Part::Route(r) => Some(r),
                Part::Item(_) => None,
            })
            .collect()
    }

    /// Frame strings as Parse::frame prints them; a pinned parameter shows
    /// as "@<id>" right after its target.
    pub fn frame(&self) -> Vec<String> {
        let mut out = Vec::new();
        let item = |it: &Item| match it {
            Item::Token(t) => t.clone(),
            Item::Span { text, .. } => format!("={text}"),
        };
        for p in &self.parts {
            match p {
                Part::Item(it) => out.push(item(it)),
                Part::Route(r) => {
                    out.push("<route>".into());
                    for (it, id) in r.items.iter().zip(&r.pinned) {
                        out.push(item(it));
                        if let Some(id) = id {
                            out.push(format!("@{id}"));
                        }
                    }
                    out.push("</route>".into());
                }
            }
        }
        out
    }

    fn add_route(&mut self, r: Route) {
        // a pinned route makes "nothing to map" wrong
        match self.parts.iter().position(|p| matches!(p, Part::Item(it) if tok(it).starts_with("I:"))) {
            Some(i) if matches!(&self.parts[i], Part::Item(it) if tok(it) == "I:none") => {
                self.parts[i] = Part::Item(Item::Token("I:map".into()));
            }
            Some(_) => {}
            None => self.parts.insert(0, Part::Item(Item::Token("I:map".into()))),
        }
        self.parts.push(Part::Route(r));
    }
}

/// A gesture chip while it is placed: bytes, source, shape, side.
type GesturePin<'a> = ((usize, usize), &'a str, Option<&'a str>, Option<&'a str>);

/// Enforces `pins` on the decoded `items` (see the module doc).
pub fn pin(items: &[Item], pins: &[Pin]) -> Frame {
    pin_spans(items, "", &[], pins)
}

/// `pin` with the parse's BIO spans: a parameter chip first takes the
/// target whose words it covers (align.rs); the other targets the parser
/// made of the same words go, since the chip names one parameter.
pub fn pin_spans(items: &[Item], text: &str, spans: &[Tagged], pins: &[Pin]) -> Frame {
    let mut f = Frame::from_items(items);
    let mut pins: Vec<&Pin> = pins.iter().collect();
    pins.sort_by_key(|p| p.bytes.0);
    let params = pins.iter().filter_map(|p| match &p.what {
        PinWhat::Param { id, class, switch } => Some((p.bytes, id, class, *switch)),
        _ => None,
    });
    let params: Vec<_> = params.collect();
    let mut placed = vec![false; params.len()];
    // 1. the target the chip's words name: among the free slots of the
    // group its bytes overlap, one of its class, else a parameter or toggle
    // slot, else any. Then the targets whose own words lie inside the chip
    // go (another group it covers, a misread of its words), unless another
    // chip touches them; a slot that only inherited the group (`shares`,
    // the parser made more targets than the BIO head found words) stays,
    // and the resolver drops it when it binds the chip's parameter again.
    if !spans.is_empty() {
        let al = align::align(&f, text, spans);
        let mut fold: Vec<(usize, usize)> = Vec::new();
        let chips: Vec<(usize, usize)> = pins.iter().map(|p| p.bytes).collect();
        for (k, &(bytes, id, class, switch)) in params.iter().enumerate() {
            let Some(g) = al.group_at(bytes) else { continue };
            let mut routes = f.routes_mut();
            let free: Vec<(usize, usize)> =
                al.slots_of(g).filter(|(r, s)| routes[*r].pinned[s.item].is_none()).map(|(r, s)| (r, s.item)).collect();
            let token = |&(r, i): &(usize, usize)| tok(&routes[r].items[i]).to_string();
            let pick = free
                .iter()
                .find(|x| {
                    let t = token(x);
                    if switch { t.starts_with("T:toggle:") } else { t.strip_prefix("T:param:") == Some(class) }
                })
                .or_else(|| free.iter().find(|x| Route::is_param_target(&token(x))))
                .or(free.first())
                .copied();
            let Some((r, i)) = pick else { continue };
            routes[r].set_param(Some(i), id, class, switch, bytes);
            placed[k] = true;
            for (g2, &(gb, ge)) in al.groups.iter().enumerate() {
                let inside = bytes.0 <= gb && ge <= bytes.1;
                let alone = chips.iter().filter(|c| c.0 < ge && gb < c.1).count() == 1;
                if inside && alone {
                    fold.extend(al.slots_of(g2).filter(|(fr, s)| !s.shares && (*fr, s.item) != (r, i)).map(|(fr, s)| (fr, s.item)));
                }
            }
        }
        fold.sort_unstable_by(|a, b| b.cmp(a));
        fold.dedup();
        let mut routes = f.routes_mut();
        for (r, i) in fold {
            if routes[r].pinned[i].is_none() && tok(&routes[r].items[i]).starts_with("T:") {
                routes[r].remove(i);
            }
        }
    }
    // 2. a free target of the chip's class
    for (k, &(bytes, id, class, switch)) in params.iter().enumerate() {
        if placed[k] {
            continue;
        }
        let mut routes = f.routes_mut();
        let hit = routes.iter_mut().find_map(|r| r.target_of_class(class, switch).map(|t| (r, t)));
        if let Some((r, t)) = hit {
            r.set_param(Some(t), id, class, switch, bytes);
            placed[k] = true;
        }
    }
    // 3. the only free target takes the only unplaced chip
    if placed.iter().filter(|p| !**p).count() == 1 {
        let mut routes = f.routes_mut();
        let free: Vec<(usize, usize)> = routes
            .iter()
            .enumerate()
            .flat_map(|(ri, r)| r.free_targets().into_iter().map(move |t| (ri, t)))
            .collect();
        if let ([(ri, t)], Some(k)) = (free.as_slice(), placed.iter().position(|p| !p)) {
            let (bytes, id, class, switch) = params[k];
            routes[*ri].set_param(Some(*t), id, class, switch, bytes);
            placed[k] = true;
        }
    }
    // 4. a chip no target takes joins the nearest route with a source as
    // one more target (the parser left it out: "more dreamy and ring
    // longer"); a route of its own would have nothing to drive it
    // (probes/pin-hill, C2)
    for (k, &(bytes, id, class, switch)) in params.iter().enumerate() {
        if placed[k] {
            continue;
        }
        let mut routes = f.routes_mut();
        match routes.iter_mut().filter(|r| r.has_source()).min_by_key(|r| r.gap(bytes)) {
            Some(r) => r.set_param(None, id, class, switch, bytes),
            None => {
                let mut r = Route::default();
                r.set_param(None, id, class, switch, bytes);
                f.add_route(r);
            }
        }
    }
    for p in &pins {
        let PinWhat::Target { kind } = &p.what else { continue };
        let mut routes = f.routes_mut();
        let near = routes
            .iter_mut()
            .filter_map(|r| r.target_for_kind(kind).map(|(i, same)| (r, i, same)))
            .min_by_key(|(r, _, same)| (!same, r.gap(p.bytes)));
        match near {
            Some((r, i, _)) => r.set_target(Some(i), kind, p.bytes),
            None => {
                let mut r = Route::default();
                r.set_target(None, kind, p.bytes);
                f.add_route(r);
            }
        }
    }
    // a side token outside the routes holds for all of them ("my right
    // thumb ..., index ..., middle ..."); a chip with that side adds none
    let shared: Vec<String> = f
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Item(it) if tok(it).starts_with("side:") => Some(tok(it)[5..].to_string()),
            _ => None,
        })
        .collect();
    let mut gestures: Vec<GesturePin> = pins
        .iter()
        .filter_map(|p| match &p.what {
            PinWhat::Gesture { source, shape, side } => {
                let side = side.as_deref().filter(|s| !shared.iter().any(|x| x == s));
                Some((p.bytes, source.as_str(), shape.as_deref(), side))
            }
            _ => None,
        })
        .collect();
    // 1. the S: token that already names the movement, in the nearest
    // route that has one ("openness to Signal 19 and left fist": the fist
    // folds to openness and belongs to the second route)
    gestures.retain(|&(bytes, source, shape, side)| {
        let mut routes = f.routes_mut();
        let own = routes.iter_mut().filter_map(|r| r.own_source(source, shape).map(|i| (r, i)));
        match own.min_by_key(|(r, _)| r.gap(bytes)) {
            Some((r, i)) => {
                r.set_source(Some(i), source, shape, side);
                false
            }
            None => true,
        }
    });
    // 2. in order when there are as many free source slots as chips, else
    // each to the nearest route with a free slot
    let mut extra = Vec::new();
    let mut routes = f.routes_mut();
    let slots: Vec<(usize, usize)> = routes
        .iter()
        .enumerate()
        .flat_map(|(ri, r)| (0..r.free_sources().len()).map(move |k| (ri, k)))
        .collect();
    if slots.len() == gestures.len() {
        // highest slot first inside a route, so earlier slots keep their index
        let mut pairs: Vec<_> = slots.into_iter().zip(&gestures).collect();
        pairs.sort_by(|a, b| a.0 .0.cmp(&b.0 .0).then(b.0 .1.cmp(&a.0 .1)));
        for ((ri, k), &(_, source, shape, side)) in pairs {
            let slot = routes[ri].free_sources()[k];
            routes[ri].set_source(slot, source, shape, side);
        }
    } else {
        for &(bytes, source, shape, side) in &gestures {
            let near = routes
                .iter_mut()
                .filter(|r| !r.free_sources().is_empty())
                .min_by_key(|r| r.gap(bytes));
            match near {
                Some(r) => {
                    let slot = r.free_sources()[0];
                    r.set_source(slot, source, shape, side);
                }
                None => extra.push((bytes, source, shape, side)),
            }
        }
    }
    // 3. a route of its own
    for (bytes, source, shape, side) in extra {
        let mut r = Route::default();
        r.set_source(None, source, shape, side);
        r.widen(bytes);
        f.add_route(r);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(frame: &[&str], text: &str) -> Vec<Item> {
        // "=span" items point at the first occurrence of the span in `text`
        frame
            .iter()
            .map(|t| match t.strip_prefix('=') {
                Some(s) => {
                    let b = text.to_lowercase().find(s).unwrap_or(0);
                    Item::Span { text: s.to_string(), bytes: (b, b + s.len()) }
                }
                None => Item::Token(t.to_string()),
            })
            .collect()
    }

    fn at(text: &str, chip: &str) -> (usize, usize) {
        let b = text.find(chip).unwrap_or(0);
        (b, b + chip.len())
    }

    fn param(text: &str, chip: &str, id: &str, class: &str, switch: bool) -> Pin {
        Pin { bytes: at(text, chip), what: PinWhat::Param { id: id.into(), class: class.into(), switch } }
    }

    fn gesture(text: &str, chip: &str, source: &str) -> Pin {
        Pin { bytes: at(text, chip), what: PinWhat::Gesture { source: source.into(), shape: None, side: None } }
    }

    fn sided(text: &str, chip: &str, source: &str, side: &str) -> Pin {
        Pin { bytes: at(text, chip), what: PinWhat::Gesture { source: source.into(), shape: None, side: Some(side.into()) } }
    }

    fn run(text: &str, frame: &[&str], pins: &[Pin]) -> Vec<String> {
        pin(&items(frame, text), pins).frame()
    }

    fn target(text: &str, chip: &str, kind: &str) -> Pin {
        Pin { bytes: at(text, chip), what: PinWhat::Target { kind: kind.into() } }
    }

    #[test]
    fn target_chip_keeps_the_kind_and_value_the_parser_chose() {
        let t = "when I spread my fingers play the kick pattern";
        let fr = ["I:map", "<route>", "S:hand.spread", "T:midi.pattern:kick pattern", "</route>"];
        assert_eq!(run(t, &fr, &[target(t, "kick pattern", "midi.pattern")]), fr);
    }

    #[test]
    fn target_chip_replaces_a_wrong_kind() {
        let t = "wenn ich die Faust mache Akkord spielen";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:other", "</route>"];
        let out = run(t, &fr, &[target(t, "Akkord spielen", "midi.chord")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.fist", "T:midi.chord:", "</route>"]);
    }

    #[test]
    fn property_chip_sets_its_property() {
        let t = "pinch changes the arp rate";
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:midi.prop:velocity", "</route>"];
        let out = run(t, &fr, &[target(t, "arp rate", "midi.prop:arp.rate")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.pinch", "T:midi.prop:arp.rate", "</route>"]);
    }

    #[test]
    fn target_chip_without_a_target_gets_a_route() {
        let t = "Akkord spielen";
        let out = run(t, &["I:none"], &[target(t, "Akkord spielen", "midi.chord")]);
        assert_eq!(out, ["I:map", "<route>", "T:midi.chord:", "</route>"]);
    }

    #[test]
    fn no_pins_keeps_frame() {
        let t = "when i hold still the width on pluck goes up";
        let fr = ["I:map", "<route>", "S:motion.hold", "T:param:stereo.width", "TRACK", "=pluck", "</route>"];
        assert_eq!(run(t, &fr, &[]), fr);
    }

    #[test]
    fn chip_takes_the_target_its_words_name() {
        let t = "when I make a fist the Filter 1 Freq goes up";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:fx.tone", "PNAME", "=filter 1 freq", "</route>"];
        let b = at(t, "Filter 1 Freq");
        let spans = [Tagged { tag: "PNAME", bytes: b }];
        let pins = [param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false)];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        assert_eq!(
            out,
            ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "@p#205", "PNAME", "=filter 1 freq", "</route>"]
        );
    }

    /// v11 reads one long name as two classes; the chip on it keeps one.
    #[test]
    fn a_chip_on_a_split_name_folds_the_extra_target() {
        let t = "when I open my mouth pan mid env attack on ShaperBox goes up";
        let fr = ["I:map", "<route>", "S:mouth.open", "T:param:amp.pan", "T:param:env.attack", "</route>"];
        let spans = [Tagged { tag: "TGT", bytes: at(t, "pan") }, Tagged { tag: "TGT", bytes: at(t, "mid env attack") }];
        let pins = [param(t, "pan mid env attack", "p#7", "env.attack", false)];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        // the chip takes the inherited slot of its class; the slot with the
        // words goes
        assert_eq!(out, ["I:map", "<route>", "S:mouth.open", "T:param:env.attack", "@p#7", "</route>"]);
        // with a class that matches nothing the chip takes the first slot;
        // the inherited one stays for the resolver to drop
        let pins = [param(t, "pan mid env attack", "p#7", "other", false)];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        assert_eq!(out, ["I:map", "<route>", "S:mouth.open", "T:param:other", "@p#7", "T:param:env.attack", "</route>"]);
    }

    /// Two chips in one joined group fold nothing and each takes its own
    /// class's slot; a target chip in the group keeps its token, in either
    /// order; a gate's side is not the source's.
    #[test]
    fn partial_chips_and_target_chips_never_fold() {
        let t = "when I pinch Cutoff Resonance Drive go up";
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "T:param:filter.resonance", "T:param:fx.drive", "</route>"];
        let spans = [
            Tagged { tag: "TGT", bytes: at(t, "Cutoff") },
            Tagged { tag: "TGT", bytes: at(t, "Resonance") },
            Tagged { tag: "TGT", bytes: at(t, "Drive") },
        ];
        let pins = [param(t, "Resonance", "p#2", "filter.resonance", false), param(t, "Cutoff", "p#1", "filter.cutoff", false)];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        assert_eq!(
            out,
            [
                "I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "@p#1", "T:param:filter.resonance", "@p#2",
                "T:param:fx.drive", "</route>"
            ]
        );
        let t = "when I pinch Cutoff Kick Pattern";
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "T:midi.pattern:kick pattern", "</route>"];
        let spans = [Tagged { tag: "TGT", bytes: at(t, "Cutoff") }, Tagged { tag: "TGT", bytes: at(t, "Kick Pattern") }];
        let pins = [param(t, "Cutoff", "p#1", "filter.cutoff", false), target(t, "Kick Pattern", "midi.pattern")];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        assert!(out.contains(&"T:midi.pattern:kick pattern".to_string()), "{out:?}");
        let t = "when I pinch Kick Pattern Cutoff";
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:midi.pattern:kick pattern", "T:param:filter.cutoff", "</route>"];
        let spans = [Tagged { tag: "TGT", bytes: at(t, "Kick Pattern") }, Tagged { tag: "TGT", bytes: at(t, "Cutoff") }];
        let pins = [param(t, "Cutoff", "p#1", "filter.freq", false), target(t, "Kick Pattern", "midi.pattern")];
        let out = pin_spans(&items(&fr, t), t, &spans, &pins).frame();
        assert!(out.contains(&"T:midi.pattern:kick pattern".to_string()), "{out:?}");
        // a sided chip on "S:hand.y ... G:hand.fist side:right" sets the
        // source's side and leaves the gate's
        let t = "the more hand height the more pitch while I hold the right fist";
        let fr = ["I:map", "<route>", "S:hand.y", "T:param:pitch", "G:hand.fist", "side:right", "</route>"];
        let out = run(t, &fr, &[sided(t, "hand height", "hand.y", "left")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.y", "side:left", "T:param:pitch", "G:hand.fist", "side:right", "</route>"]);
    }

    #[test]
    fn class_match_without_pointer() {
        let t = "spread my fingers opens Filter 1 Freq and the mouth drives Reverb Mix";
        let fr = [
            "I:map", "<route>", "S:hand.spread", "T:param:filter.cutoff", "</route>",
            "<route>", "S:mouth.open", "T:param:fx.reverb.mix", "</route>",
        ];
        let pins = [
            param(t, "Filter 1 Freq", "p#1", "filter.cutoff", false),
            param(t, "Reverb Mix", "p#9", "fx.reverb.mix", false),
        ];
        let f = pin(&items(&fr, t), &pins);
        let ids: Vec<Vec<&str>> = f.routes().map(|r| r.params().collect()).collect();
        assert_eq!(ids, [vec!["p#1"], vec!["p#9"]]);
    }

    #[test]
    fn single_route_takes_single_chip_of_other_class() {
        let t = "wenn ich die Faust schließe geht Filter 1 Freq hoch";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:fx.eq.freq", "</route>"];
        let out = run(t, &fr, &[param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false)]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "@p#205", "</route>"]);
    }

    #[test]
    fn switch_chip_makes_toggle() {
        let t = "pinch to switch Filter 1 On";
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "mode:toggle", "</route>"];
        let out = run(t, &fr, &[param(t, "Filter 1 On", "p#203", "ctl.switch", true)]);
        assert_eq!(out[3..5], ["T:toggle:", "@p#203"]);
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:toggle:filter", "</route>"];
        let out = run(t, &fr, &[param(t, "Filter 1 On", "p#203", "ctl.switch", true)]);
        assert_eq!(out[3..5], ["T:toggle:filter", "@p#203"]);
    }

    #[test]
    fn uncovered_chip_joins_the_nearest_route() {
        let t = "my fist drives the cutoff and also Reverb Mix";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "</route>"];
        let pins = [param(t, "Reverb Mix", "p#9", "fx.reverb.mix", false)];
        // one route, one chip, but its class differs: rule 3 takes it
        assert_eq!(pin(&items(&fr, t), &pins).routes().count(), 1);
        let pins = [
            param(t, "cutoff", "p#1", "filter.cutoff", false),
            param(t, "Reverb Mix", "p#9", "fx.reverb.mix", false),
        ];
        // the parser wrote one target for two chips: the second joins the
        // fist's route instead of a route with nothing to drive it
        let f = pin(&items(&fr, t), &pins);
        assert_eq!(f.routes().count(), 1);
        assert_eq!(
            f.frame(),
            ["I:map", "<route>", "S:hand.fist", "T:param:fx.reverb.mix", "@p#9", "T:param:filter.cutoff", "@p#1", "</route>"]
        );
    }

    #[test]
    fn none_intent_becomes_map_when_routes_are_added() {
        let t = "Filter 1 Freq";
        let out = run(t, &["I:none"], &[param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false)]);
        assert_eq!(out, ["I:map", "<route>", "T:param:filter.cutoff", "@p#205", "</route>"]);
    }

    #[test]
    fn gesture_chip_replaces_source() {
        let t = "when I spread my fingers the width on pluck goes up";
        let fr = ["I:map", "<route>", "S:hand.openness", "T:param:stereo.width", "TRACK", "=pluck", "</route>"];
        let out = run(t, &fr, &[gesture(t, "spread my fingers", "hand.spread")]);
        assert_eq!(out[2], "S:hand.spread");
    }

    #[test]
    fn gesture_chip_goes_to_nearest_route() {
        let t = "the Filter 1 Freq follows my mouth and Reverb Mix follows a pinch";
        let fr = [
            "I:map", "<route>", "S:hand.y", "T:param:filter.cutoff", "PNAME", "=filter 1 freq", "</route>",
            "<route>", "S:hand.y", "T:param:fx.reverb.mix", "PNAME", "=reverb mix", "</route>",
        ];
        let pins = [gesture(t, "a pinch", "hand.pinch"), gesture(t, "my mouth", "mouth.open")];
        let out = run(t, &fr, &pins);
        assert_eq!(out[2], "S:mouth.open");
        assert_eq!(out[8], "S:hand.pinch");
    }

    #[test]
    fn gesture_already_right_stays() {
        let t = "pinch on cutoff, mouth on reverb";
        let fr = [
            "I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "</route>",
            "<route>", "S:mouth.open", "T:param:fx.reverb.mix", "</route>",
        ];
        let out = run(t, &fr, &[gesture(t, "mouth", "mouth.open"), gesture(t, "pinch", "hand.pinch")]);
        assert_eq!(out, fr);
    }

    #[test]
    fn fist_chip_folds_into_openness() {
        let t = "when I close my fist the cutoff rises";
        // parser read another movement: the fold flips INV
        let fr = ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "</route>"];
        let out = run(t, &fr, &[gesture(t, "close my fist", "hand.fist")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.openness", "T:param:filter.cutoff", "INV", "</route>"]);
        // parser already folded it: INV stays as the parser set it
        let fr = ["I:map", "<route>", "S:hand.openness", "T:param:filter.cutoff", "INV", "</route>"];
        assert_eq!(run(t, &fr, &[gesture(t, "close my fist", "hand.fist")]), fr);
        // parser said fist without the fold: the route's own fold flips
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "</route>"];
        let out = run(t, &fr, &[gesture(t, "close my fist", "hand.fist")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.openness", "T:param:filter.cutoff", "INV", "</route>"]);
    }

    #[test]
    fn gesture_chip_keeps_the_side() {
        let t = "my left hand should drive the cutoff by pinching";
        let fr = ["I:map", "<route>", "S:hand.openness", "side:left", "T:param:filter.cutoff", "</route>"];
        let out = run(t, &fr, &[gesture(t, "pinching", "hand.pinch")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.pinch", "side:left", "T:param:filter.cutoff", "</route>"]);
    }

    #[test]
    fn side_chip_adds_its_side() {
        let t = "when I make a fist the cutoff goes up";
        let fr = ["I:map", "<route>", "S:hand.openness", "T:param:filter.cutoff", "INV", "</route>"];
        let out = run(t, &fr, &[sided(t, "fist", "hand.fist", "left")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.openness", "side:left", "T:param:filter.cutoff", "INV", "</route>"]);
    }

    #[test]
    fn side_chip_replaces_the_parsers_side() {
        let t = "my left hand pinches the cutoff";
        let fr = ["I:map", "<route>", "S:hand.pinch", "side:left", "T:param:filter.cutoff", "</route>"];
        let out = run(t, &fr, &[sided(t, "pinches", "hand.pinch", "right")]);
        assert_eq!(out, ["I:map", "<route>", "S:hand.pinch", "side:right", "T:param:filter.cutoff", "</route>"]);
    }

    #[test]
    fn side_goes_with_its_own_source_in_a_fan_in() {
        let t = "smile and pinch to move the osc shape";
        let fr = ["I:map", "<route>", "S:face.smile", "S:hand.pinch", "T:param:osc.shape", "</route>"];
        let out = run(t, &fr, &[sided(t, "pinch", "hand.pinch", "both")]);
        assert_eq!(out, ["I:map", "<route>", "S:face.smile", "S:hand.pinch", "side:both", "T:param:osc.shape", "</route>"]);
    }

    #[test]
    fn side_shared_by_all_routes_is_not_repeated() {
        let t = "when i lift my right thumb distortion, when i lift my index the volume";
        let fr = [
            "I:map", "<route>", "S:hand.finger.thumb", "T:param:fx.drive", "</route>",
            "<route>", "S:hand.finger.index", "T:param:amp.gain", "</route>", "side:right",
        ];
        assert_eq!(run(t, &fr, &[sided(t, "thumb", "hand.finger.thumb", "right")]), fr);
    }

    #[test]
    fn folded_chip_goes_to_the_nearest_route_with_its_source() {
        let t = "map openness to the mix and left fist to reverb";
        let fr = [
            "I:map", "<route>", "S:hand.openness", "=openness", "T:param:fx.mix", "</route>",
            "<route>", "S:hand.openness", "=fist", "INV", "T:toggle:reverb", "</route>",
        ];
        let out = run(t, &fr, &[sided(t, "fist", "hand.fist", "left")]);
        assert_eq!(&out[..6], &fr[..6]);
        assert_eq!(&out[6..9], ["<route>", "S:hand.openness", "side:left"]);
    }

    #[test]
    fn fan_in_pins_each_source() {
        let t = "when I smile and move my left hand sideways the osc shape moves";
        let fr = ["I:map", "<route>", "S:face.smile", "S:hand.y", "side:left", "T:param:osc.shape", "</route>"];
        let pins = [gesture(t, "smile", "face.smile"), gesture(t, "move my left hand sideways", "hand.x")];
        let out = run(t, &fr, &pins);
        assert_eq!(out, ["I:map", "<route>", "S:face.smile", "S:hand.x", "side:left", "T:param:osc.shape", "</route>"]);
    }

    #[test]
    fn shape_chip_sets_shape_token() {
        let t = "peace sign toggles the delay";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:toggle:delay", "</route>"];
        let pins = [Pin {
            bytes: at(t, "peace sign"),
            what: PinWhat::Gesture { source: "hand.shape".into(), shape: Some("peace".into()), side: None },
        }];
        assert_eq!(run(t, &fr, &pins)[2..4], ["S:hand.shape", "shape:peace"]);
    }

    #[test]
    fn gesture_and_param_without_routes_share_one() {
        let t = "Pinch machen Filter 1 Freq";
        let pins = [
            gesture(t, "Pinch machen", "hand.pinch"),
            param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false),
        ];
        let out = run(t, &["I:none"], &pins);
        assert_eq!(out, ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "@p#205", "</route>"]);
    }

    #[test]
    fn fan_out_pins_each_target() {
        let t = "moving the object sideways drives the resonance and the cutoff";
        let fr = ["I:map", "<route>", "S:object.x", "T:param:filter.resonance", "T:param:filter.cutoff", "</route>"];
        let pins = [
            param(t, "resonance", "p#2", "filter.resonance", false),
            param(t, "cutoff", "p#1", "filter.cutoff", false),
        ];
        let out = run(t, &fr, &pins);
        assert_eq!(
            out,
            ["I:map", "<route>", "S:object.x", "T:param:filter.resonance", "@p#2", "T:param:filter.cutoff", "@p#1", "</route>"]
        );
    }

    #[test]
    fn fewer_gestures_than_routes_go_to_nearest() {
        let t = "Filter 1 Freq goes up and Reverb Mix follows my mouth";
        let fr = [
            "I:map", "<route>", "S:hand.y", "T:param:filter.cutoff", "PNAME", "=filter 1 freq", "</route>",
            "<route>", "S:hand.y", "T:param:fx.reverb.mix", "PNAME", "=reverb mix", "</route>",
        ];
        let out = run(t, &fr, &[gesture(t, "my mouth", "mouth.open")]);
        assert_eq!(out[2], "S:hand.y");
        assert_eq!(out[8], "S:mouth.open");
    }
}
