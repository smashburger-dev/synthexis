//! Pinned spans from the sentence field's chips, enforced on a decoded
//! frame. The user picked these parts while typing, so they win over what
//! the parser guessed:
//!
//! - a parameter chip goes to the route whose PNAME span covers it, else to
//!   the route whose target has the chip's class, else to the only route
//!   with a parameter or toggle target; that route's target becomes
//!   T:param:<class> (T:toggle for a switch) and carries the parameter id;
//! - a gesture chip goes to the route that already has its source; the
//!   others go in sentence order to the free routes when their counts are
//!   equal, else each to the free route nearest to it (by its spans and
//!   pinned parameter); the route's S: token becomes the chip's source (and
//!   shape:);
//! - a chip no route takes gets a route of its own.
//!
//! Runs after decoding, never inside the grammar: the parser still reads
//! the chips as plain words.

use crate::Item;

#[derive(Clone, Debug, PartialEq)]
pub enum PinWhat {
    /// A parameter of the picked plugin; a switch parameter is a toggle.
    Param { id: String, class: String, switch: bool },
    /// A gesture source, with a shape id when the source is hand.shape.
    Gesture { source: String, shape: Option<String> },
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
    /// Pinned parameter id.
    pub param: Option<String>,
    pub source_pinned: bool,
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

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

impl Route {
    fn new(items: Vec<Item>) -> Self {
        let mut r = Self { items, ..Self::default() };
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

    /// Byte ranges of the PNAME spans.
    fn pnames(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.items.windows(2).filter_map(|w| match (&w[0], &w[1]) {
            (Item::Token(t), Item::Span { bytes, .. }) if t == "PNAME" => Some(*bytes),
            _ => None,
        })
    }

    /// Index of the first T: token.
    fn target(&self) -> Option<usize> {
        self.items.iter().position(|it| tok(it).starts_with("T:"))
    }

    /// The target is a plugin parameter or a toggle (what a chip can pin).
    fn param_target(&self) -> Option<usize> {
        self.items
            .iter()
            .position(|it| tok(it).starts_with("T:param:") || tok(it).starts_with("T:toggle:"))
    }

    fn target_class(&self) -> Option<&str> {
        self.param_target().map(|i| {
            let t = tok(&self.items[i]);
            t.trim_start_matches("T:param:").trim_start_matches("T:toggle:")
        })
    }

    /// Position after the leading source tokens.
    fn after_source(&self) -> usize {
        self.items
            .iter()
            .take_while(|it| ["S:", "side:", "shape:", "object:"].iter().any(|p| tok(it).starts_with(p)))
            .count()
    }

    fn set_param(&mut self, id: &str, class: &str, switch: bool, at: (usize, usize)) {
        let old = self.param_target().or_else(|| self.target());
        let token = if switch {
            // keep the parser's effect family if it already chose a toggle
            match old.map(|i| tok(&self.items[i])) {
                Some(t) if t.starts_with("T:toggle:") => t.to_string(),
                _ => "T:toggle:".to_string(),
            }
        } else {
            format!("T:param:{class}")
        };
        match old {
            Some(i) => self.items[i] = Item::Token(token),
            None => {
                let i = self.after_source();
                self.items.insert(i, Item::Token(token));
            }
        }
        self.param = Some(id.to_string());
        self.widen(at);
    }

    fn set_source(&mut self, source: &str, shape: Option<&str>) {
        let s = Item::Token(format!("S:{source}"));
        match self.items.iter().position(|it| tok(it).starts_with("S:")) {
            Some(i) => self.items[i] = s,
            None => self.items.insert(0, s),
        }
        if let Some(sh) = shape {
            let t = Item::Token(format!("shape:{sh}"));
            match self.items.iter().position(|it| tok(it).starts_with("shape:")) {
                Some(i) => self.items[i] = t,
                None => {
                    let i = self.items.iter().position(|it| tok(it).starts_with("S:")).map_or(0, |i| i + 1);
                    self.items.insert(i, t);
                }
            }
        }
        self.source_pinned = true;
    }

    fn has_source(&self, source: &str, shape: Option<&str>) -> bool {
        let s = format!("S:{source}");
        self.items.iter().any(|it| tok(it) == s)
            && shape.is_none_or(|sh| self.items.iter().any(|it| tok(it) == format!("shape:{sh}")))
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
    /// as "@<id>" right after its route's target.
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
                    let at = r.param_target();
                    for (i, it) in r.items.iter().enumerate() {
                        out.push(item(it));
                        if Some(i) == at {
                            if let Some(id) = &r.param {
                                out.push(format!("@{id}"));
                            }
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

/// Enforces `pins` on the decoded `items` (see the module doc).
pub fn pin(items: &[Item], pins: &[Pin]) -> Frame {
    let mut f = Frame::from_items(items);
    let mut pins: Vec<&Pin> = pins.iter().collect();
    pins.sort_by_key(|p| p.bytes.0);
    let params = pins.iter().filter_map(|p| match &p.what {
        PinWhat::Param { id, class, switch } => Some((p.bytes, id, class, *switch)),
        PinWhat::Gesture { .. } => None,
    });
    let params: Vec<_> = params.collect();
    let mut placed = vec![false; params.len()];
    // 1. pointer cover, 2. class
    for rule in 0..2 {
        for (k, &(bytes, id, class, switch)) in params.iter().enumerate() {
            if placed[k] {
                continue;
            }
            let mut routes = f.routes_mut();
            let hit = routes.iter_mut().find(|r| {
                r.param.is_none()
                    && match rule {
                        0 => r.pnames().any(|b| overlaps(b, bytes)),
                        _ => r.target_class() == Some(class.as_str()),
                    }
            });
            if let Some(r) = hit {
                r.set_param(id, class, switch, bytes);
                placed[k] = true;
            }
        }
    }
    // 3. the only free parameter route takes the only unplaced chip
    if placed.iter().filter(|p| !**p).count() == 1 {
        let mut free: Vec<&mut Route> = f
            .routes_mut()
            .into_iter()
            .filter(|r| r.param.is_none() && r.param_target().is_some())
            .collect();
        if let ([r], Some(k)) = (free.as_mut_slice(), placed.iter().position(|p| !p)) {
            let (bytes, id, class, switch) = params[k];
            r.set_param(id, class, switch, bytes);
            placed[k] = true;
        }
    }
    for (k, &(bytes, id, class, switch)) in params.iter().enumerate() {
        if !placed[k] {
            let mut r = Route::default();
            r.set_param(id, class, switch, bytes);
            f.add_route(r);
        }
    }
    let mut gestures: Vec<((usize, usize), &str, Option<&str>)> = pins
        .iter()
        .filter_map(|p| match &p.what {
            PinWhat::Gesture { source, shape } => Some((p.bytes, source.as_str(), shape.as_deref())),
            PinWhat::Param { .. } => None,
        })
        .collect();
    // 1. the route that already has the source
    gestures.retain(|&(_, source, shape)| {
        let mut routes = f.routes_mut();
        match routes.iter_mut().find(|r| !r.source_pinned && r.has_source(source, shape)) {
            Some(r) => {
                r.set_source(source, shape);
                false
            }
            None => true,
        }
    });
    // 2. in order when the counts match, else nearest
    let mut free: Vec<&mut Route> = f.routes_mut().into_iter().filter(|r| !r.source_pinned).collect();
    let mut extra = Vec::new();
    if free.len() == gestures.len() {
        for (r, &(_, source, shape)) in free.iter_mut().zip(&gestures) {
            r.set_source(source, shape);
        }
    } else {
        for &(bytes, source, shape) in &gestures {
            let near = free
                .iter_mut()
                .filter(|r| !r.source_pinned)
                .min_by_key(|r| r.gap(bytes));
            match near {
                Some(r) => r.set_source(source, shape),
                None => extra.push((bytes, source, shape)),
            }
        }
    }
    // 3. a route of its own
    for (bytes, source, shape) in extra {
        let mut r = Route::default();
        r.set_source(source, shape);
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
        Pin { bytes: at(text, chip), what: PinWhat::Gesture { source: source.into(), shape: None } }
    }

    fn run(text: &str, frame: &[&str], pins: &[Pin]) -> Vec<String> {
        pin(&items(frame, text), pins).frame()
    }

    #[test]
    fn no_pins_keeps_frame() {
        let t = "when i hold still the width on pluck goes up";
        let fr = ["I:map", "<route>", "S:motion.hold", "T:param:stereo.width", "TRACK", "=pluck", "</route>"];
        assert_eq!(run(t, &fr, &[]), fr);
    }

    #[test]
    fn pname_cover_sets_param_and_kind() {
        let t = "when I make a fist the Filter 1 Freq goes up";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:fx.tone", "PNAME", "=filter 1 freq", "</route>"];
        let out = run(t, &fr, &[param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false)]);
        assert_eq!(
            out,
            ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "@p#205", "PNAME", "=filter 1 freq", "</route>"]
        );
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
        let ids: Vec<_> = f.routes().map(|r| r.param.clone()).collect();
        assert_eq!(ids, [Some("p#1".to_string()), Some("p#9".to_string())]);
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
    fn uncovered_chip_adds_route() {
        let t = "my fist drives the cutoff and also Reverb Mix";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "</route>"];
        let pins = [param(t, "Reverb Mix", "p#9", "fx.reverb.mix", false)];
        // one route, one chip, but its class differs: rule 3 takes it
        assert_eq!(pin(&items(&fr, t), &pins).routes().count(), 1);
        let pins = [
            param(t, "cutoff", "p#1", "filter.cutoff", false),
            param(t, "Reverb Mix", "p#9", "fx.reverb.mix", false),
        ];
        let f = pin(&items(&fr, t), &pins);
        let out = f.frame();
        assert_eq!(f.routes().count(), 2);
        assert_eq!(out[out.len() - 4..], ["<route>", "T:param:fx.reverb.mix", "@p#9", "</route>"]);
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
        let t = "the Filter 1 Freq follows my mouth and Reverb Mix follows a fist";
        let fr = [
            "I:map", "<route>", "S:hand.y", "T:param:filter.cutoff", "PNAME", "=filter 1 freq", "</route>",
            "<route>", "S:hand.y", "T:param:fx.reverb.mix", "PNAME", "=reverb mix", "</route>",
        ];
        let pins = [gesture(t, "a fist", "hand.fist"), gesture(t, "my mouth", "mouth.open")];
        let out = run(t, &fr, &pins);
        assert_eq!(out[2], "S:mouth.open");
        assert_eq!(out[8], "S:hand.fist");
    }

    #[test]
    fn gesture_already_right_stays() {
        let t = "fist on cutoff, mouth on reverb";
        let fr = [
            "I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "</route>",
            "<route>", "S:mouth.open", "T:param:fx.reverb.mix", "</route>",
        ];
        let out = run(t, &fr, &[gesture(t, "mouth", "mouth.open"), gesture(t, "fist", "hand.fist")]);
        assert_eq!(out, fr);
    }

    #[test]
    fn shape_chip_sets_shape_token() {
        let t = "peace sign toggles the delay";
        let fr = ["I:map", "<route>", "S:hand.fist", "T:toggle:delay", "</route>"];
        let pins = [Pin {
            bytes: at(t, "peace sign"),
            what: PinWhat::Gesture { source: "hand.shape".into(), shape: Some("peace".into()) },
        }];
        assert_eq!(run(t, &fr, &pins)[2..4], ["S:hand.shape", "shape:peace"]);
    }

    #[test]
    fn gesture_and_param_without_routes_share_one() {
        let t = "Faust schließen Filter 1 Freq";
        let pins = [
            gesture(t, "Faust schließen", "hand.fist"),
            param(t, "Filter 1 Freq", "p#205", "filter.cutoff", false),
        ];
        let out = run(t, &["I:none"], &pins);
        assert_eq!(out, ["I:map", "<route>", "S:hand.fist", "T:param:filter.cutoff", "@p#205", "</route>"]);
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
