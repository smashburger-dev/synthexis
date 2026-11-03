//! Per-user settings, shared by every synthexis instance: the interface
//! language and completion on/off (Noa, 11-03: the language follows the
//! person, not the project, so a project saved in German opens in English
//! for someone else). JSON in ~/Library/Application Support/synthexis/
//! settings.json, $SYNTHEXIS_SETTINGS overrides the path. A missing or
//! broken file gives the defaults (English, completion on); every change is
//! written at once, through a temp file and a rename, and keys this build
//! does not know are kept.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use parking_lot::RwLock;
use serde_json::{json, Value};
use synthexis_complete::Lang;

use crate::log::log;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub lang: Lang,
    /// Suggestions in the sentence field.
    pub completion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: Lang::En, completion: true }
    }
}

pub struct Store {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl Store {
    /// Reads `path` once; what was wrong with the file goes to the log.
    pub fn open(path: PathBuf) -> Self {
        let (s, note) = read(&path);
        if let Some(n) = note {
            log(&format!("settings: {n}, using the defaults"));
        }
        Self { path, current: RwLock::new(s) }
    }

    pub fn get(&self) -> Settings {
        *self.current.read()
    }

    /// Applies `f` and writes the file when something changed.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) {
        let mut cur = self.current.write();
        let mut next = *cur;
        f(&mut next);
        if next == *cur {
            return;
        }
        *cur = next;
        if let Err(e) = write(&self.path, &next) {
            log(&format!("settings: not saved: {e}"));
        }
    }
}

/// The process-wide store every instance reads.
pub fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Store::open(path()))
}

fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("SYNTHEXIS_SETTINGS") {
        return p.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/settings.json")
}

/// The settings in `path` and, when they could not be read as a whole, why.
fn read(path: &Path) -> (Settings, Option<String>) {
    let d = Settings::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (d, None),
        Err(e) => return (d, Some(format!("{}: {e}", path.display()))),
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return (d, Some(format!("{}: {e}", path.display()))),
    };
    let lang = v.get("lang").and_then(Value::as_str).and_then(Lang::from_code);
    let completion = v.get("completion").and_then(Value::as_bool);
    let s = Settings { lang: lang.unwrap_or(d.lang), completion: completion.unwrap_or(d.completion) };
    let note = (lang.is_none() && v.get("lang").is_some()).then(|| format!("unknown language {}", v["lang"]));
    (s, note)
}

fn write(path: &Path, s: &Settings) -> Result<(), String> {
    let mut obj = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert("lang".into(), json!(s.lang.code()));
    obj.insert("completion".into(), json!(s.completion));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&Value::Object(obj)).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("synthexis-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_gives_english_and_completion_on() {
        let s = Store::open(dir("missing").join("settings.json"));
        assert_eq!(s.get(), Settings { lang: Lang::En, completion: true });
    }

    #[test]
    fn a_change_is_saved_and_read_back() {
        let p = dir("roundtrip").join("sub/settings.json");
        let s = Store::open(p.clone());
        s.update(|x| x.lang = Lang::De);
        s.update(|x| x.completion = false);
        assert_eq!(Store::open(p.clone()).get(), Settings { lang: Lang::De, completion: false });
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn broken_or_unknown_values_fall_back_per_key() -> Result<(), Box<dyn std::error::Error>> {
        let d = dir("broken");
        std::fs::create_dir_all(&d)?;
        let p = d.join("settings.json");
        std::fs::write(&p, "{\"lang\": \"fr\", \"completion\": false}")?;
        let (s, note) = read(&p);
        assert_eq!(s, Settings { lang: Lang::En, completion: false });
        assert!(note.is_some_and(|n| n.contains("fr")));
        std::fs::write(&p, "{not json")?;
        assert_eq!(read(&p).0, Settings::default());
        // a broken file is replaced by the next change
        let s = Store::open(p.clone());
        s.update(|x| x.lang = Lang::De);
        assert_eq!(read(&p), (Settings { lang: Lang::De, completion: true }, None));
        Ok(())
    }

    #[test]
    fn keys_of_other_builds_are_kept() -> Result<(), Box<dyn std::error::Error>> {
        let d = dir("keys");
        std::fs::create_dir_all(&d)?;
        let p = d.join("settings.json");
        std::fs::write(&p, "{\"lang\": \"en\", \"theme\": \"dark\"}")?;
        Store::open(p.clone()).update(|x| x.lang = Lang::De);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
        assert_eq!((v["lang"].as_str(), v["theme"].as_str()), (Some("de"), Some("dark")));
        Ok(())
    }
}
