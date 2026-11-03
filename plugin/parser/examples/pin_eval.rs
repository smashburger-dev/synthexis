//! Pins given chips on dumped frames (probes/pin-hill): reads rows
//! {pred, pred_bytes, pins} from stdin, writes {frame} per row.
//! A pin: [start byte, end byte, "g", source, shape, side] for a gesture,
//! [start, end, "p", class, switch] for a parameter, [start, end, "t", kind]
//! for a target that is no parameter.
//!
//!   pin_eval < rows.jsonl > out.jsonl

use std::io::{BufRead, Write};

use serde_json::{json, Value};
use synthexis_parser::{pin, Item, Pin, PinWhat};

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn main() {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(r) = serde_json::from_str::<Value>(&line) else {
            eprintln!("bad row");
            std::process::exit(1);
        };
        let empty = Vec::new();
        let pred = r["pred"].as_array().unwrap_or(&empty);
        let bytes = r["pred_bytes"].as_array().unwrap_or(&empty);
        let items: Vec<Item> = pred
            .iter()
            .zip(bytes)
            .map(|(t, b)| {
                let t = t.as_str().unwrap_or_default();
                match (t.strip_prefix('='), b.as_array()) {
                    (Some(text), Some(b)) => Item::Span {
                        text: text.to_string(),
                        bytes: (b[0].as_u64().unwrap_or(0) as usize, b[1].as_u64().unwrap_or(0) as usize),
                    },
                    _ => Item::Token(t.to_string()),
                }
            })
            .collect();
        let pins: Vec<Pin> = r["pins"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let bytes = (p[0].as_u64()? as usize, p[1].as_u64()? as usize);
                let what = match p[2].as_str()? {
                    "g" => PinWhat::Gesture { source: s(&p[3])?, shape: s(&p[4]), side: s(&p[5]) },
                    "p" => PinWhat::Param { id: format!("p{k}"), class: s(&p[3])?, switch: p[4].as_bool().unwrap_or(false) },
                    "t" => PinWhat::Target { kind: s(&p[3])? },
                    _ => return None,
                };
                Some(Pin { bytes, what })
            })
            .collect();
        let frame: Vec<String> = pin(&items, &pins).frame().into_iter().filter(|t| !t.starts_with('@')).collect();
        if writeln!(out, "{}", json!({ "frame": frame })).is_err() {
            std::process::exit(1);
        }
    }
}
