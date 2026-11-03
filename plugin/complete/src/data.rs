//! Loads what tools/mapping/export_completion.py writes:
//!
//!   <dir>/vocab.json           gesture phrases, class phrases, target
//!                              phrases, class prior, exported plugins
//!   <dir>/plugins/<slug>.json  one plugin's parameters (index, name, class)

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::{Engine, Gesture, Param};

#[derive(Deserialize)]
struct VocabFile {
    gestures: Vec<GestureRow>,
    param_phrases: Vec<PhraseRow>,
    #[serde(default)]
    targets: Vec<TargetRow>,
    class_prior: HashMap<String, u64>,
    plugins: Vec<PluginInfo>,
}

#[derive(Deserialize)]
struct GestureRow {
    source: String,
    #[serde(default)]
    shape: Option<String>,
    p: String,
    pol: i8,
}

#[derive(Deserialize)]
struct PhraseRow {
    class: String,
    p: String,
}

#[derive(Deserialize)]
struct TargetRow {
    kind: String,
    p: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PluginInfo {
    pub plugin_id: String,
    pub name: String,
    pub file: String,
    pub n: usize,
}

#[derive(Deserialize)]
struct PluginFile {
    params: Vec<ParamRow>,
}

#[derive(Deserialize)]
struct ParamRow {
    i: u32,
    n: String,
    c: String,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Engine without a plugin, plus the plugins that can be picked.
pub fn load(dir: &Path) -> Result<(Engine, Vec<PluginInfo>), String> {
    let path = dir.join("vocab.json");
    let v: VocabFile = serde_json::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    let gestures = v
        .gestures
        .into_iter()
        .map(|g| Gesture { source: g.source, shape: g.shape, phrase: g.p, pol: g.pol })
        .collect();
    let phrases = v.param_phrases.into_iter().map(|r| (r.class, r.p)).collect();
    let targets = v.targets.into_iter().map(|r| (r.kind, r.p)).collect();
    Ok((Engine::new(gestures, phrases, &v.class_prior).with_targets(targets), v.plugins))
}

pub fn load_plugin(dir: &Path, info: &PluginInfo) -> Result<Vec<Param>, String> {
    let path = dir.join("plugins").join(&info.file);
    let f: PluginFile = serde_json::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(f.params
        .into_iter()
        .map(|p| Param { index: p.i, name: p.n, class: p.c })
        .collect())
}
