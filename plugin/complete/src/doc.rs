//! The sentence field's content: plain text plus chips over byte ranges of
//! it. The text edit widget owns cursor and typing; after each change the
//! field hands the new text to `update`, which moves the chips along. A
//! deletion that touches a chip removes the whole chip, so a chip is never
//! half a parameter name.

use std::ops::Range;

use crate::{Chip, Suggestion};

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub range: Range<usize>,
    pub chip: Chip,
}

#[derive(Clone, Debug, Default)]
pub struct ChipText {
    pub text: String,
    /// Sorted by start, never overlapping.
    pub chips: Vec<Placed>,
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((i, _), _)| i)
}

fn common_suffix(a: &str, b: &str, max: usize) -> usize {
    let mut n = 0;
    for (x, y) in a.chars().rev().zip(b.chars().rev()) {
        if x != y || n + x.len_utf8() > max {
            break;
        }
        n += x.len_utf8();
    }
    n
}

impl ChipText {
    /// Takes the widget's text after an edit. Returns the cursor byte to
    /// set when it removed a whole chip (the widget's cursor is stale then).
    pub fn update(&mut self, new: &str) -> Option<usize> {
        if new == self.text {
            return None;
        }
        let old = std::mem::take(&mut self.text);
        let p = common_prefix(&old, new);
        let s = common_suffix(&old, new, old.len().min(new.len()) - p);
        let (oe, ne) = (old.len() - s, new.len() - s);
        let touched = |c: &Placed| {
            if oe == p {
                c.range.start < p && c.range.end > p // insertion inside the chip
            } else {
                c.range.start < oe && c.range.end > p
            }
        };
        if ne == p && self.chips.iter().any(touched) {
            let lo = self.chips.iter().filter(|c| touched(c)).map(|c| c.range.start).fold(p, usize::min);
            let hi = self.chips.iter().filter(|c| touched(c)).map(|c| c.range.end).fold(oe, usize::max);
            self.chips.retain(|c| !touched(c));
            self.shift(hi, lo as isize - hi as isize);
            self.text = format!("{}{}", &old[..lo], &old[hi..]);
            return Some(lo);
        }
        self.chips.retain(|c| !touched(c));
        self.shift(oe, ne as isize - oe as isize);
        self.text = new.to_string();
        None
    }

    fn shift(&mut self, from: usize, by: isize) {
        for c in self.chips.iter_mut().filter(|c| c.range.start >= from) {
            c.range.start = c.range.start.saturating_add_signed(by);
            c.range.end = c.range.end.saturating_add_signed(by);
        }
    }

    /// Puts the suggestion's chip over the typed text it replaces, followed
    /// by a space; returns the cursor byte after that space.
    pub fn accept(&mut self, s: &Suggestion) -> usize {
        let r = s.replace.clone();
        let label = s.chip.label();
        let rest = &self.text[r.end..];
        let space = if rest.starts_with(' ') { "" } else { " " };
        let text = format!("{}{label}{space}{rest}", &self.text[..r.start]);
        self.chips.retain(|c| c.range.end <= r.start || c.range.start >= r.end);
        let delta = (label.len() + space.len()) as isize - r.len() as isize;
        self.shift(r.end, delta);
        self.text = text;
        let at = self.chips.partition_point(|c| c.range.start < r.start);
        self.chips.insert(at, Placed { range: r.start..r.start + label.len(), chip: s.chip.clone() });
        r.start + label.len() + 1
    }

    /// End of the last chip that ends at or before `cursor`.
    pub fn floor(&self, cursor: usize) -> usize {
        self.chips
            .iter()
            .map(|c| c.range.end)
            .filter(|&e| e <= cursor)
            .max()
            .unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.chips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gesture(phrase: &str) -> Chip {
        Chip::Gesture { source: "hand.fist".into(), shape: None, phrase: phrase.into(), pol: 1 }
    }

    fn accept(d: &mut ChipText, typed: &str, phrase: &str) -> usize {
        let start = d.text.rfind(typed).unwrap_or(d.text.len());
        let s = Suggestion { chip: gesture(phrase), score: 1.0, replace: start..start + typed.len(), via: None, prefix: true, weak: false };
        d.accept(&s)
    }

    #[test]
    fn accept_places_chip_and_space() {
        let mut d = ChipText::default();
        d.update("wenn ich Fau");
        let cur = accept(&mut d, "Fau", "Faust schließen");
        assert_eq!(d.text, "wenn ich Faust schließen ");
        assert_eq!(cur, d.text.len());
        assert_eq!(&d.text[d.chips[0].range.clone()], "Faust schließen");
    }

    #[test]
    fn typing_after_and_before_moves_chip() {
        let mut d = ChipText::default();
        d.update("wenn ich Fau");
        accept(&mut d, "Fau", "Faust");
        assert_eq!(d.update("wenn ich Faust geht"), None);
        assert_eq!(d.update("oh wenn ich Faust geht"), None);
        assert_eq!(&d.text[d.chips[0].range.clone()], "Faust");
    }

    #[test]
    fn backspace_into_chip_removes_it() {
        let mut d = ChipText::default();
        d.update("wenn ich Fau");
        accept(&mut d, "Fau", "Faust");
        assert_eq!(d.update("wenn ich Faust"), None); // space removed
        assert_eq!(d.chips.len(), 1);
        assert_eq!(d.update("wenn ich Faus"), Some(9));
        assert_eq!(d.text, "wenn ich ");
        assert!(d.chips.is_empty());
    }

    #[test]
    fn typing_inside_chip_turns_it_into_text() {
        let mut d = ChipText::default();
        d.update("x Fau");
        accept(&mut d, "Fau", "Faust");
        assert_eq!(d.update("x Fa-ust "), None);
        assert!(d.chips.is_empty());
        assert_eq!(d.text, "x Fa-ust ");
    }

    #[test]
    fn floor_is_last_chip_end() {
        let mut d = ChipText::default();
        d.update("x Fau");
        accept(&mut d, "Fau", "Faust");
        assert_eq!(d.floor(d.text.len()), 7);
        assert_eq!(d.floor(3), 0);
    }
}
