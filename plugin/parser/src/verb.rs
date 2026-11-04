//! Which way a sentence moves a route's target ("pinch my fingers
//! decrease the filter": down). The parser sometimes leaves out INV for a
//! lowering verb (plugin log 2026-11-04: S:hand.pinch -> filter.cutoff
//! without INV); the resolver reads the verb from the sentence and sets it.
//!
//! A direction word that belongs to the movement ("open my fist", "Hand
//! runter", "close my eyes") is the source's, not the target's: it tells
//! whether the movement itself goes down (a closing fist, a lowered hand),
//! which turns a lowering verb back into a straight route. Among the
//! target's words the one nearest the target's name wins, preferring its
//! own clause ("the more I pinch, the less reverb": less).

/// A direction a sentence names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// Direction words: (word, direction, whether it belongs to a body noun
/// right before it, "Hand runter", "fist open"). German verbs after the
/// noun are the movement's ("wenn ich die Hand senke"); English verbs
/// after it are the target's ("my fingers decrease the filter").
const WORDS: [(&str, Dir, bool); 76] = [
    ("decrease", Dir::Down, false),
    ("decreases", Dir::Down, false),
    ("decreasing", Dir::Down, false),
    ("lower", Dir::Down, false),
    ("lowers", Dir::Down, false),
    ("lowering", Dir::Down, false),
    ("lowered", Dir::Down, true),
    ("reduce", Dir::Down, false),
    ("reduces", Dir::Down, false),
    ("reducing", Dir::Down, false),
    ("close", Dir::Down, true),
    ("closes", Dir::Down, true),
    ("closing", Dir::Down, true),
    ("closed", Dir::Down, true),
    ("shut", Dir::Down, true),
    ("down", Dir::Down, true),
    ("less", Dir::Down, false),
    ("drop", Dir::Down, false),
    ("drops", Dir::Down, false),
    ("cut", Dir::Down, false),
    ("cuts", Dir::Down, false),
    ("quieter", Dir::Down, false),
    ("increase", Dir::Up, false),
    ("increases", Dir::Up, false),
    ("increasing", Dir::Up, false),
    ("raise", Dir::Up, false),
    ("raises", Dir::Up, false),
    ("raising", Dir::Up, false),
    ("raised", Dir::Up, true),
    ("lift", Dir::Up, false),
    ("lifts", Dir::Up, false),
    ("open", Dir::Up, true),
    ("opens", Dir::Up, true),
    ("opening", Dir::Up, true),
    ("more", Dir::Up, false),
    ("up", Dir::Up, true),
    ("louder", Dir::Up, false),
    ("boost", Dir::Up, false),
    ("verringern", Dir::Down, true),
    ("verringere", Dir::Down, true),
    ("verringert", Dir::Down, true),
    ("senken", Dir::Down, true),
    ("senke", Dir::Down, true),
    ("senkt", Dir::Down, true),
    ("absenken", Dir::Down, true),
    ("absenke", Dir::Down, true),
    ("reduzieren", Dir::Down, true),
    ("reduziere", Dir::Down, true),
    ("reduziert", Dir::Down, true),
    ("runter", Dir::Down, true),
    ("herunter", Dir::Down, true),
    ("weniger", Dir::Down, true),
    ("schliessen", Dir::Down, true),
    ("schliesse", Dir::Down, true),
    ("schliesst", Dir::Down, true),
    ("zu", Dir::Down, true),
    ("leiser", Dir::Down, true),
    ("erhöhen", Dir::Up, true),
    ("erhöhe", Dir::Up, true),
    ("erhöht", Dir::Up, true),
    ("öffnen", Dir::Up, true),
    ("öffne", Dir::Up, true),
    ("öffnet", Dir::Up, true),
    ("auf", Dir::Up, true),
    ("mehr", Dir::Up, true),
    ("lauter", Dir::Up, true),
    ("hoch", Dir::Up, true),
    ("rauf", Dir::Up, true),
    ("heben", Dir::Up, true),
    ("hebe", Dir::Up, true),
    ("hebt", Dir::Up, true),
    ("anheben", Dir::Up, true),
    ("aufdrehen", Dir::Up, true),
    ("aufmachen", Dir::Up, true),
    ("zumachen", Dir::Down, true),
    ("dimm", Dir::Down, true),
];

/// Body nouns and the source prefix they move.
const BODY: [(&str, &str); 36] = [
    ("hand", "hand"),
    ("hands", "hand"),
    ("hände", "hand"),
    ("fist", "hand"),
    ("fists", "hand"),
    ("faust", "hand"),
    ("fäuste", "hand"),
    ("finger", "hand"),
    ("fingers", "hand"),
    ("fingern", "hand"),
    ("thumb", "hand"),
    ("daumen", "hand"),
    ("palm", "hand"),
    ("eye", "eye"),
    ("eyes", "eye"),
    ("auge", "eye"),
    ("augen", "eye"),
    ("eyelid", "eye"),
    ("eyelids", "eye"),
    ("mouth", "mouth"),
    ("mund", "mouth"),
    ("lips", "mouth"),
    ("lippen", "mouth"),
    ("jaw", "mouth"),
    ("head", "head"),
    ("kopf", "head"),
    ("brow", "brow"),
    ("brows", "brow"),
    ("eyebrow", "brow"),
    ("eyebrows", "brow"),
    ("augenbraue", "brow"),
    ("augenbrauen", "brow"),
    ("arm", "arm"),
    ("arms", "arm"),
    ("arme", "arm"),
    ("wrist", "hand"),
];

/// Words between a gesture verb and its body noun ("open my left fist").
const DETS: [&str; 22] = [
    "my", "your", "the", "a", "both", "left", "right", "his", "her", "die", "den", "der", "das", "mein", "meine",
    "meinen", "beide", "beiden", "linke", "linken", "rechte", "rechten",
];

/// Words that start a clause.
const CLAUSE: [&str; 14] =
    ["when", "whenever", "if", "while", "and", "then", "but", "wenn", "falls", "während", "sobald", "und", "dann", "aber"];

struct Word {
    text: String,
    bytes: (usize, usize),
    clause: usize,
    /// Punctuation or the end of the sentence follows.
    last: bool,
}

fn split(sentence: &str) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut clause = 0;
    let mut start: Option<usize> = None;
    let push = |out: &mut Vec<Word>, a: usize, b: usize, clause: &mut usize| {
        let text = sentence.get(a..b).unwrap_or("").to_lowercase().replace('ß', "ss");
        if CLAUSE.contains(&text.as_str()) && !out.is_empty() {
            *clause += 1;
        }
        out.push(Word { text, bytes: (a, b), clause: *clause, last: false });
    };
    for (i, c) in sentence.char_indices() {
        if c.is_alphanumeric() {
            start.get_or_insert(i);
            continue;
        }
        if let Some(a) = start.take() {
            push(&mut out, a, i, &mut clause);
        }
        if ",;.!?:".contains(c) {
            if let Some(w) = out.last_mut() {
                w.last = true;
            }
            clause += 1;
        }
    }
    if let Some(a) = start {
        push(&mut out, a, sentence.len(), &mut clause);
    }
    if let Some(w) = out.last_mut() {
        w.last = true;
    }
    out
}

fn entry(w: &str) -> Option<(Dir, bool)> {
    WORDS.iter().find(|e| e.0 == w).map(|e| (e.1, e.2))
}

fn body(w: &str) -> Option<&'static str> {
    BODY.iter().find(|b| b.0 == w).map(|b| b.1)
}

/// The body part the direction word at `i` moves, if it is the
/// movement's: a body noun after it ("open my fist") or, for words that
/// can follow their noun, right before it ("Hand runter").
fn attached(ws: &[Word], i: usize) -> Option<&'static str> {
    let w = ws.get(i)?;
    let rest = ws.get(i + 1..).unwrap_or(&[]);
    let after = rest.iter().take_while(|n| n.clause == w.clause).take(4).find(|n| !DETS.contains(&n.text.as_str()));
    if let Some(part) = after.and_then(|n| body(&n.text)) {
        return Some(part);
    }
    let before = i.checked_sub(1).and_then(|j| ws.get(j)).filter(|p| p.clause == w.clause)?;
    let follows = entry(&w.text).is_some_and(|e| e.1);
    body(&before.text).filter(|_| follows)
}

/// The direction of the target's words, or None when the sentence names
/// none or two that the target's name cannot decide between.
///
/// `src`: the SRC spans; `anchors`: byte ranges of the target's names;
/// `names`: words of the bound parameters' names ("Low Cut" is no cut).
pub fn target_dir(sentence: &str, src: &[(usize, usize)], anchors: &[(usize, usize)], names: &[String]) -> Option<Dir> {
    let ws = split(sentence);
    let inside = |b: (usize, usize), r: &(usize, usize)| r.0 <= b.0 && b.1 <= r.1;
    let next = |i: usize| ws.get(i + 1).filter(|n| n.clause == ws[i].clause).map(|n| n.text.as_str());
    let found: Vec<(usize, Dir)> = (0..ws.len())
        .filter_map(|i| {
            let w = &ws[i];
            let (d, _) = entry(&w.text)?;
            let skip = src.iter().any(|r| inside(w.bytes, r))
                // a word of the parameter's name where the name stands
                || (names.contains(&w.text) && (anchors.is_empty() || anchors.iter().any(|r| inside(w.bytes, r))))
                || attached(&ws, i).is_some()
                // a range ("up to 80 %") or a comparison, not a verb
                || (matches!(w.text.as_str(), "up" | "down") && matches!(next(i), Some("to" | "bis")))
                || (matches!(w.text.as_str(), "more" | "less" | "mehr" | "weniger") && matches!(next(i), Some("than" | "als")))
                // "zu" and "auf" end their clause ("Filter zu"), else they
                // are prepositions ("um den Filter zu erhöhen")
                || (matches!(w.text.as_str(), "zu" | "auf") && !w.last && next(i).is_some());
            (!skip).then_some((i, d))
        })
        .collect();
    let at: Vec<usize> = (0..ws.len()).filter(|&i| anchors.iter().any(|a| a.0 < ws[i].bytes.1 && ws[i].bytes.0 < a.1)).collect();
    if at.is_empty() {
        let first = found.first()?.1;
        return found.iter().all(|f| f.1 == first).then_some(first);
    }
    found
        .iter()
        .min_by_key(|(i, _)| {
            let near = at.iter().map(|&a| a.abs_diff(*i)).min().unwrap_or(usize::MAX);
            let own = at.iter().any(|&a| ws[a].clause == ws[*i].clause);
            (!own, near)
        })
        .map(|f| f.1)
}

/// True when the sentence says the route's movement in its falling
/// direction: closing the fist (hand openness), closing the eyes, a hand
/// going down. A lowering verb on such a movement is a straight route.
pub fn source_falls(sentence: &str, source: &str) -> bool {
    let ws = split(sentence);
    // hands.distance moves the hands
    let part = source.split('.').next().map(|p| p.trim_end_matches('s')).unwrap_or("");
    let mut moves: Vec<Dir> = (0..ws.len())
        .filter(|&i| attached(&ws, i) == Some(part).filter(|p| !p.is_empty()))
        .filter_map(|i| entry(&ws[i].text).map(|e| e.0))
        .collect();
    // a sideways movement falls to the left ("tilt my head to the left");
    // "left hand" names the side, not the way
    if source.ends_with(".x") || source.ends_with(".roll") || source.ends_with(".yaw") {
        let way = |i: usize| match ws[i].text.as_str() {
            "left" | "links" => Some(Dir::Down),
            "right" | "rechts" => Some(Dir::Up),
            _ => None,
        };
        let names_side = |i: usize| {
            ws.get(i + 1)
                .filter(|n| n.clause == ws[i].clause)
                .is_some_and(|n| body(&n.text).is_some() || matches!(n.text.as_str(), "side" | "seite" | "one"))
        };
        moves.extend((0..ws.len()).filter(|&i| !names_side(i)).filter_map(way));
    }
    // squeezing the fingers closes the hand ("Finger zusammendrücken")
    if part == "hand" && ws.iter().any(|w| ["zusammendrück", "zusammenkneif", "squeez"].iter().any(|p| w.text.starts_with(p))) {
        moves.push(Dir::Down);
    }
    // the pinch rises as the hand closes: closing fingers pinch, opening
    // them lets go
    if source == "hand.pinch" {
        moves = moves.into_iter().map(|d| if d == Dir::Down { Dir::Up } else { Dir::Down }).collect();
    }
    let down = moves.contains(&Dir::Down);
    let up = moves.contains(&Dir::Up);
    // "make a fist": the fist is the hand's openness falling
    let fist = source == "hand.openness" && ws.iter().any(|w| matches!(w.text.as_str(), "fist" | "fists" | "faust" | "fäuste"));
    (down || fist) && !up
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str, w: &str) -> (usize, usize) {
        let b = s.find(w).unwrap_or(0);
        (b, b + w.len())
    }

    #[test]
    fn the_target_verb_not_the_movement() {
        let s = "pinch my fingers decrease the filter";
        assert_eq!(target_dir(s, &[], &[at(s, "filter")], &[]), Some(Dir::Down));
        let s = "pinch to increase the filter";
        assert_eq!(target_dir(s, &[], &[at(s, "filter")], &[]), Some(Dir::Up));
        let s = "when I open my fist decrease the reverb";
        assert_eq!(target_dir(s, &[], &[at(s, "reverb")], &[]), Some(Dir::Down));
        assert!(!source_falls(s, "hand.openness"));
        let s = "wenn ich die Finger zusammendrücke, Filter runter";
        assert_eq!(target_dir(s, &[], &[at(s, "Filter")], &[]), Some(Dir::Down));
        let s = "raise my hand to lower the cutoff";
        assert_eq!(target_dir(s, &[], &[], &[]), Some(Dir::Down));
        assert!(!source_falls(s, "hand.y"));
    }

    #[test]
    fn ranges_prepositions_and_names_are_no_verbs() {
        let s = "fist steps the tone only up to 80 percent";
        assert_eq!(target_dir(s, &[], &[at(s, "tone")], &[]), None);
        let s = "pinch to cut the low cut";
        assert_eq!(target_dir(s, &[], &[at(s, "low cut")], &["low".into(), "cut".into()]), Some(Dir::Down));
        let s = "pinch more to raise the low cut";
        assert_eq!(target_dir(s, &[], &[at(s, "low cut")], &["low".into(), "cut".into()]), Some(Dir::Up));
        let s = "kneifen um den Filter zu erhöhen";
        assert_eq!(target_dir(s, &[], &[at(s, "Filter")], &[]), Some(Dir::Up));
        let s = "beim Kneifen Filter zu";
        assert_eq!(target_dir(s, &[], &[at(s, "Filter")], &[]), Some(Dir::Down));
    }

    #[test]
    fn the_nearest_verb_of_its_clause_wins() {
        let s = "the more I pinch, the less reverb";
        assert_eq!(target_dir(s, &[], &[at(s, "reverb")], &[]), Some(Dir::Down));
        // without a name two directions decide nothing
        assert_eq!(target_dir(s, &[], &[], &[]), None);
        let s = "decrease the reverb when I open my fist";
        assert_eq!(target_dir(s, &[], &[at(s, "reverb")], &[]), Some(Dir::Down));
    }

    #[test]
    fn falling_movements() {
        assert!(source_falls("when I close my eyes, close the filter", "eye.open"));
        assert!(source_falls("make a fist to lower the drive", "hand.openness"));
        assert!(source_falls("wenn ich die Hand senke, Filter runter", "hand.y"));
        assert!(!source_falls("wenn ich die Hand hebe, Filter runter", "hand.y"));
        assert!(!source_falls("pinch my fingers decrease the filter", "hand.pinch"));
        assert!(source_falls("tilt my head to the left to lower the cutoff", "head.roll"));
        assert!(!source_falls("left hand moves right to lower the cutoff", "hand.x"));
        assert!(!source_falls("left hand pinch lowers the cutoff", "hand.pinch"));
        // squeezed fingers close the spread and raise the pinch
        let s = "wenn ich die Finger zusammendrücke, Filter runter";
        assert!(source_falls(s, "hand.spread"));
        assert!(!source_falls(s, "hand.pinch"));
        assert!(source_falls("open my fingers to lower the filter", "hand.pinch"));
        // the other route's eyes are not the hand's
        assert!(!source_falls("close my eyes and pinch to lower the drive", "hand.pinch"));
    }
}
