//! User aliases (map.aliases in probes/mapping-pipeline/SPEC_PROPOSAL.md):
//! phrase -> parameter name per plugin, checked by the resolver before
//! names and classes. Learnt when the user picks a parameter chip through a
//! class phrase ("Cutoff" -> Filter 1 Freq on Serum 2): the next sentence
//! that says "cutoff" without a chip lands on the same parameter.
//!
//! One JSON file, {plugin name: {alias key: parameter name}}, at
//! $SYNTHEXIS_ALIASES, else ~/Library/Application Support/synthexis/aliases.json.
//! Read once per process, written on every new alias (GUI thread, a few KB).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use parking_lot::Mutex;
use synthexis_parser::resolve::alias_key;

use crate::log::log;

type Store = HashMap<String, HashMap<String, String>>;

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("synthexis-aliases-test-{}.json", std::process::id()));
    }
    if let Some(p) = std::env::var_os("SYNTHEXIS_ALIASES") {
        return p.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/aliases.json")
}

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        let s = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok());
        Mutex::new(s.unwrap_or_default())
    })
}

fn plugin_key(plugin: &str) -> String {
    plugin.trim().to_lowercase()
}

/// alias key -> parameter name for `plugin` (its name, not the FL slot).
pub fn of(plugin: &str) -> HashMap<String, String> {
    store().lock().get(&plugin_key(plugin)).cloned().unwrap_or_default()
}

/// Remembers that `phrase` means parameter `name` of `plugin`.
pub fn learn(plugin: &str, phrase: &str, name: &str) {
    let key = alias_key(phrase);
    if key.is_empty() {
        return;
    }
    let mut s = store().lock();
    let m = s.entry(plugin_key(plugin)).or_default();
    if m.get(&key).is_some_and(|n| n == name) {
        return;
    }
    m.insert(key.clone(), name.to_string());
    log(&format!("alias: \"{key}\" -> {plugin} / {name}"));
    let res = serde_json::to_string_pretty(&*s).map_err(|e| e.to_string()).and_then(|t| {
        let p = path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, t).map_err(|e| format!("{}: {e}", p.display()))
    });
    if let Err(e) = res {
        log(&format!("alias: not saved: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learn_then_read() {
        learn("Serum 2", "the Cutoff", "Filter 1 Freq");
        assert_eq!(of("serum 2").get("cutoff").map(String::as_str), Some("Filter 1 Freq"));
        let _ = std::fs::remove_file(path());
    }
}
