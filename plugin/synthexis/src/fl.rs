//! FL Studio bridge: what the open FL project holds and which plugin window
//! the user last clicked, for the mapping assistant's plugin picker.
//!
//! The plugin owns a CoreMIDI virtual source and destination, both named
//! "synthexis", which FL binds to the "synthexis bridge" device script
//! (plugin/fl/device_synthexis_bridge.py, protocol 4). Frames both ways are
//! F0 7D <ASCII JSON> F7. One worker thread owns the ports, sends requests
//! and pages through replies; the script pushes focus and inventory events
//! on its own. The GUI reads a snapshot (`View`) and asks for parameter
//! names with `want`; the route writer sends parameter values with `write`
//! (fire and forget, the worker keeps only the latest value per parameter);
//! nothing here runs on the audio thread.
//!
//! Setup without MIDI settings (probes/fl-midi-autolink/RESULT.md): FL asks
//! every MIDI device it has never seen who it is (Universal Device Inquiry,
//! about 1 s after the ports appear). The destination's callback answers
//! with `IDENTITY`, which the script lists in `# supportedHardwareIds=`, so
//! FL enables the input, links the output and loads the script itself; the
//! shared name matches `# supportedDevices=` as a second way. FL never asks
//! again for a name it stored, so a port name stays fixed once shipped.
//!
//! Test hook: SYNTHEXIS_FL_PORTS="name" opens both ports as "name" without
//! the fixed IDs, next to a running probe daemon (plugin/fl/test/fakefl.swift).
//!
//! Size rule (probes/fl-target-picker/RESULT.md, section 1): a 65,536-byte
//! frame froze FL 26.1.7, so requests ask for pages of PAGE bytes and the
//! script refuses replies above 60,000 bytes.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use coremidi::{Client, PacketBuffer, PacketList, Sources, VirtualSource};
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use parking_lot::{Mutex, MutexGuard};
use serde_json::{json, Value};

use crate::log::log;

/// Source and destination share the name: FL pairs them by it when it
/// assigns the script by name. Before 2026-11-03 the ports were "synthexis
/// bridge" / "synthexis bridge return"; FL had stored those without an ID,
/// and it never asks a stored name again.
const PORT: &str = "synthexis";
const SOURCE_UID: i32 = 0x5359_4E01;
const DEST_UID: i32 = 0x5359_4E02;
const PROTO: u64 = 4;
/// Our reply to FL's device inquiry: manufacturer 7D (non-commercial),
/// family "SX", model "FL", version = protocol. The script's
/// `# supportedHardwareIds=7D 53 58 46 4C` is a prefix of it.
const IDENTITY: [u8; 9] = [0x7D, 0x53, 0x58, 0x46, 0x4C, PROTO as u8, 0, 0, 0];
const PAGE: u64 = 16_000;
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(2);
/// FL loads the script a few seconds after the ports appear (7 s at the
/// first setup in FL 26.1.7, probes/fl-midi-autolink); no answer before
/// then is still "waiting", not "FL does not answer".
const SETUP: Duration = Duration::from_secs(20);
const KEEPALIVE: Duration = Duration::from_secs(10);
/// Our own plugin name; FL shows it like any other effect or generator.
const SELF_NAME: &str = "synthexis";

/// Where a plugin sits in the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum At {
    /// Mixer track (0 = Master) and effect slot (0..9).
    Fx { track: u32, slot: u32 },
    /// Channel rack generator.
    Gen { ch: u32 },
}

/// One plugin instance: place plus plugin name, so a different plugin in
/// the same slot is a different key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub at: At,
    pub plugin: String,
}

impl Key {
    /// Stable text form, also the completion engine's plugin id:
    /// "fx:<track>:<slot>:<plugin>" or "gen:<channel>:<plugin>".
    pub fn id(&self) -> String {
        match self.at {
            At::Fx { track, slot } => format!("fx:{track}:{slot}:{}", self.plugin),
            At::Gen { ch } => format!("gen:{ch}:{}", self.plugin),
        }
    }

    pub fn parse(s: &str) -> Option<Key> {
        let mut it = s.splitn(4, ':');
        let num = |x: Option<&str>| x.and_then(|v| v.parse::<u32>().ok());
        match it.next()? {
            "fx" => {
                let (track, slot) = (num(it.next())?, num(it.next())?);
                Some(Key { at: At::Fx { track, slot }, plugin: it.next()?.to_string() })
            }
            "gen" => {
                let ch = num(it.next())?;
                let rest: Vec<&str> = it.collect();
                Some(Key { at: At::Gen { ch }, plugin: rest.join(":") })
            }
            _ => None,
        }
    }

    /// "Portal, Master slot 1", "Portal, Track 3 slot 0", "Serum 2, Channel 1".
    pub fn label(&self) -> String {
        match self.at {
            At::Fx { track: 0, slot } => format!("{}, Master slot {slot}", self.plugin),
            At::Fx { track, slot } => format!("{}, Track {track} slot {slot}", self.plugin),
            At::Gen { ch } => format!("{}, Channel {ch}", self.plugin),
        }
    }
}

/// A picker entry: the plugin and the mixer track it sits on or, for a
/// generator, the track its channel feeds.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub key: Key,
    pub track: u32,
    pub track_name: String,
}

impl Target {
    /// Generators are sound sources, mixer slots effects.
    pub fn is_sound(&self) -> bool {
        matches!(self.key.at, At::Gen { .. })
    }

    /// "Master", "Track 3", or "Track 3 · Drums" once the user renamed it.
    pub fn track_label(&self) -> String {
        let name = self.track_name.trim();
        match self.track {
            0 => "Master".to_string(),
            t if name.is_empty() || name == format!("Insert {t}") => format!("Track {t}"),
            t => format!("Track {t} · {name}"),
        }
    }
}

fn is_self(plugin: &str) -> bool {
    plugin.trim().eq_ignore_ascii_case(SELF_NAME)
}

/// Names that are no real parameter, checked against every harvested plugin
/// (tools/mapping/fl_param_counts.py has the same rule): a plugin's own MIDI
/// slots ("MIDI CC 7|0" in JUCE plugins, "MIDI CC" in FabFilter, "CC0 Chan 1"
/// and "Pitch Bend Chan 16" in Serum), unused or internal slots ("Reserved3"
/// in Valhalla, "X (Reserved)" in ShaperBox, "Internal", "unused"). FL's
/// appended CC entries (index >= 4096) never arrive, the script stops
/// before them.
fn is_placeholder(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    let digits = |r: &str| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit());
    let midi = match n.strip_prefix("midi cc") {
        Some(r) => r.is_empty() || r.starts_with([' ', '#']) || r.starts_with(|c: char| c.is_ascii_digit()),
        None => n.strip_prefix("cc").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit() || c == '#' || c == ' ')),
    };
    midi || n.rsplit_once(" chan ").is_some_and(|(_, ch)| digits(ch))
        || n.strip_prefix("reserved").is_some_and(|r| r.is_empty() || digits(r.trim()))
        || n.contains("(reserved)")
        || n == "unused"
        || n == "internal"
}

/// Link::Down reasons the window puts in words (sentence.rs).
pub const NO_ANSWER: &str = "FL not answering";
pub const OLD_SCRIPT: &str = "FL script speaks protocol ";

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Link {
    #[default]
    Waiting,
    Up,
    Down(String),
}

/// Parameter names (index, name) of one plugin, or why FL sent none.
pub type Params = Result<Vec<(u32, String)>, String>;

/// What the GUI shows; `gen` counts changes.
#[derive(Default)]
pub struct View {
    pub link: Link,
    /// Plugins in the project, mixer slots first, synthexis left out.
    pub targets: Vec<Target>,
    /// The inventory arrived at least once since the link came up.
    pub loaded: bool,
    /// Last plugin window focused in FL (never synthexis, kept when a
    /// non-plugin window takes the focus).
    pub focus: Option<Key>,
    /// Parameter names (index, name) of the last wanted plugin.
    pub params: Option<(Key, Params)>,
    pub gen: u64,
}

/// A plugin's parameter names as FL sent them, for whoever classifies.
pub type Names = (Key, Vec<(u32, String)>);

pub struct Bridge {
    view: Mutex<View>,
    want: Sender<Key>,
    /// Gets every parameter list as it arrives (on_names).
    names: Mutex<Vec<Sender<Names>>>,
    writes: Sender<(Key, u32, f32)>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Starts the bridge once per process. Never call from the audio thread.
pub fn bridge() -> &'static Bridge {
    BRIDGE.get_or_init(|| {
        let (want, rx) = unbounded();
        let (writes, wrx) = unbounded();
        if let Err(e) = std::thread::Builder::new()
            .name("synthexis-fl".into())
            .spawn(move || run(rx, wrx))
        {
            log(&format!("fl: worker not started: {e}"));
        }
        Bridge { view: Mutex::new(View::default()), want, names: Mutex::new(Vec::new()), writes }
    })
}

impl Bridge {
    pub fn view(&self) -> MutexGuard<'_, View> {
        self.view.lock()
    }

    /// Loads the parameter names of `key` into `View::params`.
    pub fn want(&self, key: &Key) {
        let _ = self.want.send(key.clone());
    }

    /// Sets parameter `index` of `key` to `v` (0..1) through the FL script;
    /// FL records it as automation while it records.
    pub fn write(&self, key: &Key, index: u32, v: f32) {
        let _ = self.writes.send((key.clone(), index, v));
    }

    /// Sends `tx` every parameter list FL sends from now on, and the
    /// current one at once, so classification starts when the names arrive
    /// instead of on the next frame of the synthexis window.
    pub fn on_names(&self, tx: Sender<Names>) {
        // under the view lock: a list arriving meanwhile is sent once more
        // at worst, never missed
        let v = self.view.lock();
        if let Some((k, Ok(names))) = &v.params {
            let _ = tx.send((k.clone(), names.clone()));
        }
        self.names.lock().push(tx);
    }

    /// Stores FL's answer for `key` and hands a list to the on_names
    /// subscribers (dropping the gone ones).
    pub(crate) fn set_params(&self, key: Key, res: Params) {
        let names = res.as_ref().ok().cloned();
        self.update(|v| v.params = Some((key.clone(), res)));
        if let Some(n) = names {
            self.names.lock().retain(|tx| tx.send((key.clone(), n.clone())).is_ok());
        }
    }

    /// A bridge without ports or worker; tests feed its view by hand.
    #[cfg(test)]
    pub fn detached() -> &'static Bridge {
        let (want, rx) = unbounded();
        let (writes, wrx) = unbounded();
        std::mem::forget((rx, wrx));
        Box::leak(Box::new(Bridge { view: Mutex::new(View::default()), want, names: Mutex::new(Vec::new()), writes }))
    }

    pub(crate) fn update(&self, f: impl FnOnce(&mut View)) {
        let mut v = self.view.lock();
        f(&mut v);
        v.gen += 1;
    }
}

/// Splits a byte stream into SysEx frames, F0 ... F7 included.
#[derive(Default)]
struct Deframer {
    buf: Vec<u8>,
}

impl Deframer {
    fn feed(&mut self, data: &[u8], out: &mut impl FnMut(&[u8])) {
        self.buf.extend_from_slice(data);
        loop {
            let Some(start) = self.buf.iter().position(|&b| b == 0xF0) else {
                self.buf.clear();
                return;
            };
            self.buf.drain(..start);
            let Some(end) = self.buf.iter().position(|&b| b == 0xF7) else { return };
            out(&self.buf[..=end]);
            self.buf.drain(..=end);
        }
    }
}

/// The JSON of an F0 7D ... F7 frame.
fn json_payload(frame: &[u8]) -> Option<&[u8]> {
    (frame.len() >= 3 && frame[1] == 0x7D).then(|| &frame[2..frame.len() - 1])
}

/// The identity reply when `frame` is a Universal Device Inquiry
/// (F0 7E <device> 06 01 F7, any device number).
fn identity_reply(frame: &[u8]) -> Option<Vec<u8>> {
    matches!(frame, [0xF0, 0x7E, _, 0x06, 0x01, 0xF7]).then(|| {
        let mut r = vec![0xF0, 0x7E, 0x7F, 0x06, 0x02];
        r.extend_from_slice(&IDENTITY);
        r.push(0xF7);
        r
    })
}

fn frame(v: &Value) -> Vec<u8> {
    // serde_json escapes nothing above ASCII by default; FL decodes ASCII
    let text = v.to_string();
    let mut out = Vec::with_capacity(text.len() + 3);
    out.extend_from_slice(&[0xF0, 0x7D]);
    out.extend(text.chars().flat_map(|c| {
        if c.is_ascii() {
            vec![c as u8]
        } else {
            let mut b = [0u16; 2];
            c.encode_utf16(&mut b).iter().flat_map(|u| format!("\\u{u:04x}").into_bytes()).collect()
        }
    }));
    out.push(0xF7);
    out
}

struct Ports {
    _client: Client,
    source: Arc<VirtualSource>,
    _dest: coremidi::VirtualDestination,
}

fn open_ports(frames: Sender<Vec<u8>>) -> Result<Ports, String> {
    let test = std::env::var("SYNTHEXIS_FL_PORTS").ok();
    let name = test.clone().unwrap_or_else(|| PORT.to_string());
    if Sources.into_iter().any(|s| s.name().as_deref() == Some(name.as_str())) {
        return Err(format!("another process owns \"{name}\" (synthexis open in another host?)"));
    }
    let client = Client::new("synthexis").map_err(|e| format!("MIDI client: {e}"))?;
    let source = Arc::new(client.virtual_source(&name).map_err(|e| format!("MIDI source: {e}"))?);
    let mut de = Deframer::default();
    let answer = Arc::clone(&source);
    let dest = client
        .virtual_destination(&name, move |list: &PacketList| {
            for p in list.iter() {
                de.feed(p.data(), &mut |f| {
                    // answered here on the CoreMIDI thread, whatever the worker
                    // is waiting for: FL lists devices once and moves on
                    if let Some(r) = identity_reply(f) {
                        let sent = answer.received(&PacketBuffer::new(0, &r));
                        log(&format!("fl: FL asked who we are, answered: {sent:?}"));
                    } else if let Some(j) = json_payload(f) {
                        let _ = frames.send(j.to_vec());
                    }
                });
            }
        })
        .map_err(|e| format!("MIDI destination: {e}"))?;
    // fixed IDs so FL re-identifies the ports across restarts
    let ids = if test.is_some() { vec![] } else { vec![(&***source, SOURCE_UID), (&**dest, DEST_UID)] };
    for (obj, uid) in ids {
        if let Err(e) = obj.set_property_integer("uniqueID", uid) {
            log(&format!("fl: unique id {uid:#x} not set: {e}"));
        }
    }
    Ok(Ports { _client: client, source, _dest: dest })
}

fn run(want: Receiver<Key>, writes: Receiver<(Key, u32, f32)>) {
    let b = bridge();
    let (tx, frames) = unbounded();
    let ports = match open_ports(tx) {
        Ok(p) => p,
        Err(e) => {
            log(&format!("fl: {e}"));
            b.update(|v| v.link = Link::Down(e));
            return;
        }
    };
    log("fl: MIDI ports open");
    let mut w = Worker {
        b,
        ports,
        frames,
        opened: Instant::now(),
        next_id: 1,
        hash: None,
        sync: true,
        cache: HashMap::new(),
        unsent: Vec::new(),
    };
    let mut last = Instant::now();
    loop {
        if w.sync {
            w.sync = false;
            w.connect();
            last = Instant::now();
        }
        let wait = if b.view().link == Link::Up { KEEPALIVE } else { RETRY };
        select! {
            recv(w.frames) -> f => match f {
                Ok(f) => w.on_frame(&f),
                Err(_) => return,
            },
            recv(want) -> k => match k {
                Ok(k) => w.load_params(k),
                Err(_) => return,
            },
            recv(writes) -> m => match m {
                Ok(first) => w.write(first, &writes),
                Err(_) => return,
            },
            default(wait.saturating_sub(last.elapsed())) => w.sync = true,
        }
        if w.hash.is_none() && b.view().link == Link::Up {
            w.load_inventory();
        }
    }
}

struct Worker {
    b: &'static Bridge,
    ports: Ports,
    frames: Receiver<Vec<u8>>,
    opened: Instant,
    next_id: u64,
    /// Inventory hash the targets were loaded at.
    hash: Option<i64>,
    sync: bool,
    cache: HashMap<Key, Vec<(u32, String)>>,
    /// Latest value per parameter not sent yet (the link was down).
    unsent: Vec<(Key, u32, f32)>,
}

impl Worker {
    fn send(&self, v: &Value) {
        let bytes = frame(v);
        if let Err(e) = self.ports.source.received(&PacketBuffer::new(0, &bytes)) {
            log(&format!("fl: send failed: {e}"));
        }
    }

    /// One request; events that arrive meanwhile are handled in passing.
    fn call(&mut self, mut req: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        req["id"] = json!(id);
        self.send(&req);
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(f) = self.frames.recv_timeout(left) else {
                return Err(NO_ANSWER.into());
            };
            let Ok(v) = serde_json::from_slice::<Value>(&f) else { continue };
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return match v.get("error").and_then(Value::as_str) {
                    Some(e) => Err(e.to_string()),
                    None => Ok(v),
                };
            }
            self.on_event(&v);
        }
    }

    fn on_frame(&mut self, f: &[u8]) {
        if let Ok(v) = serde_json::from_slice::<Value>(f) {
            self.on_event(&v);
        }
    }

    fn on_event(&mut self, v: &Value) {
        match v.get("ev").and_then(Value::as_str) {
            Some("hello") => self.sync = true,
            Some("inv") => {
                if v.get("h").and_then(Value::as_i64) != self.hash {
                    self.hash = None; // reloaded by the main loop
                }
            }
            Some("focus") => self.on_focus(v),
            _ => {}
        }
    }

    fn on_focus(&mut self, v: &Value) {
        let plugin = v.get("plugin").and_then(Value::as_str).unwrap_or("").to_string();
        let num = |k: &str, i: usize| v.get(k).and_then(|x| x.get(i)).and_then(Value::as_u64);
        let at = if let (Some(track), Some(slot)) = (num("fx", 0), num("fx", 1)) {
            At::Fx { track: track as u32, slot: slot as u32 }
        } else if let Some(ch) = v.get("ch").and_then(Value::as_u64) {
            At::Gen { ch: ch as u32 }
        } else {
            return; // mixer, channel rack, playlist: keep the last plugin
        };
        if plugin.is_empty() || is_self(&plugin) {
            return;
        }
        // the inventory's name wins; an unknown place means the project changed
        let known = self.b.view().targets.iter().find(|t| t.key.at == at).map(|t| t.key.clone());
        if known.is_none() {
            self.hash = None;
        }
        let key = known.unwrap_or(Key { at, plugin });
        self.b.update(|v| v.focus = Some(key));
    }

    fn connect(&mut self) {
        match self.call(json!({"cmd": "ping"})) {
            Ok(r) if r.get("proto").and_then(Value::as_u64) == Some(PROTO) => {
                if self.b.view().link != Link::Up {
                    log("fl: script answered, protocol 4");
                    self.b.update(|v| v.link = Link::Up);
                    self.flush();
                }
                if r.get("h").and_then(Value::as_i64) != self.hash {
                    self.load_inventory();
                }
                if self.b.view().focus.is_none() {
                    if let Ok(f) = self.call(json!({"cmd": "focus"})) {
                        self.on_focus(&f);
                    }
                }
            }
            Ok(r) => self.down(format!(
                "{OLD_SCRIPT}{}, install plugin/fl/device_synthexis_bridge.py",
                r.get("proto").map_or("?".into(), Value::to_string)
            )),
            Err(e) => {
                let setting_up = self.b.view().link == Link::Waiting && self.opened.elapsed() < SETUP;
                if !(setting_up && e == NO_ANSWER) {
                    self.down(e);
                }
            }
        }
    }

    fn down(&mut self, why: String) {
        if self.b.view().link != Link::Down(why.clone()) {
            log(&format!("fl: {why}"));
            self.hash = None;
            self.b.update(|v| {
                v.link = Link::Down(why);
                v.loaded = false;
            });
        }
    }

    fn load_inventory(&mut self) {
        for _ in 0..3 {
            match self.inventory_once() {
                Ok(Some((h, targets))) => {
                    self.hash = Some(h);
                    self.cache.retain(|k, _| targets.iter().any(|t| t.key == *k));
                    self.b.update(|v| {
                        let at = v.focus.as_ref().map(|f| f.at);
                        v.focus = at.and_then(|at| targets.iter().find(|t| t.key.at == at).map(|t| t.key.clone()));
                        v.targets = targets;
                        v.loaded = true;
                    });
                    return;
                }
                Ok(None) => continue, // project changed while paging
                Err(e) => return self.down(e),
            }
        }
    }

    fn inventory_once(&mut self) -> Result<Option<(i64, Vec<Target>)>, String> {
        let (mut out, mut start, mut hash) = (Vec::new(), 0u64, None);
        loop {
            let r = self.call(json!({"cmd": "inv", "start": start, "budget": PAGE}))?;
            let h = r.get("h").and_then(Value::as_i64);
            if hash.is_some() && h != hash {
                return Ok(None);
            }
            hash = h;
            for row in r.get("rows").and_then(Value::as_array).into_iter().flatten() {
                if let Some(k) = parse_row(row) {
                    out.push(k);
                }
            }
            match r.get("next").and_then(Value::as_u64) {
                Some(n) => start = n,
                None => return Ok(Some((hash.unwrap_or(0), out))),
            }
        }
    }

    /// Sends the latest value of every parameter queued so far (the camera
    /// runs at 30 Hz, a slow MIDI round must not pile up stale values); while
    /// the link is down they wait and go out once it is up. No reply: the
    /// script answers "rec" frames without an id with nothing. Each frame
    /// names the plugin, and the script writes only when that plugin still
    /// sits in the slot (a route never drives a plugin that replaced it).
    fn write(&mut self, first: (Key, u32, f32), more: &Receiver<(Key, u32, f32)>) {
        for (k, i, v) in std::iter::once(first).chain(more.try_iter()) {
            match self.unsent.iter_mut().find(|(uk, ui, _)| *uk == k && *ui == i) {
                Some(slot) => slot.2 = v,
                None => self.unsent.push((k, i, v)),
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        if self.b.view().link != Link::Up {
            return;
        }
        for (k, i, v) in std::mem::take(&mut self.unsent) {
            let mut msg = json!({"cmd": "rec", "p": k.plugin, "i": i, "v": (v * 10_000.0).round() / 10_000.0});
            match k.at {
                At::Fx { track, slot } => msg["fx"] = json!([track, slot]),
                At::Gen { ch } => msg["ch"] = json!(ch),
            }
            self.send(&msg);
        }
    }

    fn load_params(&mut self, key: Key) {
        if self.b.view().params.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let res = match self.cache.get(&key) {
            Some(p) => Ok(p.clone()),
            None => self.plist(&key),
        };
        if let Ok(p) = &res {
            self.cache.insert(key.clone(), p.clone());
        }
        self.b.set_params(key, res);
    }

    fn plist(&mut self, key: &Key) -> Result<Vec<(u32, String)>, String> {
        let (index, slot) = match key.at {
            At::Fx { track, slot } => (i64::from(track), i64::from(slot)),
            At::Gen { ch } => (i64::from(ch), -1),
        };
        let t0 = Instant::now();
        let (mut out, mut start) = (Vec::new(), 0u64);
        loop {
            let r = self.call(json!({"cmd": "plist", "index": index, "slot": slot, "start": start, "budget": PAGE}))?;
            if r.get("plugin").and_then(Value::as_str) != Some(key.plugin.as_str()) {
                self.hash = None;
                return Err(format!("{} is no longer in that slot", key.plugin));
            }
            for p in r.get("p").and_then(Value::as_array).into_iter().flatten() {
                if let (Some(i), Some(n)) = (p.get(0).and_then(Value::as_u64), p.get(1).and_then(Value::as_str)) {
                    if !is_placeholder(n) {
                        out.push((i as u32, n.to_string()));
                    }
                }
            }
            match r.get("next").and_then(Value::as_u64) {
                Some(n) => start = n,
                None => break,
            }
        }
        log(&format!(
            "fl: {} parameters of {} in {:.0} ms",
            out.len(),
            key.label(),
            t0.elapsed().as_secs_f64() * 1e3
        ));
        Ok(out)
    }
}

/// ["fx", track, slot, plugin, user, track name] or
/// ["gen", channel, plugin, user, channel name, target track, track name].
fn parse_row(row: &Value) -> Option<Target> {
    let s = |i: usize| row.get(i).and_then(Value::as_str);
    let n = |i: usize| row.get(i).and_then(Value::as_u64).map(|x| x as u32);
    let (key, track, name) = match s(0)? {
        "fx" => {
            let track = n(1)?;
            (Key { at: At::Fx { track, slot: n(2)? }, plugin: s(3)?.to_string() }, track, s(5))
        }
        "gen" => (Key { at: At::Gen { ch: n(1)? }, plugin: s(2)?.to_string() }, n(5).unwrap_or(0), s(6)),
        _ => return None,
    };
    let t = Target { key, track, track_name: name.unwrap_or("").to_string() };
    (!t.key.plugin.is_empty() && !is_self(&t.key.plugin)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trip_and_label() {
        let k = Key { at: At::Fx { track: 0, slot: 1 }, plugin: "Portal".into() };
        assert_eq!(k.id(), "fx:0:1:Portal");
        assert_eq!(Key::parse(&k.id()), Some(k.clone()));
        assert_eq!(k.label(), "Portal, Master slot 1");
        let g = Key { at: At::Gen { ch: 1 }, plugin: "Serum 2: Pad".into() };
        assert_eq!(Key::parse(&g.id()), Some(g.clone()));
        assert_eq!(g.label(), "Serum 2: Pad, Channel 1");
        let t = Key { at: At::Fx { track: 3, slot: 0 }, plugin: "Portal".into() };
        assert_eq!(t.label(), "Portal, Track 3 slot 0");
        assert_eq!(Key::parse("Xfer Records/Serum 2"), None);
        assert_eq!(Key::parse(""), None);
    }

    #[test]
    fn rows_skip_synthexis() {
        assert_eq!(parse_row(&json!(["fx", 2, 0, "synthexis", "", "Insert 2"])), None);
        assert_eq!(parse_row(&json!(["gen", 4, "Synthexis", "", "x", 0, "Master"])), None);
        let row = |t: &Target| (t.key.clone(), t.track, t.track_label(), t.is_sound());
        let serum = Key { at: At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        assert_eq!(
            parse_row(&json!(["gen", 1, "Serum 2", "", "Serum 2", 3, "Pads"])).as_ref().map(row),
            Some((serum, 3, "Track 3 · Pads".into(), true))
        );
        let portal = Key { at: At::Fx { track: 2, slot: 4 }, plugin: "Portal".into() };
        assert_eq!(
            parse_row(&json!(["fx", 2, 4, "Portal", "", "Insert 2"])).as_ref().map(row),
            Some((portal, 2, "Track 2".into(), false))
        );
    }

    #[test]
    fn deframer_splits_and_joins_packets() {
        let mut d = Deframer::default();
        let mut got = Vec::new();
        let mut keep = |f: &[u8]| got.extend(json_payload(f).map(<[u8]>::to_vec));
        d.feed(&[0x00, 0xF0, 0x7D, b'{', b'}'], &mut keep);
        d.feed(&[0xF7, 0xF0, 0x7E, b'x', 0xF7, 0xF0, 0x7D, b'1', 0xF7], &mut keep);
        assert_eq!(got, vec![b"{}".to_vec(), b"1".to_vec()]);
    }

    #[test]
    fn answers_device_inquiry_only() {
        // FL sent exactly this in probes/fl-midi-autolink (M1)
        let want = [0xF0, 0x7E, 0x7F, 0x06, 0x02, 0x7D, 0x53, 0x58, 0x46, 0x4C, 0x04, 0, 0, 0, 0xF7];
        assert_eq!(identity_reply(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]).as_deref(), Some(&want[..]));
        assert_eq!(identity_reply(&[0xF0, 0x7E, 0x00, 0x06, 0x01, 0xF7]).as_deref(), Some(&want[..]));
        assert!(IDENTITY.iter().all(|b| *b < 0x80));
        assert_eq!(identity_reply(&[0xF0, 0x7E, 0x7F, 0x06, 0x02, 0xF7]), None);
        assert_eq!(identity_reply(&[0xF0, 0x7D, b'{', b'}', 0xF7]), None);
        assert_eq!(json_payload(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]), None);
    }

    #[test]
    fn script_declares_our_identity() {
        // the header line FL matches by prefix against IDENTITY
        let script = include_str!("../../fl/device_synthexis_bridge.py");
        let header = |key: &str| script.lines().find_map(|l| l.strip_prefix(key)).unwrap_or("");
        let ids = header("# supportedHardwareIds=");
        let id: Vec<u8> = ids.split(' ').filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        assert!(id.len() >= 5 && id.len() == ids.split(' ').count() && IDENTITY.starts_with(&id), "{ids}");
        let names = header("# supportedDevices=");
        assert!(names.split(',').any(|n| n == PORT), "{names}");
    }

    #[test]
    fn frames_are_seven_bit() {
        let f = frame(&json!({"plugin": "Wärme"}));
        assert!(f[1..f.len() - 1].iter().all(|b| *b < 0x80));
        assert_eq!(&f[2..f.len() - 1], br#"{"plugin":"W\u00e4rme"}"#);
    }

    #[test]
    fn placeholders() {
        for n in ["CC 74", "CC0 Chan 1", "MIDI CC 7", "MIDI CC 7|0", "MIDI CC", "Midi CC 64 (Sustain Pedal)",
            "Reserved4", "Reserved", "X (Reserved)", "Aftertouch Chan 16", "Internal", "unused"]
        {
            assert!(is_placeholder(n), "{n}");
        }
        for n in ["Cutoff", "CCW Rotation", "Chan 1 Level", "Receive Midi", "Param 3", "1", "MIDI Octaves"] {
            assert!(!is_placeholder(n), "{n}");
        }
    }
}
