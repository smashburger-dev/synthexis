//! Parameter classes for the picked plugin: the label of a mapped plugin
//! where there is one (known.tsv, 887 plugins), else param-sense, the
//! small classifier of tools/mapping/train_paramsense.py (hashed character
//! n-grams of the name and words of the plugin, one hidden layer). Both come
//! from tools/mapping/export_paramsense.py into the completion dir.
//!
//! param-sense runs runtime-only (no value strings), on the GUI thread when
//! FL sends a plugin's names: a few ms for 2,600 names. Sizes come from
//! meta.json, so a retrained run with more buckets or a wider layer loads
//! unchanged. Parity with the Python model: tests/fixtures/paramsense_parity.json.
//!
//! Labels and param-sense together hold about 100 MB, so every instance
//! shares one copy (shared()), loaded when a picked plugin first needs
//! classes and dropped after IDLE without use (probes/plugin-settings, 5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use crate::log::log;

/// Unused this long, the shared classes are dropped.
const IDLE: Duration = Duration::from_secs(180);
const CHECK: Duration = Duration::from_secs(15);

struct SharedState {
    classes: Option<Arc<Classes>>,
    loading: bool,
    used: Option<Instant>,
}

static SHARED: Mutex<SharedState> = Mutex::new(SharedState { classes: None, loading: false, used: None });

/// The shared classes of `dir` once loaded. The first call starts loading
/// them on a thread and returns None: ask again on a later frame. Every
/// call counts as a use; that thread drops them after IDLE without one.
pub fn shared(dir: &Path) -> Option<Arc<Classes>> {
    let mut s = SHARED.lock();
    s.used = Some(Instant::now());
    if let Some(c) = &s.classes {
        return Some(c.clone());
    }
    if s.loading {
        return None;
    }
    let dir: PathBuf = dir.to_path_buf();
    let spawned = std::thread::Builder::new().name("synthexis-classes".into()).spawn(move || {
        let t0 = Instant::now();
        let (classes, notes) = Classes::load(&dir);
        for n in notes {
            log(&format!("completion: classes: {n}"));
        }
        log(&format!("completion: labels and param-sense loaded in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3));
        {
            let mut s = SHARED.lock();
            s.classes = Some(Arc::new(classes));
            s.loading = false;
        }
        loop {
            std::thread::sleep(CHECK);
            let mut s = SHARED.lock();
            if s.used.is_none_or(|t| t.elapsed() >= IDLE) {
                s.classes = None;
                log(&format!("completion: labels and param-sense dropped, unused for {} s", IDLE.as_secs()));
                return;
            }
        }
    });
    match spawned {
        Ok(_) => {
            s.loading = true;
            None
        }
        Err(e) => {
            // without the thread the picker still works, with empty classes
            log(&format!("completion: classes loader not started: {e}"));
            Some(Arc::new(Classes::default()))
        }
    }
}

/// Python's str.casefold for the letters plugin names use.
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

/// Python's re.findall(r"[^\W_]+", casefold(s)).
fn words(s: &str) -> Vec<String> {
    casefold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn fnv_bytes(mut h: u32, bytes: impl IntoIterator<Item = u8>) -> u32 {
    for b in bytes {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a 32 over the UTF-8 of field + "|" + gram.
fn fnv1a(field: &str, gram: &str) -> u32 {
    fnv_bytes(fnv_bytes(0x811C_9DC5, field.bytes().chain(*b"|")), gram.bytes())
}

fn f16_to_f32(b: [u8; 2]) -> f32 {
    let h = u16::from_le_bytes(b);
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1F);
    let frac = f32::from(h & 0x3FF);
    match exp {
        0 => sign * frac * (2.0f32).powi(-24),
        31 if frac == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + frac / 1024.0) * (2.0f32).powi(e - 15),
    }
}

/// erf with an error below 1.5e-7 (Abramowitz and Stegun 7.1.26), for GELU.
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152_1) * t) + 1.421_413_8) * t - 0.284_496_74) * t + 0.254_829_6)
            * t
            * (-x * x).exp();
    y.copysign(x)
}

pub struct Sense {
    classes: Vec<String>,
    buckets: u32,
    dim: usize,
    hidden: usize,
    /// The embedding table stays f16 (half the memory, 31 MB for ps-v2);
    /// mean() widens the rows it reads through `wide`, every f16 as f32.
    emb: Vec<u16>,
    wide: Vec<f32>,
    hid_w: Vec<f32>,
    hid_b: Vec<f32>,
    cls_w: Vec<f32>,
    cls_b: Vec<f32>,
}

impl Sense {
    pub fn load(dir: &Path) -> Result<Sense, String> {
        let read = |f: &str| std::fs::read(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()));
        let meta: Value = serde_json::from_slice(&read("meta.json")?).map_err(|e| format!("meta.json: {e}"))?;
        let blob = read("weights.f16.bin")?;
        let raw = |name: &str| -> Result<(Vec<usize>, &[u8]), String> {
            let t = meta["tensors"]
                .as_array()
                .and_then(|ts| ts.iter().find(|t| t["name"] == name))
                .ok_or(format!("meta.json: no tensor {name}"))?;
            let shape: Vec<usize> =
                t["shape"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|x| x as usize).collect();
            let (off, n) = (t["offset"].as_u64().unwrap_or(0) as usize, t["nbytes"].as_u64().unwrap_or(0) as usize);
            let bytes = blob.get(off..off + n).ok_or(format!("weights.f16.bin: {name} out of range"))?;
            if shape.iter().product::<usize>() * 2 != n {
                return Err(format!("meta.json: {name} shape does not match its bytes"));
            }
            Ok((shape, bytes))
        };
        let tensor = |name: &str| -> Result<(Vec<usize>, Vec<f32>), String> {
            let (shape, bytes) = raw(name)?;
            Ok((shape, bytes.as_chunks::<2>().0.iter().map(|c| f16_to_f32(*c)).collect()))
        };
        let classes: Vec<String> =
            meta["classes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
        let (es, emb) = raw("emb.weight").map(|(s, b)| (s, b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<u16>>()))?;
        let wide = (0..=u16::MAX).map(|h| f16_to_f32(h.to_le_bytes())).collect();
        let (hs, hid_w) = tensor("hid.weight")?;
        let (_, hid_b) = tensor("hid.bias")?;
        let (cs, cls_w) = tensor("cls.weight")?;
        let (_, cls_b) = tensor("cls.bias")?;
        let (buckets, dim, hidden) = (es[0], es[1], hs[0]);
        if hs[1] != 3 * dim || cs != [classes.len(), hidden] || hid_b.len() != hidden || cls_b.len() != classes.len() {
            return Err("param-sense: tensor shapes do not fit together".into());
        }
        Ok(Sense { classes, buckets: buckets as u32, dim, hidden, emb, wide, hid_w, hid_b, cls_w, cls_b })
    }

    fn bucket(&self, field: &str, gram: &str) -> usize {
        (fnv1a(field, gram) % self.buckets) as usize
    }

    fn name_feats(&self, name: &str) -> Vec<usize> {
        let s: Vec<char> = format!("<{}>", casefold(name)).chars().collect();
        let field = fnv_bytes(0x811C_9DC5, *b"n|");
        let mut utf8 = [0u8; 4];
        let mut out: Vec<usize> = (3..=5)
            .flat_map(|n| s.windows(n))
            .map(|w| {
                let h = w.iter().fold(field, |h, c| fnv_bytes(h, c.encode_utf8(&mut utf8).bytes()));
                (h % self.buckets) as usize
            })
            .chain(words(name).iter().map(|w| self.bucket("n", &format!("w:{w}"))))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Plugin-name words plus the 64 most frequent words of its parameter
    /// names (ties in first-appearance order, like Counter.most_common).
    fn ctx_feats(&self, plugin: &str, names: &[String]) -> Vec<usize> {
        let mut count: HashMap<String, (usize, usize)> = HashMap::new();
        for n in names {
            let mut seen = words(n);
            let mut i = 0;
            while i < seen.len() {
                if seen[..i].contains(&seen[i]) {
                    seen.remove(i);
                } else {
                    i += 1;
                }
            }
            for w in seen {
                let next = count.len();
                count.entry(w).or_insert((0, next)).0 += 1;
            }
        }
        let mut top: Vec<(String, (usize, usize))> = count.into_iter().collect();
        top.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.1 .1.cmp(&b.1 .1)));
        let mut out: Vec<usize> = words(plugin)
            .iter()
            .map(|w| self.bucket("c", &format!("p:{w}")))
            .chain(top.iter().take(64).map(|(w, _)| self.bucket("c", w)))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn mean(&self, ids: &[usize], out: &mut [f32]) {
        out.iter_mut().for_each(|x| *x = 0.0);
        for &i in ids {
            let row = &self.emb[i * self.dim..(i + 1) * self.dim];
            out.iter_mut().zip(row).for_each(|(o, r)| *o += self.wide[usize::from(*r)]);
        }
        if !ids.is_empty() {
            let n = ids.len() as f32;
            out.iter_mut().for_each(|x| *x /= n);
        }
    }

    /// The class of every name, in order.
    pub fn classify(&self, plugin: &str, names: &[String]) -> Vec<String> {
        let d = self.dim;
        // hidden = W_name . name + W_ctx . ctx + W_val . 0 + b; the context
        // part is the same for every name of the plugin
        let mut ctx = vec![0.0f32; d];
        self.mean(&self.ctx_feats(plugin, names), &mut ctx);
        let base: Vec<f32> = (0..self.hidden)
            .map(|j| {
                let w = &self.hid_w[j * 3 * d + d..j * 3 * d + 2 * d];
                self.hid_b[j] + w.iter().zip(&ctx).map(|(a, b)| a * b).sum::<f32>()
            })
            .collect();
        let (mut z, mut h) = (vec![0.0f32; d], vec![0.0f32; self.hidden]);
        names
            .iter()
            .map(|n| {
                self.mean(&self.name_feats(n), &mut z);
                for (j, hj) in h.iter_mut().enumerate() {
                    let w = &self.hid_w[j * 3 * d..j * 3 * d + d];
                    let x = base[j] + w.iter().zip(&z).map(|(a, b)| a * b).sum::<f32>();
                    *hj = 0.5 * x * (1.0 + erf(x / std::f32::consts::SQRT_2));
                }
                let best = (0..self.classes.len())
                    .map(|c| {
                        let w = &self.cls_w[c * self.hidden..(c + 1) * self.hidden];
                        (c, self.cls_b[c] + w.iter().zip(&h).map(|(a, b)| a * b).sum::<f32>())
                    })
                    .fold((0, f32::NEG_INFINITY), |a, b| if b.1 > a.1 { b } else { a });
                self.classes[best.0].clone()
            })
            .collect()
    }
}

/// Labels of the mapped plugins plus param-sense for the rest.
#[derive(Default)]
pub struct Classes {
    /// casefold plugin name -> casefold parameter name -> class
    known: HashMap<String, HashMap<String, String>>,
    sense: Option<Sense>,
}

/// Where a plugin's classes came from, for the picker's note.
#[derive(Debug, Default, PartialEq)]
pub struct Sources {
    pub labelled: usize,
    pub sensed: usize,
}

impl Classes {
    /// known.tsv and paramsense/ from the completion dir; a missing part is
    /// left out and named in the returned notes.
    pub fn load(dir: &Path) -> (Classes, Vec<String>) {
        let mut notes = Vec::new();
        let mut known: HashMap<String, HashMap<String, String>> = HashMap::new();
        match std::fs::read_to_string(dir.join("known.tsv")) {
            Ok(text) => {
                let mut cur = String::new();
                for line in text.lines() {
                    if let Some(p) = line.strip_prefix('#') {
                        cur = p.to_string();
                    } else if let Some((n, c)) = line.split_once('\t') {
                        known.entry(cur.clone()).or_default().insert(n.to_string(), c.to_string());
                    }
                }
            }
            Err(e) => notes.push(format!("known.tsv: {e}")),
        }
        let sense = Sense::load(&dir.join("paramsense")).map_err(|e| notes.push(e)).ok();
        (Classes { known, sense }, notes)
    }

    /// Classes for `names` of `plugin`: `exported` (name -> class) first,
    /// then the labels of a mapped plugin of that name, then param-sense;
    /// "" where neither exists.
    pub fn of(&self, plugin: &str, names: &[String], exported: &HashMap<String, String>) -> (Vec<String>, Sources) {
        let known = self.known.get(&casefold(plugin.trim()));
        let mut src = Sources::default();
        let mut out: Vec<Option<String>> = names
            .iter()
            .map(|n| {
                let c = exported.get(n).or_else(|| known.and_then(|k| k.get(&casefold(n.trim())))).cloned();
                src.labelled += usize::from(c.is_some());
                c
            })
            .collect();
        if out.iter().any(Option::is_none) {
            if let Some(s) = &self.sense {
                let sensed = s.classify(plugin, names);
                for (o, c) in out.iter_mut().zip(sensed) {
                    if o.is_none() {
                        *o = Some(c);
                        src.sensed += 1;
                    }
                }
            }
        }
        (out.into_iter().map(Option::unwrap_or_default).collect(), src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_and_words_match_python() {
        // values from train_paramsense.py's fnv1a and words()
        assert_eq!(fnv1a("n", "<cu"), 1_291_246_841);
        assert_eq!(words("Filter_1 Freq (Hz)"), ["filter", "1", "freq", "hz"]);
        assert_eq!(casefold("Straße"), "strasse");
        assert_eq!(f16_to_f32([0x00, 0x3C]), 1.0);
        assert_eq!(f16_to_f32([0x00, 0xC0]), -2.0);
        assert!((erf(0.5) - 0.520_499_9).abs() < 1e-6);
    }

    /// The shipped run against its Python predictions (export_paramsense.py).
    #[test]
    fn parity_with_python() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sense = match Sense::load(&root.join("../assistant/completion/paramsense")) {
            Ok(s) => s,
            Err(e) => return assert!(e.is_empty(), "{e}"),
        };
        let fixture: Value = match serde_json::from_str(include_str!("../tests/fixtures/paramsense_parity.json")) {
            Ok(v) => v,
            Err(e) => return assert!(e.to_string().is_empty(), "{e}"),
        };
        let (mut same, mut all) = (0, 0);
        for case in fixture["cases"].as_array().into_iter().flatten() {
            let strs = |k: &str| -> Vec<String> {
                case[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
            };
            let (names, want) = (strs("names"), strs("classes"));
            let got = sense.classify(case["plugin"].as_str().unwrap_or(""), &names);
            for ((n, g), w) in names.iter().zip(&got).zip(&want) {
                all += 1;
                if g == w {
                    same += 1;
                } else {
                    eprintln!("{n}: rust {g}, python {w}");
                }
            }
        }
        assert!(all > 2000);
        assert_eq!(same, all, "{same} of {all} classes agree");
    }

    /// The shipped completion data: Serum 2 is labelled, a made-up plugin
    /// gets param-sense classes, in well under a frame.
    #[test]
    fn shipped_classes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (c, notes) = Classes::load(&root.join("../assistant/completion"));
        assert!(notes.is_empty(), "{notes:?}");
        let names: Vec<String> = ["Filter 1 Freq", "A Level"].map(String::from).to_vec();
        let (cls, src) = c.of("Serum 2", &names, &HashMap::new());
        assert_eq!((cls[0].as_str(), src.labelled, src.sensed), ("filter.cutoff", 2, 0));
        let names: Vec<String> = (0..2600).map(|i| format!("Osc {} Cutoff Freq", i % 40)).collect();
        let t0 = std::time::Instant::now();
        let (cls, src) = c.of("Totally Unknown Synth", &names, &HashMap::new());
        assert_eq!((src.labelled, src.sensed), (0, 2600));
        assert_eq!(cls[0], "filter.cutoff");
        eprintln!("param-sense: 2600 names in {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    }

    #[test]
    fn known_labels_win_and_the_rest_is_sensed() {
        let mut known = HashMap::new();
        known.insert("serum 2".to_string(), HashMap::from([("filter 1 freq".to_string(), "filter.cutoff".to_string())]));
        let c = Classes { known, sense: None };
        let names = vec!["Filter 1 Freq".to_string(), "Mystery".to_string()];
        let (cls, src) = c.of("Serum 2", &names, &HashMap::new());
        assert_eq!(cls, ["filter.cutoff", ""]);
        assert_eq!(src, Sources { labelled: 1, sensed: 0 });
    }
}
