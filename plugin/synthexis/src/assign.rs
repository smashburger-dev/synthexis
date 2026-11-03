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
use crate::i18n::{fill, Text, EN};
use crate::route::{Route, Target, NUM_MACROS};
use crate::signal::perception_ids;

/// Engine signal ids for a source and side; Err names why there is none,
/// in the language of `t`.
pub fn signals_in(source: &str, side: Option<&str>, t: &Text) -> Result<Vec<String>, String> {
    if source.is_empty() {
        return Err(t.n_which_move.into());
    }
    let ids: Vec<&str> = perception_ids().collect();
    let (head, rest) = source.split_once('.').unwrap_or((source, ""));
    let sided = |s: &str| format!("cam0.{head}.{s}.{rest}");
    if !rest.is_empty() && ids.contains(&sided("L").as_str()) {
        return match side {
            Some("left") => Ok(vec![sided("L")]),
            Some("right") => Ok(vec![sided("R")]),
            Some("both") => Ok(vec![sided("L"), sided("R")]),
            _ if head == "eye" => Err(t.n_which_eye.into()),
            _ => Err(t.n_which_hand.into()),
        };
    }
    let id = format!("cam0.{source}");
    if ids.contains(&id.as_str()) {
        Ok(vec![id])
    } else {
        Err(fill(t.n_not_live, &[&source.replace('.', " ")]))
    }
}

/// The targets of a resolved route, whatever its source: a bound parameter
/// (of the chip's own plugin when the chip came from another one), or a
/// Signal macro for `T:macro:N`.
pub fn targets(r: &resolve::Route, plugin: Option<&fl::Key>) -> Vec<Target> {
    r.targets
        .iter()
        .filter_map(|t| match &t.bound {
            Some(b) => {
                let key = b.plugin.as_deref().and_then(fl::Key::parse).or_else(|| plugin.cloned())?;
                Some(Target::Param { plugin: key, index: b.index })
            }
            None => t
                .token
                .strip_prefix("T:macro:")
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| (1..=NUM_MACROS).contains(n))
                .map(|n| Target::Macro(n - 1)),
        })
        .collect()
}

/// The gate signals for a route whose source signal is `sig`. A gate's
/// side: its own, else the route's side when the gate is the same body part
/// as the source ("left hand y while I pinch": the left hand pinches), else
/// the sentence's side when the source has none ("with my right hand: mouth
/// opens the mix while I pinch"); an eye never lends its side to a hand.
/// Err when a gate cannot run, then the route must not run either.
fn gate_signals(r: &resolve::Route, sig: &str, t: &Text) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = sig.split('.').collect();
    let sided = parts.get(2).is_some_and(|h| *h == "L" || *h == "R");
    r.gates
        .iter()
        .map(|g| {
            let src = g[0].trim_start_matches("G:");
            let named = src.replace('.', " ");
            if g.iter().any(|t| t.starts_with("shape:")) {
                return Err(fill(t.n_gate_shape, &[&named]));
            }
            let lent = match (sided, parts.get(1), parts.get(2)) {
                (true, Some(body), Some(h)) if src.split('.').next() == Some(*body) => {
                    Some(if *h == "L" { "left" } else { "right" })
                }
                (false, ..) => r.side.as_deref(),
                _ => None,
            };
            let side = g.iter().find_map(|t| t.strip_prefix("side:")).or(lent);
            match side {
                // "both hands pinch": both must be open
                Some("both") => signals_in(src, Some("both"), t).map_err(|e| fill(t.n_gate, &[&named, &e])),
                s => signals_in(src, s, t).map_err(|e| fill(t.n_gate, &[&named, &e])),
            }
        })
        .collect::<Result<Vec<Vec<String>>, String>>()
        .map(|v| v.into_iter().flatten().collect())
}

/// What a resolved route becomes: engine routes and a note for the line
/// ("" when everything runs).
pub fn routes(r: &resolve::Route, plugin: Option<&fl::Key>) -> (Vec<Route>, String) {
    routes_in(r, plugin, &EN)
}

/// `routes` with the note in the language of `t`.
pub fn routes_in(r: &resolve::Route, plugin: Option<&fl::Key>, t: &Text) -> (Vec<Route>, String) {
    let sigs = match signals_in(&r.source, r.side.as_deref(), t) {
        Ok(s) => s,
        Err(e) => return (Vec::new(), e),
    };
    let targets = targets(r, plugin);
    if targets.is_empty() {
        return (Vec::new(), t.n_no_param.into());
    }
    let mut notes: Vec<String> = Vec::new();
    let get = |p: &str| r.mods.iter().find_map(|m| m.strip_prefix(p)).and_then(|v| v.parse::<f32>().ok());
    let skipped: Vec<&str> = r
        .mods
        .iter()
        .filter(|m| ["mode:", "hold:", "quantize:", "delta:", "BIP"].iter().any(|p| m.starts_with(p)))
        .map(String::as_str)
        .collect();
    if !skipped.is_empty() {
        notes.push(fill(t.n_mods_not_live, &[&skipped.join(" ")]));
    }
    let mut out = Vec::new();
    for sig in &sigs {
        let gates = match gate_signals(r, sig, t) {
            Ok(g) => g,
            Err(e) => return (Vec::new(), e),
        };
        for t in &targets {
            // both hands are one line: route.rs averages them
            let mut route = Route::to(sig, t.clone(), get("amt:").unwrap_or(1.0));
            route.lo = get("lo:").unwrap_or(0.0);
            route.hi = get("hi:").unwrap_or(1.0);
            route.gates.clone_from(&gates);
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
                bound: bound.then(|| resolve::Bound { index: 12, name: "Amp Drive".into(), via: resolve::Via::Name, plugin: None }),
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
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch"]);
    }

    #[test]
    fn no_side_no_route_and_unsided_sources_work() {
        let (rs, note) = routes(&resolved("hand.openness", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        let (rs, _) = routes(&resolved("hand.y", Some("both"), &["hi:0.8"], vec![], true), Some(&fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() }));
        assert_eq!(rs.iter().map(|r| r.signal.as_str()).collect::<Vec<_>>(), ["cam0.hand.L.y", "cam0.hand.R.y"]);
        assert!(rs.iter().all(|r| r.depth == 1.0 && (r.hi - 0.8).abs() < 1e-6));
        let (rs, note) = routes(&resolved("mouth.open", None, &["mode:step"], vec![], false), None);
        assert!(rs.is_empty() && note == "no parameter to drive", "{note}");
        let mut macro_route = resolved("mouth.open", None, &["mode:step"], vec![], false);
        macro_route.targets[0].token = "T:macro:3".into();
        let (rs, note) = routes(&macro_route, None);
        assert_eq!(rs[0].target, Target::Macro(2));
        assert_eq!(note, "mode:step not live yet");
        let (rs, note) = routes(&resolved("motion.circle", None, &[], vec![], true), None);
        assert!(rs.is_empty() && note == "motion circle is not live yet", "{note}");
        let (_, note) = routes(&resolved("", None, &[], vec![], true), None);
        assert_eq!(note, "which movement?");
        let (_, note) = routes(&resolved("eye.open", None, &[], vec![], true), None);
        assert!(note.starts_with("which eye?"), "{note}");
    }

    #[test]
    fn gates_take_the_routes_hand_and_never_run_open() {
        let key = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        // two gates: both must hold
        let gates = vec![vec!["G:hand.pinch".to_string(), "side:right".to_string()], vec!["G:eye.open".to_string(), "side:left".to_string()]];
        let (rs, _) = routes(&resolved("hand.openness", Some("left"), &[], gates, true), Some(&key));
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch", "cam0.eye.L.open"]);
        // a sided gate without a side takes the route's hand
        let gates = vec![vec!["G:hand.pinch".to_string()]];
        let (rs, _) = routes(&resolved("hand.y", Some("both"), &[], gates.clone(), true), Some(&key));
        assert_eq!(rs.iter().map(|r| r.gates[0].as_str()).collect::<Vec<_>>(), ["cam0.hand.L.pinch", "cam0.hand.R.pinch"]);
        // an unsided source has no hand to lend: no route, not an ungated one
        let (rs, note) = routes(&resolved("mouth.open", None, &[], gates, true), Some(&key));
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        // the sentence's side for an unsided source; an eye lends no hand
        let mut mouth = resolved("mouth.open", None, &[], vec![vec!["G:hand.pinch".to_string()]], true);
        mouth.side = Some("right".into());
        let (rs, _) = routes(&mouth, Some(&key));
        assert_eq!(rs[0].gates, ["cam0.hand.R.pinch"]);
        let (rs, note) = routes(&resolved("eye.open", Some("left"), &[], vec![vec!["G:hand.pinch".to_string()]], true), Some(&key));
        assert!(rs.is_empty() && note.contains("which hand"), "{note}");
        let shape = vec![vec!["G:hand.shape".to_string(), "shape:fist".to_string()]];
        let (rs, note) = routes(&resolved("mouth.open", None, &[], shape, true), Some(&key));
        assert!(rs.is_empty() && note.contains("not live yet"), "{note}");
    }

    #[test]
    fn a_chip_from_another_plugin_keeps_its_plugin() {
        let picked = fl::Key { at: fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let mut r = resolved("hand.openness", Some("left"), &[], vec![], true);
        if let Some(b) = r.targets[0].bound.as_mut() {
            b.plugin = Some("fx:0:0:Topos".into());
        }
        let topos = fl::Key { at: fl::At::Fx { track: 0, slot: 0 }, plugin: "Topos".into() };
        assert_eq!(targets(&r, Some(&picked)), [Target::Param { plugin: topos, index: 12 }]);
    }
}
