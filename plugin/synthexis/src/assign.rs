//! Resolved routes (synthexis_parser::resolve) -> engine routes (route.rs).
//!
//! Source: `cam0.` + the source id, with L or R after its first segment for
//! a sided source (signal.rs); a sided source without a side gets no route
//! (Noa, 11-03: no default hand), "both" gets one per hand. Target: the
//! bound parameter of the picked plugin, or a Signal macro for
//! `T:macro:N`. Direction, range, amount, curve, smoothing and gates go
//! into the route; what the engine cannot run yet (steps, toggles, holds,
//! ramps, shapes, motions) is named in the note and left out.

use synthexis_parser::resolve;

use crate::cond::Curve;
use crate::fl;
use crate::route::{Route, Target, NUM_MACROS};
use crate::signal::perception_ids;

/// Engine signal ids for a source and side; Err names why there is none.
pub fn signals(source: &str, side: Option<&str>) -> Result<Vec<String>, String> {
    let ids: Vec<&str> = perception_ids().collect();
    let (head, rest) = source.split_once('.').unwrap_or((source, ""));
    let sided = |s: &str| format!("cam0.{head}.{s}.{rest}");
    if !rest.is_empty() && ids.contains(&sided("L").as_str()) {
        return match side {
            Some("left") => Ok(vec![sided("L")]),
            Some("right") => Ok(vec![sided("R")]),
            Some("both") => Ok(vec![sided("L"), sided("R")]),
            _ => Err("which hand? say left or right".into()),
        };
    }
    let id = format!("cam0.{source}");
    if ids.contains(&id.as_str()) {
        Ok(vec![id])
    } else {
        Err(format!("{} is not live yet", source.replace('.', " ")))
    }
}

/// What a resolved route becomes: engine routes and a note for the line
/// ("" when everything runs).
pub fn routes(r: &resolve::Route, plugin: Option<&fl::Key>) -> (Vec<Route>, String) {
    let mut notes: Vec<String> = Vec::new();
    let sigs = match signals(&r.source, r.side.as_deref()) {
        Ok(s) => s,
        Err(e) => return (Vec::new(), e),
    };
    let mut gate = None;
    for g in &r.gates {
        let src = g[0].trim_start_matches("G:");
        let side = g.iter().find_map(|t| t.strip_prefix("side:"));
        match signals(src, side.or(r.side.as_deref())) {
            Ok(s) if g.iter().all(|t| !t.starts_with("shape:")) => gate = s.into_iter().next(),
            _ => notes.push(format!("gate {} not live yet", src.replace('.', " "))),
        }
    }
    let targets: Vec<Target> = r
        .targets
        .iter()
        .filter_map(|t| match (&t.bound, plugin) {
            (Some(b), Some(k)) => Some(Target::Param { plugin: k.clone(), index: b.index }),
            _ => t
                .token
                .strip_prefix("T:macro:")
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| (1..=NUM_MACROS).contains(n))
                .map(|n| Target::Macro(n - 1)),
        })
        .collect();
    if targets.is_empty() {
        return (Vec::new(), if notes.is_empty() { "no parameter to drive".into() } else { notes.join(", ") });
    }
    let get = |p: &str| r.mods.iter().find_map(|m| m.strip_prefix(p)).and_then(|v| v.parse::<f32>().ok());
    let skipped: Vec<&str> = r
        .mods
        .iter()
        .filter(|m| ["mode:", "hold:", "quantize:", "delta:", "BIP"].iter().any(|p| m.starts_with(p)))
        .map(String::as_str)
        .collect();
    if !skipped.is_empty() {
        notes.push(format!("{} not live yet", skipped.join(" ")));
    }
    let mut out = Vec::new();
    for sig in &sigs {
        for t in &targets {
            let mut route = Route::to(sig, t.clone(), get("amt:").unwrap_or(1.0));
            route.lo = get("lo:").unwrap_or(0.0);
            route.hi = get("hi:").unwrap_or(1.0);
            route.gate.clone_from(&gate);
            route.cond.range.invert = r.inv;
            route.cond.range.curve = match r.mods.iter().find_map(|m| m.strip_prefix("curve:")) {
                Some("exp") => Curve::Exp,
                Some("log") => Curve::Log,
                Some("s") => Curve::S,
                _ => Curve::Linear,
            };
            // smoothing: the One Euro filter's cutoff, higher is snappier
            route.cond.filter.min_cutoff *= match r.mods.iter().find_map(|m| m.strip_prefix("smooth:")) {
                Some("low") => 3.0,
                Some("high") => 1.0 / 3.0,
                _ => 1.0,
            };
            out.push(route);
        }
    }
    (out, notes.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(source: &str, side: Option<&str>, mods: &[&str], gates: Vec<Vec<String>>, bound: bool) -> resolve::Route {
        resolve::Route {
            source: source.into(),
            side: side.map(str::to_string),
            inv: true,
            gates,
            mods: mods.iter().map(|m| m.to_string()).collect(),
            targets: vec![resolve::Target {
                token: "T:param:fx.drive".into(),
                text: "drive".into(),
                bound: bound.then(|| resolve::Bound { index: 12, name: "Amp Drive".into(), via: resolve::Via::Name }),
            }],
        }
    }

    #[test]
    fn left_fist_drives_topos() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        let gate = vec![vec!["G:hand.pinch".to_string(), "side:right".to_string()]];
        let (rs, note) = routes(&resolved("hand.openness", Some("left"), &["hi:0.8", "smooth:low"], gate, true), Some(&key));
        assert_eq!(note, "");
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].signal, "cam0.hand.L.openness");
        assert_eq!(rs[0].target, Target::Param { plugin: key, index: 12 });
        assert!(rs[0].cond.range.invert);
        assert_eq!((rs[0].lo, rs[0].hi), (0.0, 0.8));
        assert_eq!(rs[0].gate.as_deref(), Some("cam0.hand.R.pinch"));
    }

    #[test]
    fn no_side_no_route_and_unsided_sources_work() {
        let (rs, note) = routes(&resolved("hand.openness", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        let (rs, _) = routes(&resolved("hand.y", Some("both"), &[], vec![], true), Some(&fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() }));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y", "cam0.hand.R.y"]);
        let (rs, note) = routes(&resolved("mouth.open", None, &["mode:step"], vec![], false), None);
        assert!(rs.is_empty() && note == "no parameter to drive", "{note}");
        let mut macro_route = resolved("mouth.open", None, &["mode:step"], vec![], false);
        macro_route.targets[0].token = "T:macro:3".into();
        let (rs, note) = routes(&macro_route, None);
        assert_eq!(rs[0].target, Target::Macro(2));
        assert_eq!(note, "mode:step not live yet");
        let (rs, note) = routes(&resolved("motion.circle", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note == "motion circle is not live yet", "{note}");
    }
}
