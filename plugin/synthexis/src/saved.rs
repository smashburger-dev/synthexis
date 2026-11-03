//! Kept routes saved with the project: the sentence field's kept lines as
//! JSON in the plugin state (params `routes`, persisted like the picker
//! pin), so FL stores them in the project. initialize() runs them again
//! before any window opens; the field shows them when it opens. A line
//! that lost part of its routes ("partly removed") keeps only the rest.

use serde::{Deserialize, Serialize};
use synthexis_parser::resolve;

use crate::assign;
use crate::fl;
use crate::log::log;
use crate::route::Target;
use crate::writer::Writer;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// The plugin the sentence was resolved on (fl::Key id).
    pub key: Option<String>,
    pub route: resolve::Route,
    pub line: u64,
    pub batch: u64,
    pub full: usize,
    /// The line lost part of its routes.
    #[serde(default)]
    pub partly: bool,
    pub note: String,
    /// The engine routes the line still has, as (signal, target_id).
    pub keys: Vec<(String, String)>,
}

/// "macro:3" or "param:fx:0:0:Topos#12".
pub fn target_id(t: &Target) -> String {
    match t {
        Target::Macro(i) => format!("macro:{}", i + 1),
        Target::Param { plugin, index } => format!("param:{}#{index}", plugin.id()),
    }
}

pub fn load(json: &str) -> Vec<Saved> {
    if json.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(json).unwrap_or_else(|e| {
        log(&format!("saved routes: not read ({e})"));
        Vec::new()
    })
}

pub fn store(lines: &[Saved]) -> String {
    serde_json::to_string(lines).unwrap_or_default()
}

/// Runs the saved lines as kept routes, each line once (initialize() may
/// run more than once).
pub fn run(saved: &[Saved], writer: &Writer) {
    let mut n = 0;
    for s in saved {
        if writer.has_line(s.line) {
            continue;
        }
        let key = s.key.as_deref().and_then(fl::Key::parse);
        let (routes, _) = assign::routes(&s.route, key.as_ref());
        let routes: Vec<_> = routes
            .into_iter()
            .filter(|r| s.keys.contains(&(r.signal.clone(), target_id(&r.target))))
            .map(|mut r| {
                r.line = s.line;
                r
            })
            .collect();
        n += routes.len();
        let ids = writer.propose(routes);
        writer.accept(&ids);
    }
    if !saved.is_empty() {
        log(&format!("saved routes: {} line(s), {n} route(s) running", saved.len()));
    }
}
