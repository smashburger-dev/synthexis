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
        Target::Part { id, trigger } => match trigger {
            crate::route::Trigger::Above => format!("part:{id}"),
            crate::route::Trigger::Shape { mask, care } => format!("part:{id}:shape:{mask}:{care}"),
        },
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

/// Moves a line from plugin `from` to `to` (the plugin moved in the
/// project, a preset loaded elsewhere): the picked plugin, every chip
/// bound to `from` and the engine routes it still has, so `routes` builds
/// the same routes at the new place.
pub fn rebind(s: &mut Saved, from: &fl::Key, to: &fl::Key) {
    let (old, new) = (from.id(), to.id());
    if s.key.as_deref() == Some(old.as_str()) {
        s.key = Some(new.clone());
    }
    for b in s.route.targets.iter_mut().filter_map(|t| t.bound.as_mut()) {
        if b.plugin.as_deref() == Some(old.as_str()) {
            b.plugin = Some(new.clone());
        }
    }
    for (_, t) in &mut s.keys {
        // "param:<plugin id>#<index>"; the plugin name may hold a '#'
        let moved = t.strip_prefix("param:").and_then(|r| r.rsplit_once('#')).filter(|(id, _)| *id == old).map(|(_, i)| i.to_string());
        if let Some(index) = moved {
            *t = format!("param:{new}#{index}");
        }
    }
}

/// The engine routes a saved line runs: its resolved route bound again,
/// only those it still had.
fn routes(s: &Saved) -> Vec<crate::route::Route> {
    let key = s.key.as_deref().and_then(fl::Key::parse);
    let (routes, _) = assign::routes(&s.route, key.as_ref());
    routes
        .into_iter()
        .filter(|r| s.keys.contains(&(r.signal.clone(), target_id(&r.target))))
        .map(|mut r| {
            r.line = s.line;
            r
        })
        .collect()
}

/// Runs the saved lines as kept routes, each line once (initialize() may
/// run more than once).
pub fn run(saved: &[Saved], writer: &Writer) {
    let mut n = 0;
    for s in saved {
        if writer.has_line(s.line) {
            continue;
        }
        let routes = routes(s);
        n += routes.len();
        let ids = writer.propose(routes);
        writer.accept(&ids);
    }
    if !saved.is_empty() {
        log(&format!("saved routes: {} line(s), {n} route(s) running", saved.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topos(track: u32, slot: u32) -> fl::Key {
        fl::Key { at: fl::At::Fx { track, slot }, plugin: "Topos".into() }
    }

    /// "left hand openness drives the drive" on Topos, plus a chip of
    /// Topos's parameter 3 picked while the picker showed Topos too.
    fn line(key: &fl::Key) -> Saved {
        let target = |token: &str, index: u32, plugin: Option<String>| resolve::Target {
            token: token.into(),
            text: String::new(),
            bound: Some(resolve::Bound { index, name: format!("#{index}"), via: resolve::Via::Name, plugin }),
        };
        let route = resolve::Route {
            source: "hand.openness".into(),
            side: Some("left".into()),
            inv: false,
            gates: Vec::new(),
            mods: Vec::new(),
            targets: vec![target("T:param:fx.drive", 12, None), target("T:param:fx.mix", 3, Some(key.id()))],
        };
        let mut s = Saved { key: Some(key.id()), route, line: 4, batch: 2, full: 2, partly: false, note: String::new(), keys: Vec::new() };
        // what the field saves: every engine route the line had
        let (all, _) = assign::routes(&s.route, Some(key));
        s.keys = all.iter().map(|r| (r.signal.clone(), target_id(&r.target))).collect();
        s
    }

    #[test]
    fn a_rebound_line_builds_the_same_routes_at_the_new_place() {
        let (from, to) = (topos(0, 0), topos(3, 1));
        let mut s = line(&from);
        let before = routes(&s);
        assert_eq!(before.len(), 2);
        rebind(&mut s, &from, &to);
        let after = routes(&s);
        assert_eq!(after.len(), before.len());
        assert!(after.iter().all(|r| matches!(&r.target, Target::Param { plugin, .. } if *plugin == to)));
        assert_eq!(s.key, Some(to.id()));
        assert!(s.keys.iter().all(|(_, t)| t.starts_with("param:fx:3:1:Topos#")));
    }

    #[test]
    fn rebind_leaves_other_plugins_alone() {
        let (from, other) = (topos(0, 0), topos(5, 0));
        let mut s = line(&from);
        let kept = s.clone();
        rebind(&mut s, &other, &topos(6, 0));
        assert_eq!(s, kept);
    }
}
