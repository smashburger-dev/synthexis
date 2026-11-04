//! Saved parts (ui-redesign R17): one JSON file per name in
//! ~/Library/Application Support/synthexis/parts/ ($SYNTHEXIS_PARTS
//! overrides it), `{"version":1,"name":..,"play":..,"mode":..,"start":..}`.
//! Files are written through a tmp file and a rename, so a crash never
//! leaves half a part. The slug and the atomic write are this file's own
//! (preset.rs has its twins), so the part library and the preset library
//! never share a file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::midi::{Mode, Part, Play, Start};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartPreset {
    pub version: u32,
    pub name: String,
    pub play: Play,
    pub mode: Mode,
    pub start: Start,
}

/// $SYNTHEXIS_PARTS, else ~/Library/Application Support/synthexis/parts.
/// Tests without the variable get a folder of their own under the temp
/// dir, so they never read or write the person's parts.
fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_PARTS").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("synthexis-parts-test-{}", std::process::id()));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/parts")
}

/// The saved parts' names, sorted without regard to case.
pub fn list() -> Vec<String> {
    list_in(&dir())
}

pub fn load(name: &str) -> Result<PartPreset, String> {
    load_in(&dir(), name)
}

pub fn save(name: &str, part: &Part) -> Result<(), String> {
    save_in(&dir(), name, part)
}

pub fn delete(name: &str) -> Result<(), String> {
    delete_in(&dir(), name)
}

fn list_in(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| read(&p).ok())
        .map(|p| p.name)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

fn read(path: &Path) -> Result<PartPreset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("part not read: {e}"))?;
    let p: PartPreset = serde_json::from_str(&text).map_err(|e| format!("part not read: {e}"))?;
    if p.version > VERSION {
        return Err(format!("part from a newer synthexis (version {})", p.version));
    }
    Ok(p)
}

fn load_in(dir: &Path, name: &str) -> Result<PartPreset, String> {
    read(&dir.join(file_name(name)))
}

fn save_in(dir: &Path, name: &str, part: &Part) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("type a name first".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("part not written: {e}"))?;
    let p = PartPreset { version: VERSION, name: name.to_string(), play: part.play.clone(), mode: part.mode, start: part.start };
    let text = serde_json::to_string_pretty(&p).map_err(|e| format!("part not written: {e}"))?;
    write_atomic(dir, &dir.join(file_name(name)), &text)
}

fn delete_in(dir: &Path, name: &str) -> Result<(), String> {
    std::fs::remove_file(dir.join(file_name(name))).map_err(|e| format!("part not deleted: {e}"))
}

fn file_name(name: &str) -> String {
    format!("{}.json", slug(name))
}

/// Lower case ASCII letters and digits, accents dropped ("Bässe" ->
/// "basse"), every other run one '-'.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'ñ' => "n",
            'ç' => "c",
            'ß' => "ss",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => {
                if !out.is_empty() && !out.ends_with('-') {
                    out.push('-');
                }
                continue;
            }
        };
        out.push_str(plain);
    }
    let s: String = out.trim_end_matches('-').chars().take(60).collect();
    if s.is_empty() { "part".into() } else { s }
}

static TMP: AtomicU64 = AtomicU64::new(0);

fn write_atomic(dir: &Path, path: &Path, text: &str) -> Result<(), String> {
    let tmp = dir.join(format!(".part.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, text).map_err(|e| format!("part not written: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("part not written: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::Step;

    #[test]
    fn part_presets_save_list_load_delete() {
        let d = std::env::temp_dir().join(format!("synthexis-parts-{}-{}", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        assert!(list_in(&d).is_empty(), "no folder yet: no parts");
        let play = Play::Clip { bars: 2, steps: vec![Step { at: 3, len: 1, pitch: 42, vel: 90, ratchet: 2, nudge: 0 }], rows: vec![42] };
        let part = Part { id: 9, name: "ignored".into(), channel: None, play: play.clone(), mode: Mode::Latch, start: Start::Beat };
        assert!(save_in(&d, "  ", &part).is_err(), "an empty name is refused");
        assert_eq!(save_in(&d, "Hats Bässe", &part), Ok(()));
        assert_eq!(save_in(&d, "Another", &part), Ok(()));
        assert_eq!(list_in(&d), vec!["Another".to_string(), "Hats Bässe".to_string()]);
        let back = load_in(&d, "Hats Bässe");
        assert_eq!(back, Ok(PartPreset { version: 1, name: "Hats Bässe".into(), play, mode: Mode::Latch, start: Start::Beat }));
        // overwrite keeps one file per name
        let mut other = part.clone();
        other.mode = Mode::Once;
        assert_eq!(save_in(&d, "Hats Bässe", &other), Ok(()));
        assert_eq!(list_in(&d).len(), 2);
        assert_eq!(load_in(&d, "Hats Bässe").map(|p| p.mode), Ok(Mode::Once));
        let files: Vec<String> = std::fs::read_dir(&d).map(|r| r.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        assert!(files.iter().all(|f| !f.ends_with(".tmp")), "no tmp file stays: {files:?}");
        assert!(files.contains(&"hats-basse.json".to_string()), "{files:?}");
        assert_eq!(delete_in(&d, "Hats Bässe"), Ok(()));
        assert_eq!(list_in(&d), vec!["Another".to_string()]);
        assert!(load_in(&d, "Hats Bässe").is_err());
        assert!(delete_in(&d, "Hats Bässe").is_err());
        let text = std::fs::read_to_string(d.join("another.json")).unwrap_or_default();
        assert!(text.contains("\"version\": 1") && text.contains("\"name\": \"Another\""), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
