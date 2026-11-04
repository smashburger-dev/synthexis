//! Face and head gestures on the shipped vocabulary
//! (plugin/assistant/completion/vocab.json): what a German or English user
//! types in a gesture position offers the right source as the top
//! gesture, and an eye takes the side the sentence names before it.

use std::path::Path;

use synthexis_complete::{data, Chip, Engine, Lang};

fn engine(lang: Lang) -> Result<Engine, String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assistant/completion");
    let (mut e, _) = data::load(&dir)?;
    e.set_lang(Some(lang));
    Ok(e)
}

/// Source of the best gesture item for `text` typed up to its end.
fn top_gesture(e: &Engine, text: &str) -> Option<(String, String)> {
    e.complete(text, text.len(), 0, 12).into_iter().find_map(|s| match s.chip {
        Chip::Gesture { source, phrase, .. } => Some((source, phrase)),
        _ => None,
    })
}

const DE: &[(&str, &str)] = &[
    ("wenn ich den Mund auf", "mouth.open"),
    ("wenn ich den Mund öff", "mouth.open"),
    ("Cutoff hoch, wenn ich den Mund zum", "mouth.open"),
    ("wenn ich blinz", "eye.closed"),
    ("wenn ich zwink", "eye.closed"),
    ("wenn ich die Augen schl", "eye.closed"),
    ("wenn ich den Kopf nach rechts dre", "head.yaw"),
    ("wenn ich den Kopf dre", "head.yaw"),
    ("wenn ich nick", "head.pitch"),
    ("wenn ich den Kopf neig", "head.roll"),
    ("wenn ich den Kopf schief", "head.roll"),
    ("wenn ich läch", "face.smile"),
    ("wenn ich links zwink", "eye.closed"),
    ("wenn ich den Kopf nach links dre", "head.yaw"),
    // neigen is the head to a shoulder, drehen up, down, left, right
    ("wenn ich den Kopf nach links nei", "head.roll"),
    ("wenn ich den Kopf nach rechts nei", "head.roll"),
    ("wenn ich den Kopf zur linken Schul", "head.roll"),
    ("wenn ich den Kopf nach oben dre", "head.pitch"),
];

const EN: &[(&str, &str)] = &[
    ("when I open my mou", "mouth.open"),
    ("when I close my mou", "mouth.open"),
    ("when I drop my ja", "mouth.open"),
    ("when I blin", "eye.closed"),
    ("when I win", "eye.closed"),
    ("when I close my ey", "eye.closed"),
    ("when I turn my he", "head.yaw"),
    ("when I shake my he", "head.yaw"),
    ("when I nod", "head.pitch"),
    ("when I look u", "head.pitch"),
    ("when I tilt my he", "head.roll"),
    ("when I smi", "face.smile"),
    ("when I turn my head to the ri", "head.yaw"),
    // tilt is the head to a shoulder, turn up, down, left, right
    ("when I tilt my head to the le", "head.roll"),
    ("when I tilt my head to the ri", "head.roll"),
    ("when I tilt my head to my sh", "head.roll"),
    ("when I turn my head le", "head.yaw"),
    ("when I turn my head u", "head.pitch"),
    ("when I turn my head do", "head.pitch"),
];

fn check(lang: Lang, cases: &[(&str, &str)]) -> Result<(), String> {
    let e = engine(lang)?;
    let mut misses = Vec::new();
    for (text, want) in cases {
        match top_gesture(&e, text) {
            Some((src, _)) if src == *want => {}
            got => misses.push(format!("{text:?}: want {want}, got {got:?}")),
        }
    }
    assert!(misses.is_empty(), "{lang:?}:\n{}", misses.join("\n"));
    Ok(())
}

#[test]
fn german_head_gestures() -> Result<(), String> {
    check(Lang::De, DE)
}

#[test]
fn english_head_gestures() -> Result<(), String> {
    check(Lang::En, EN)
}

#[test]
fn an_eye_takes_the_side_before_it() -> Result<(), String> {
    for (lang, text, side) in [
        (Lang::De, "wenn ich das linke Auge schl", 0),
        (Lang::De, "wenn ich das rechte Auge schl", 1),
        (Lang::De, "wenn ich links zwink", 0),
        (Lang::En, "when I close my left ey", 0),
        (Lang::En, "when I close my right ey", 1),
    ] {
        let e = engine(lang)?;
        let s = e.complete(text, text.len(), 0, 12);
        let eye = s.iter().find(|s| matches!(&s.chip, Chip::Gesture { source, .. } if source.starts_with("eye.")));
        assert!(eye.is_some(), "{text:?}: no eye gesture");
        if let Some(eye) = eye {
            assert!(eye.variants.sided, "{text:?}: the eye needs a side");
            assert_eq!(eye.variants.side, Some(side), "{text:?}");
        }
    }
    Ok(())
}
