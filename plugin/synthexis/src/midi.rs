//! synthexis' own MIDI (spec midi.*): parts, what a finger or a shape
//! plays (a clip of steps, chords in the key, one note), and the player
//! that turns a route's trigger edges into notes in host time.
//!
//! Notes leave through the FL bridge: the script plays them on the FL
//! channel the part names with `channels.midiNoteOn`, so no MIDI port or
//! wrapper setting is involved (probes/fl-midi-notes/RESULT.md).
//!
//! Time: process() publishes the host transport once per block
//! (`Transport::store`, lock-free); the player thread maps the host clock
//! to beats from it. While the host stops, the player runs on its own
//! clock at the last tempo, so a pattern still plays in a live jam. Clips
//! are bar-aligned: step n of a clip plays on the n-th 1/16 of the bar, so
//! a finger shown late joins the pattern in time instead of restarting it.
//! A host loop of whole bars is seamless: a playing part counts its own
//! bars on, so a progression or a long clip moves on in a one-bar loop.
//!
//! Pong's wall hits play parts too (`Player::hits`, design pong-presets 6):
//! each hit is a voice of its own next to the route-held play. A sounding
//! note counts its owners (part, voice), so a hit on a pitch a finger holds
//! re-strikes it and the finger's note outlives the hit. Hit note-ons share
//! a process-wide budget; route-held notes never ask it.

use std::collections::HashMap;
use std::sync::atomic::{fence, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::fl::ChannelRow;
use crate::log::log;
use crate::route::Trigger;

/// Steps per bar in every clip (1/16 notes in 4/4).
pub const STEPS: u16 = 16;
pub const MAX_BARS: u8 = 8;
const TICK: Duration = Duration::from_millis(1);
/// The host block older than this means the host stopped calling
/// process(); the player's own clock takes over.
const STALE_NS: u64 = 250_000_000;

/// Host clock in ns (mach_absolute_time, the camera's timebase too).
#[allow(unsafe_code)]
pub fn now_ns() -> u64 {
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }
    static TB: OnceLock<(u64, u64)> = OnceLock::new();
    let (n, d) = *TB.get_or_init(|| {
        let mut tb = Timebase { numer: 1, denom: 1 };
        // SAFETY: writes one mach_timebase_info_data_t into tb.
        unsafe { mach_timebase_info(&mut tb) };
        (u64::from(tb.numer.max(1)), u64::from(tb.denom.max(1)))
    });
    // SAFETY: no arguments, reads the monotonic clock.
    let t = unsafe { mach_absolute_time() };
    (u128::from(t) * u128::from(n) / u128::from(d)) as u64
}

// --- model --------------------------------------------------------------

/// Key and scale of the MIDI roles (spec midi.scale).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// 0 = C .. 11 = B.
    pub root: u8,
    pub scale: Scale,
}

impl Default for Key {
    fn default() -> Self {
        Key { root: 9, scale: Scale::Minor }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    HarmonicMinor,
    MajorPentatonic,
    MinorPentatonic,
}

impl Scale {
    pub const ALL: [Scale; 9] = [
        Scale::Major,
        Scale::Minor,
        Scale::Dorian,
        Scale::Phrygian,
        Scale::Lydian,
        Scale::Mixolydian,
        Scale::HarmonicMinor,
        Scale::MajorPentatonic,
        Scale::MinorPentatonic,
    ];

    pub fn steps(self) -> &'static [u8] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "major",
            Scale::Minor => "minor",
            Scale::Dorian => "dorian",
            Scale::Phrygian => "phrygian",
            Scale::Lydian => "lydian",
            Scale::Mixolydian => "mixolydian",
            Scale::HarmonicMinor => "harmonic minor",
            Scale::MajorPentatonic => "major pentatonic",
            Scale::MinorPentatonic => "minor pentatonic",
        }
    }

    pub fn parse(s: &str) -> Option<Scale> {
        let s = s.trim().to_lowercase().replace(['_', '-'], " ");
        Scale::ALL.into_iter().find(|x| x.name() == s).or(match s.as_str() {
            "moll" | "minor scale" | "aeolian" => Some(Scale::Minor),
            "dur" | "ionian" => Some(Scale::Major),
            _ => None,
        })
    }
}

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// FL's note name: MIDI 60 is "C5" in FL (one octave above the usual C4).
pub fn note_name(pitch: u8) -> String {
    format!("{}{}", NAMES[usize::from(pitch % 12)], pitch / 12)
}

/// A note name in FL's naming ("C5" = 60, "F#3" = 42, "Bb2"); None when
/// it is not one.
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' | 'H' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (acc, oct) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') if rest.len() > 1 => (-1, &rest[1..]),
        _ => (0, rest.as_str()),
    };
    let oct: i32 = oct.parse().ok()?;
    let p = oct * 12 + base + acc;
    u8::try_from(p).ok().filter(|p| *p < 128)
}

/// "A", "F#", "Bb", "h" (German B) as 0..11.
pub fn parse_root(s: &str) -> Option<u8> {
    let p = parse_note(&format!("{}5", s.trim()))?;
    Some(p % 12)
}

/// Key from frame tokens ("key:A", "scale:minor"); the scale alone keeps
/// the root, the root alone keeps the scale.
pub fn key_from(tokens: &[String], old: Key) -> Option<Key> {
    let root = tokens.iter().find_map(|t| t.strip_prefix("key:")).and_then(parse_root);
    let scale = tokens.iter().find_map(|t| t.strip_prefix("scale:")).and_then(Scale::parse);
    (root.is_some() || scale.is_some()).then(|| Key { root: root.unwrap_or(old.root), scale: scale.unwrap_or(old.scale) })
}

pub fn key_name(k: Key) -> String {
    format!("{} {}", NAMES[usize::from(k.root % 12)], k.scale.name())
}

/// The chord on scale degree `degree` (0 = I) with `size` notes stacked in
/// thirds of the scale, its root in FL octave `octave` whatever the key's
/// root, so a progression stays in one register (Am = A4 C5 E5, F = F4 A4
/// C5 in octave 4).
pub fn chord(key: Key, degree: u8, size: u8, octave: u8) -> Vec<u8> {
    let st = key.scale.steps();
    let n = st.len();
    let d = usize::from(degree);
    let abs = |i: usize| i32::from(st[i % n]) + 12 * (i / n) as i32;
    let root = i32::from(octave) * 12 + (i32::from(key.root % 12) + abs(d)) % 12;
    (0..usize::from(size.clamp(1, 6)))
        .filter_map(|k| {
            let p = root + abs(d + 2 * k) - abs(d);
            u8::try_from(p).ok().filter(|p| *p < 128)
        })
        .collect()
}

/// "Am", "F", "B°", "G7" for the chord on `degree`.
pub fn chord_name(key: Key, degree: u8, size: u8) -> String {
    let notes = chord(key, degree, size.max(3), 4);
    let (Some(&r), Some(&t), Some(&f)) = (notes.first(), notes.get(1), notes.get(2)) else {
        return "?".into();
    };
    let q = match (t - r, f - r) {
        (4, 7) => "",
        (3, 7) => "m",
        (3, 6) => "°",
        (4, 8) => "+",
        (2, 7) => "sus2",
        (5, 7) => "sus4",
        _ => "",
    };
    let seventh = if size >= 4 { "7" } else { "" };
    format!("{}{q}{seventh}", NAMES[usize::from(r % 12)])
}

/// Roman numeral of a degree in the key ("i", "VI", "ii°").
pub fn degree_name(key: Key, degree: u8) -> String {
    const R: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
    let n = chord_name(key, degree, 3);
    let r = R[usize::from(degree) % 7];
    if n.contains('°') {
        format!("{}°", r.to_lowercase())
    } else if n.ends_with('m') {
        r.to_lowercase()
    } else {
        r.to_string()
    }
}

/// One hit in a clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Position in 1/16 from the clip start.
    pub at: u16,
    /// Length in 1/16, at least 1.
    pub len: u16,
    pub pitch: u8,
    pub vel: u8,
    /// Hits inside the step (1 = one, 2 = two 1/32, 3 = a triplet ...).
    #[serde(default = "one")]
    pub ratchet: u8,
    /// Ticks (1/96 beat, 0..23) after the step's start: a strum or a
    /// laid-back note moved in the piano roll.
    #[serde(default)]
    pub nudge: u8,
}

fn one() -> u8 {
    1
}

/// What a part plays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Play {
    /// A loop of 1..8 bars, 16 steps per bar. `rows`: the notes the
    /// editor shows as lanes even without a hit on them.
    Clip {
        bars: u8,
        steps: Vec<Step>,
        #[serde(default)]
        rows: Vec<u8>,
    },
    /// Chords in the key, one per bar in turn while the part plays (one
    /// degree = one chord held); `strum` ticks (1/96 beat) between its
    /// notes from the lowest up, 0 = all at once.
    Chords {
        degrees: Vec<u8>,
        size: u8,
        octave: u8,
        vel: u8,
        #[serde(default)]
        strum: u8,
    },
    /// One note held while the part plays.
    Note { pitch: u8, vel: u8 },
}

/// When a part plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// While its trigger is on.
    Gate,
    /// Each trigger switches it on or off.
    Latch,
    /// Each trigger plays it once to its end.
    Once,
}

/// Where a part starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// At once.
    Now,
    /// On the next 1/16.
    Sixteenth,
    /// On the next beat.
    Beat,
    /// On the next bar.
    Bar,
}

/// An FL channel rack channel, by index and the name it had, so a moved
/// channel can be found again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub index: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: u64,
    pub name: String,
    pub channel: Option<Channel>,
    pub play: Play,
    pub mode: Mode,
    pub start: Start,
}

/// GM drum notes (FPC's default kit): kick 36, snare 38, clap 39,
/// closed hat 42, open hat 46.
pub const KICK: u8 = 36;
pub const SNARE: u8 = 38;
pub const CLAP: u8 = 39;
pub const HAT: u8 = 42;

/// Built-in starting points (`ui.pattern_editor` templates).
pub fn template(name: &str) -> Option<Play> {
    let hits = |pitch: u8, at: &[(u16, u8, u8)]| Play::Clip {
        bars: 1,
        steps: at.iter().map(|&(at, vel, ratchet)| Step { at, len: 1, pitch, vel, ratchet, nudge: 0 }).collect(),
        rows: vec![pitch],
    };
    let n = name.trim().to_lowercase().replace(['-', '_'], " ");
    Some(match n.as_str() {
        "hi hat basic" | "hi hat" | "hihat" | "hi hat pattern" | "hats" => {
            hits(HAT, &(0..8).map(|i| (i * 2, if i % 2 == 0 { 110 } else { 80 }, 1)).collect::<Vec<_>>())
        }
        "trap roll" | "trap hats" | "roll" => hits(
            HAT,
            &[(0, 110, 1), (1, 70, 1), (2, 90, 1), (3, 70, 2), (4, 110, 1), (5, 70, 1), (6, 90, 3), (7, 70, 1),
              (8, 110, 1), (9, 70, 1), (10, 90, 1), (11, 70, 2), (12, 110, 1), (13, 80, 4), (14, 90, 1), (15, 80, 2)],
        ),
        "snare fill" | "fill" | "snare roll" => {
            hits(SNARE, &[(4, 100, 1), (10, 80, 1), (12, 90, 1), (13, 95, 1), (14, 105, 2), (15, 120, 3)])
        }
        "kick" | "kick pattern" => hits(KICK, &[(0, 120, 1), (6, 100, 1), (8, 115, 1)]),
        "clap" | "clap pattern" | "clap 2 and 4" => hits(CLAP, &[(4, 110, 1), (12, 110, 1)]),
        "i v vi iv" | "pop" | "axis" => Play::Chords { degrees: vec![0, 4, 5, 3], size: 3, octave: 4, vel: 96, strum: 0 },
        _ => return None,
    })
}

/// The notes a Chords part plays, as clip steps: each chord held for its
/// bar, strummed by `nudge` (the piano roll's "edit as notes").
pub fn chords_as_clip(key: Key, play: &Play) -> Option<Play> {
    let Play::Chords { degrees, size, octave, vel, strum } = play else { return None };
    let mut steps = Vec::new();
    for (bar, d) in degrees.iter().enumerate().take(usize::from(MAX_BARS)) {
        for (k, pitch) in chord(key, *d, *size, *octave).into_iter().enumerate() {
            let ticks = (k as u32 * u32::from(*strum)).min(23 * 15);
            steps.push(Step {
                at: bar as u16 * STEPS + (ticks / 24) as u16,
                len: STEPS - (ticks / 24) as u16,
                pitch,
                vel: *vel,
                ratchet: 1,
                nudge: (ticks % 24) as u8,
            });
        }
    }
    Some(Play::Clip { bars: degrees.len().clamp(1, usize::from(MAX_BARS)) as u8, steps, rows: Vec::new() })
}

/// GM drum notes the templates use.
const DRUMS: [u8; 8] = [36, 38, 39, 42, 44, 46, 49, 51];

/// Drum templates speak GM (FPC's kit); a Sampler or synth channel plays
/// one sound at its root, C5 (MIDI 60), so there they move to C5. A clip
/// with other notes stays as it is.
pub fn fit_to_channel(play: &mut Play, channel: &str) {
    let Play::Clip { steps, rows, .. } = play else { return };
    let drums = channel.to_lowercase().contains("fpc");
    if drums || !steps.iter().map(|s| s.pitch).chain(rows.iter().copied()).all(|p| DRUMS.contains(&p)) {
        return;
    }
    for s in steps.iter_mut() {
        s.pitch = 60;
    }
    *rows = vec![60];
}

pub const TEMPLATES: [&str; 6] = ["Hi-hat basic", "Trap roll", "Snare fill", "Kick", "Clap", "I-V-vi-IV"];

/// Hits spread as evenly as possible (Euclid), `k` of `n` steps.
pub fn euclid(k: u16, n: u16) -> Vec<bool> {
    let n = n.max(1);
    let k = k.min(n);
    (0..n).map(|i| k > 0 && (u32::from(i) * u32::from(k)) % u32::from(n) < u32::from(k)).collect()
}

/// What starts a part: a signal and its trigger ("right index shown" is
/// cam0.hand.R.shape with the index bit).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bind {
    pub signal: String,
    pub trigger: Trigger,
    pub part: u64,
}

/// Parts, key and binds, saved with the project (params `midi`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub version: u32,
    pub key: Key,
    pub parts: Vec<Part>,
    pub next_id: u64,
    #[serde(default)]
    pub binds: Vec<Bind>,
    /// Parts Pong's walls play: `unbind` keeps them, so a wall's part
    /// never goes with a finger.
    #[serde(default)]
    pub walls: Vec<u64>,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: 1, key: Key::default(), parts: Vec::new(), next_id: 1, binds: Vec::new(), walls: Vec::new() }
    }
}

impl Store {
    pub fn load(json: &str) -> Store {
        if json.trim().is_empty() {
            return Store::default();
        }
        serde_json::from_str(json).unwrap_or_else(|e| {
            log(&format!("midi parts: not read ({e})"));
            Store::default()
        })
    }

    pub fn save(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn add(&mut self, name: &str, play: Play, channel: Option<Channel>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let (mode, start) = match play {
            Play::Clip { .. } => (Mode::Gate, Start::Sixteenth),
            _ => (Mode::Gate, Start::Now),
        };
        self.parts.push(Part { id, name: name.to_string(), channel, play, mode, start });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Part> {
        self.parts.iter_mut().find(|p| p.id == id)
    }

    /// The part bound to (signal, trigger), if any.
    pub fn bound(&self, signal: &str, trigger: Trigger) -> Option<&Part> {
        self.binds.iter().find(|b| b.signal == signal && b.trigger == trigger).and_then(|b| self.get(b.part))
    }

    /// Binds (signal, trigger) to `part`, replacing what it played.
    pub fn bind(&mut self, signal: &str, trigger: Trigger, part: u64) {
        self.binds.retain(|b| !(b.signal == signal && b.trigger == trigger));
        self.binds.push(Bind { signal: signal.to_string(), trigger, part });
    }

    /// Unbinds (signal, trigger); its part goes with it unless another
    /// bind or a wall plays it too. Parts no bind uses yet (a sentence
    /// line before Keep) stay.
    pub fn unbind(&mut self, signal: &str, trigger: Trigger) {
        let hit = |b: &Bind| b.signal == signal && b.trigger == trigger;
        let old: Vec<u64> = self.binds.iter().filter(|b| hit(b)).map(|b| b.part).collect();
        self.binds.retain(|b| !hit(b));
        let (binds, walls) = (&self.binds, &self.walls);
        self.parts.retain(|p| !old.contains(&p.id) || binds.iter().any(|b| b.part == p.id) || walls.contains(&p.id));
    }

    /// Finds each part's channel again in `rows`, FL's channels now, after
    /// the rack changed (a channel inserted, deleted or moved, a project
    /// loaded). A part stays where its name still is, moves to the channel
    /// with its name, keeps a channel renamed in place (the same plugin at
    /// its index), else has none: its notes never reach another channel.
    /// An empty list (inventory not read yet) changes nothing. True when a
    /// channel changed; `Player::set_store` then ends the old notes.
    pub fn rebind_channels(&mut self, rows: &[ChannelRow]) -> bool {
        if rows.is_empty() {
            return false;
        }
        let mut changed = false;
        for part in &mut self.parts {
            let Some(c) = &part.channel else { continue };
            let fits = |r: &&ChannelRow| r.label() == c.name || (!r.plugin.is_empty() && r.plugin == c.name);
            let here = rows.iter().find(|r| r.index == c.index);
            if here.filter(fits).is_some() {
                continue;
            }
            let moved = rows.iter().find(|r| r.label() == c.name).or_else(|| rows.iter().find(fits));
            let renamed = here.filter(|r| !r.plugin.is_empty() && c.name.split(" · ").next() == Some(r.plugin.as_str()));
            part.channel = match (moved, renamed) {
                (Some(r), _) => Some(Channel { index: r.index, name: c.name.clone() }),
                (None, Some(r)) => Some(Channel { index: r.index, name: r.label() }),
                (None, None) => None,
            };
            changed = true;
        }
        changed
    }
}

// --- transport ------------------------------------------------------------

/// The host transport of the last block, written by process() without
/// locks (a sequence counter guards the reader against torn values).
#[derive(Default)]
pub struct Transport {
    seq: AtomicU64,
    playing: AtomicU64,
    tempo: AtomicU64,
    beats: AtomicU64,
    at_ns: AtomicU64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub playing: bool,
    pub tempo: f64,
    /// Beats at `at_ns`, when the host gave a position.
    pub beats: Option<f64>,
    pub at_ns: u64,
}

impl Transport {
    /// Audio thread: once per block.
    pub fn store(&self, playing: bool, tempo: Option<f64>, beats: Option<f64>, at_ns: u64) {
        self.seq.fetch_add(1, Ordering::Relaxed);
        // the odd count before the values: a reader that sees a new value
        // sees the count move
        fence(Ordering::Release);
        self.playing.store(u64::from(playing), Ordering::Relaxed);
        if let Some(t) = tempo.filter(|t| t.is_finite() && *t > 1.0) {
            self.tempo.store(t.to_bits(), Ordering::Relaxed);
        }
        self.beats.store(beats.filter(|b| b.is_finite()).map_or(u64::MAX, f64::to_bits), Ordering::Relaxed);
        self.at_ns.store(at_ns, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    pub fn load(&self) -> Pos {
        loop {
            let s0 = self.seq.load(Ordering::Acquire);
            let tempo = f64::from_bits(self.tempo.load(Ordering::Relaxed));
            let pos = Pos {
                playing: self.playing.load(Ordering::Relaxed) == 1,
                tempo: if tempo > 1.0 { tempo } else { 120.0 },
                beats: match self.beats.load(Ordering::Relaxed) {
                    u64::MAX => None,
                    b => Some(f64::from_bits(b)),
                },
                at_ns: self.at_ns.load(Ordering::Relaxed),
            };
            // the values are read before the second count (an Acquire load
            // alone lets them move after it, and a torn block passes)
            fence(Ordering::Acquire);
            if s0.is_multiple_of(2) && self.seq.load(Ordering::Relaxed) == s0 {
                return pos;
            }
            std::hint::spin_loop();
        }
    }
}

/// Host clock to beats: the host's position while it plays, else a clock
/// of our own at the current tempo that runs on from where the host was.
#[derive(Debug, Default)]
struct Clock {
    last: Option<(u64, f64, bool)>,
    /// The position jumped (host start, stop, loop or seek) since the
    /// player last looked: the beats it moved against the running clock
    /// (a loop back is about minus the loop's length) and whether the host
    /// started or stopped.
    jump: Option<(f64, bool)>,
}

impl Clock {
    /// Beats at `now`; notes a jump in `jump`.
    fn beats(&mut self, pos: &Pos, now: u64) -> f64 {
        let per_ns = pos.tempo / 60.0 / 1e9;
        let host = match pos.beats {
            Some(b) if pos.playing && now.saturating_sub(pos.at_ns) < STALE_NS => {
                Some(b + (now as f64 - pos.at_ns as f64) * per_ns)
            }
            _ => None,
        };
        // our own clock runs on from the last look, so a tempo change bends
        // it instead of moving every beat since it started
        let run_on = self.last.map(|(t, b, _)| b + (now as f64 - t as f64) * per_ns);
        let beats = host.or(run_on).unwrap_or(0.0);
        let playing = host.is_some();
        if let (Some((_, _, was)), Some(expect)) = (self.last, run_on) {
            let moved = beats - expect;
            if was != playing || moved.abs() > 0.05 {
                let (m, started) = self.jump.unwrap_or_default();
                self.jump = Some((m + moved, started || was != playing));
            }
        }
        self.last = Some((now, beats, playing));
        beats
    }
}

// --- player -----------------------------------------------------------------

/// Where notes go: (FL channel, pitch, velocity), velocity 0 = off.
pub trait Out: Send + Sync {
    fn notes(&self, ev: &[(u32, u8, u8)]);
}

/// The FL bridge as the note output.
pub struct Bridge;

impl Out for Bridge {
    fn notes(&self, ev: &[(u32, u8, u8)]) {
        crate::fl::bridge().notes(ev);
    }
}

#[derive(Debug, Default)]
struct Active {
    /// Routes holding the part now.
    holders: Vec<u64>,
    latched: bool,
    /// Whole bars, in 1/16, the part counts ahead of the host's position:
    /// a host loop back adds the loop, so the part plays on where it was.
    /// The steps below are the part's own (host 1/16 count + shift).
    shift: i64,
    /// First step and, for Once, the end.
    from: i64,
    until: Option<i64>,
    /// Last step scanned.
    scanned: Option<i64>,
    /// Chord or note started at this step (bar index for progressions).
    chord_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ev {
    beat: f64,
    part: u64,
    /// 0 = the part's route-held play, else a hit voice.
    voice: u64,
    ch: u32,
    pitch: u8,
    vel: u8,
}

/// A note on now for one owner (part, voice).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sound {
    part: u64,
    voice: u64,
    ch: u32,
    pitch: u8,
}

/// A wall hit on a clip part: the clip once from its step 0, scanned a
/// step ahead like a route-held clip.
#[derive(Debug)]
struct HitVoice {
    id: u64,
    part: u64,
    /// Host beat of the clip's step 0 (a seamless host loop moves it).
    base: f64,
    /// Last clip step queued.
    scanned: Option<i64>,
    /// Velocity factor, 1 = the part's own.
    gain: f32,
}

/// One wall hit for `Player::hits`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitReq {
    pub part: u64,
    /// The shot's strength, 0..1.
    pub strength: f32,
    /// Host clock (`now_ns`) of the wall hit.
    pub at_ns: u64,
}

/// A hit note lasts at least this long, whatever the tempo.
const MIN_HIT_NS: u64 = 40_000_000;
/// Clip voices per part at once; one more ends the oldest.
const MAX_CLIP_VOICES: usize = 2;
/// Hit note-ons per second over all instances, and the burst on top
/// (h: FL's JSON notes path was measured at 30 notes/s).
const HIT_RATE: f64 = 40.0;
const HIT_BURST: f64 = 8.0;

/// Token bucket for hit note-ons; route-held notes never ask it.
struct Budget {
    /// Tokens and the host time they were counted at.
    bucket: Mutex<(f64, Option<u64>)>,
    dropped: AtomicU64,
}

impl Budget {
    const fn new() -> Budget {
        Budget { bucket: parking_lot::const_mutex((HIT_BURST, None)), dropped: AtomicU64::new(0) }
    }

    /// Takes a token at host time `now`; false (and counted) when none is
    /// left. Time never runs back: players read the clock before the lock.
    fn take(&self, now: u64) -> bool {
        let mut b = self.bucket.lock();
        let (tokens, last) = &mut *b;
        if let Some(l) = *last {
            *tokens = (*tokens + now.saturating_sub(l) as f64 * HIT_RATE / 1e9).min(HIT_BURST);
        }
        *last = Some(last.map_or(now, |l| l.max(now)));
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}

/// Shared by every instance in the process: FL takes the notes of all of
/// them through one script.
static BUDGET: Budget = Budget::new();

/// Hit note-ons over the budget since the process started (not sent).
pub fn hits_dropped() -> u64 {
    BUDGET.dropped.load(Ordering::Relaxed)
}

#[derive(Default)]
struct State {
    store: Store,
    active: HashMap<u64, Active>,
    /// Notes waiting for their beat, on and off.
    queue: Vec<Ev>,
    /// Notes on now, one entry per owner.
    sounding: Vec<Sound>,
    /// Clip hit voices, oldest first.
    hits: Vec<HitVoice>,
    /// Hits so far per Chords part: the next hit plays the next degree.
    chord_hits: HashMap<u64, usize>,
    /// Last hit voice id handed out (0 is the route-held play).
    next_voice: u64,
    clock: Clock,
    live: bool,
    /// Beats the notes leave early (host latency compensation, R2).
    lead: f64,
    /// The hit budget; None = the process-wide one.
    budget: Option<&'static Budget>,
}

impl State {
    fn budget(&self) -> &'static Budget {
        self.budget.unwrap_or(&BUDGET)
    }
}

/// One per plugin instance. Its thread wakes every millisecond while a
/// part plays and sleeps otherwise.
pub struct Player {
    state: Mutex<State>,
    pub transport: Transport,
    out: Arc<dyn Out>,
    wake: Sender<()>,
}

impl Player {
    pub fn new(out: Arc<dyn Out>) -> Arc<Player> {
        let (wake, rx) = bounded(1);
        let p = Arc::new(Player { state: Mutex::new(State { live: true, ..State::default() }), transport: Transport::default(), out, wake });
        let weak = Arc::downgrade(&p);
        if let Err(e) = std::thread::Builder::new().name("synthexis-midi".into()).spawn(move || run(weak, rx)) {
            log(&format!("midi: player not started: {e}"));
        }
        p
    }

    /// A player without a thread; tests call `tick` with their own clock.
    /// Its hit budget is its own, so parallel tests with made-up clocks do
    /// not drain each other's.
    #[cfg(test)]
    pub fn manual(out: Arc<dyn Out>) -> Arc<Player> {
        let (wake, rx) = bounded(1);
        std::mem::forget(rx);
        let budget: &'static Budget = Box::leak(Box::new(Budget::new()));
        Arc::new(Player { state: Mutex::new(State { live: true, budget: Some(budget), ..State::default() }), transport: Transport::default(), out, wake })
    }

    pub fn store(&self) -> Store {
        self.state.lock().store.clone()
    }

    /// Replaces parts and key (an edit in the window, a load); parts that
    /// are gone or changed channel end every voice, hits included. A
    /// playing part whose note or chords changed (or its key) ends what it
    /// played and plays the new content at once; an edited clip plays on.
    pub fn set_store(&self, store: Store) {
        self.with(move |st, out| {
            // every part with a voice: route-held, hits, notes queued or on
            let mut ids: Vec<u64> = st
                .active
                .keys()
                .copied()
                .chain(st.hits.iter().map(|v| v.part))
                .chain(st.queue.iter().map(|e| e.part))
                .chain(st.sounding.iter().map(|s| s.part))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            let mut gone = Vec::new();
            let mut changed = Vec::new();
            for id in ids {
                match (st.store.get(id), store.get(id)) {
                    (Some(a), Some(b)) if a.channel == b.channel => {
                        let clips = matches!((&a.play, &b.play), (Play::Clip { .. }, Play::Clip { .. }));
                        let key = st.store.key != store.key && matches!(b.play, Play::Chords { .. });
                        if st.active.contains_key(&id) && !clips && (a.play != b.play || key) {
                            changed.push(id);
                        }
                    }
                    _ => gone.push(id),
                }
            }
            st.store = store;
            for id in gone {
                end_part(st, id, out);
            }
            // a clip after a note or chord starts on the next step, the
            // steps already past stay unplayed
            let now = st.clock.last.map_or(0, |(_, b, _)| step_of(b + st.lead));
            for id in changed {
                end_route(st, id, out);
                if let Some(a) = st.active.get_mut(&id) {
                    a.chord_at = None;
                    a.scanned = Some(now + a.shift);
                }
            }
        });
    }

    /// A route's trigger went on (`on`) or off for `part`.
    pub fn trigger(&self, route: u64, part: u64, on: bool) {
        // the clock read under the lock: a tick in between cannot be newer
        // and turn this trigger's time back
        self.with(|st, out| trigger_at(st, &self.transport.load(), now_ns(), route, part, on, out));
        let _ = self.wake.try_send(());
    }

    /// The route is gone (removed, discarded): it holds nothing any more.
    pub fn release_route(&self, route: u64) {
        let parts: Vec<u64> = self.state.lock().active.iter().filter(|(_, a)| a.holders.contains(&route)).map(|(id, _)| *id).collect();
        for p in parts {
            self.trigger(route, p, false);
        }
    }

    /// Wall hits from one Pong step, sent as one notes frame. Each starts
    /// a voice of its part (mode ignored, start as the part's `start`):
    /// a note or the next chord of the progression for `hit_len_beats`
    /// (at least 40 ms), a clip once to its end. With `vel_from_strength`
    /// velocities scale by 0.55 + 0.45 x strength. Returns the hits that
    /// started; 0 while Live is off. Parts without a channel play nothing.
    pub fn hits(&self, hits: &[HitReq], vel_from_strength: bool, hit_len_beats: f64) -> usize {
        let n = self.with(|st, out| hits_at(st, &self.transport.load(), now_ns(), hits, vel_from_strength, hit_len_beats, out));
        if n > 0 {
            let _ = self.wake.try_send(());
        }
        n
    }

    /// Ends every hit voice now (Pong off, a preset load); route-held
    /// notes play on.
    pub fn end_hits(&self) {
        self.with(end_hits);
    }

    /// Ends every voice of these parts, route-held and hits, whatever
    /// their mode (a Latch or a Once too).
    pub fn end_parts(&self, parts: &[u64]) {
        self.with(|st, out| {
            for p in parts {
                end_part(st, *p, out);
            }
        });
    }

    /// Live off, deactivate, unload: every note ends, nothing starts until
    /// Live is on again.
    pub fn set_live(&self, live: bool) {
        self.with(|st, out| {
            if st.live != live {
                st.live = live;
                if !live {
                    all_off(st, out);
                }
            }
        });
    }

    pub fn stop_all(&self) {
        self.with(all_off);
    }

    /// Part ids playing now (the window lights their rows).
    pub fn playing(&self) -> Vec<u64> {
        self.state.lock().active.keys().copied().collect()
    }

    /// Beats now on the player's clock (the window's playhead).
    pub fn beats_now(&self) -> f64 {
        let pos = self.transport.load();
        let now = now_ns();
        let st = self.state.lock();
        let per_ns = pos.tempo / 60.0 / 1e9;
        match (pos.beats, st.clock.last) {
            (Some(b), _) if pos.playing && now.saturating_sub(pos.at_ns) < STALE_NS => b + (now as f64 - pos.at_ns as f64) * per_ns,
            (_, Some((t, b, _))) => b + (now as f64 - t as f64) * per_ns,
            _ => 0.0,
        }
    }

    /// Beats on `part`'s own count, the playhead of its clip: the
    /// player's beats plus the bars it counted on in host loops.
    pub fn beats_of(&self, part: u64) -> f64 {
        let shift = self.state.lock().active.get(&part).map_or(0, |a| a.shift);
        self.beats_now() + shift as f64 / 4.0
    }

    /// Runs `f` on the state and sends its notes before the lock goes, so
    /// the player thread's, a trigger's and an edit's notes reach FL in
    /// the order the state changed: an off never overtakes its on.
    fn with<R>(&self, f: impl FnOnce(&mut State, &mut Vec<(u32, u8, u8)>) -> R) -> R {
        let mut st = self.state.lock();
        let mut out = Vec::new();
        let r = f(&mut st, &mut out);
        if !out.is_empty() {
            self.out.notes(&out);
        }
        r
    }

    /// Sends what is due at `now`; true while something plays or waits.
    pub fn tick(&self, now: u64) -> bool {
        self.with(|st, out| tick_at(st, &self.transport.load(), now, out))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.with(all_off);
    }
}

fn run(player: Weak<Player>, wake: Receiver<()>) {
    loop {
        let busy = match player.upgrade() {
            Some(p) => p.tick(now_ns()),
            None => return,
        };
        let wait = if busy { TICK } else { Duration::from_millis(200) };
        match wake.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn step_of(beats: f64) -> i64 {
    (beats * 4.0).floor() as i64
}

fn first_step(start: Start, beats: f64) -> i64 {
    let ceil = |x: f64| x.ceil() as i64;
    match start {
        Start::Now => step_of(beats),
        Start::Sixteenth => ceil(beats * 4.0 - 1e-9),
        Start::Beat => ceil(beats - 1e-9) * 4,
        Start::Bar => ceil(beats / 4.0 - 1e-9) * 16,
    }
}

fn clip_len(play: &Play) -> i64 {
    match play {
        Play::Clip { bars, .. } => i64::from((*bars).clamp(1, MAX_BARS)) * i64::from(STEPS),
        Play::Chords { degrees, .. } => 16 * degrees.len().max(1) as i64,
        Play::Note { .. } => 16,
    }
}

/// The notes of the clip steps at `at` whose 1/16 starts on `beat`, as
/// (beat, length, pitch, velocity), ratchets split.
fn step_notes(steps: &[Step], at: u16, beat: f64, mut f: impl FnMut(f64, f64, u8, u8)) {
    for h in steps.iter().filter(|h| h.at == at) {
        let r = h.ratchet.clamp(1, 8);
        let w = 0.25 / f64::from(r);
        for k in 0..r {
            let b = beat + f64::from(h.nudge.min(23)) / 96.0 + f64::from(k) * w;
            let dur = if r > 1 { w * 0.8 } else { (f64::from(h.len.max(1)) * 0.25 - 0.02).max(0.05) };
            f(b, dur, h.pitch, h.vel.clamp(1, 127));
        }
    }
}

/// A velocity times a hit's gain, kept in 1..127.
fn scaled(vel: u8, gain: f32) -> u8 {
    (f32::from(vel) * gain).round().clamp(1.0, 127.0) as u8
}

#[allow(clippy::too_many_arguments)]
fn trigger_at(st: &mut State, pos: &Pos, now: u64, route: u64, part: u64, on: bool, out: &mut Vec<(u32, u8, u8)>) {
    let Some(p) = st.store.get(part).cloned() else { return };
    if on && !st.live {
        return;
    }
    // a host jump since the last tick re-joins the running parts here, so
    // it cannot move where this part starts or ends
    let beats = look(st, pos, now, out);
    let a = st.active.entry(part).or_default();
    let was_held = !a.holders.is_empty();
    if on {
        if !a.holders.contains(&route) {
            a.holders.push(route);
        }
    } else {
        a.holders.retain(|r| *r != route);
    }
    let held = !a.holders.is_empty();
    let pressed = held && !was_held;
    // `scanned` stays: a Once pressed again while it plays must not queue
    // the step ahead a second time (Gate and Latch restart a fresh part)
    let restart = |a: &mut Active| {
        a.from = first_step(p.start, beats) + a.shift;
        a.chord_at = None;
        a.until = None;
    };
    let stop = match p.mode {
        Mode::Gate => {
            if pressed {
                restart(a);
            }
            !held
        }
        Mode::Latch => {
            if pressed && a.latched {
                true
            } else if pressed {
                restart(a);
                a.latched = true;
                false
            } else {
                !a.latched && !held
            }
        }
        Mode::Once => {
            if pressed {
                restart(a);
                a.until = Some(a.from + clip_len(&p.play));
            }
            a.until.is_none() && !held
        }
    };
    if stop {
        st.active.remove(&part);
        end_route(st, part, out);
    }
    tick_at(st, pos, now, out);
}

/// Queues one voice per hit and sends what is due at once; the hits that
/// started.
#[allow(clippy::too_many_arguments)]
fn hits_at(st: &mut State, pos: &Pos, now: u64, hits: &[HitReq], vel_from_strength: bool, hit_len: f64, out: &mut Vec<(u32, u8, u8)>) -> usize {
    if !st.live {
        return 0;
    }
    let beats = look(st, pos, now, out);
    let horizon = beats + st.lead;
    let per_ns = pos.tempo / 60.0 / 1e9;
    let len = if hit_len.is_finite() { hit_len } else { 0.0 }.max(MIN_HIT_NS as f64 * per_ns);
    let key = st.store.key;
    let mut started = 0;
    for h in hits {
        let Some(part) = st.store.get(h.part).cloned() else { continue };
        let Some(ch) = part.channel.as_ref().map(|c| c.index) else { continue };
        // quantized from the beat of the wall hit, which may lie a frame back
        let late = now.saturating_sub(h.at_ns).min(STALE_NS) as f64 * per_ns;
        let at = match part.start {
            Start::Now => horizon,
            s => first_step(s, beats - late) as f64 * 0.25,
        };
        let strength = if h.strength.is_finite() { h.strength.clamp(0.0, 1.0) } else { 1.0 };
        let gain = if vel_from_strength { 0.55 + 0.45 * strength } else { 1.0 };
        st.next_voice += 1;
        let voice = st.next_voice;
        let note = |st: &mut State, b: f64, pitch: u8, vel: u8| {
            st.queue.push(Ev { beat: b, part: part.id, voice, ch, pitch, vel: scaled(vel, gain) });
            st.queue.push(Ev { beat: b + len, part: part.id, voice, ch, pitch, vel: 0 });
        };
        match &part.play {
            Play::Note { pitch, vel } => note(st, at, *pitch, *vel),
            Play::Chords { degrees, size, octave, vel, strum } => {
                let n = st.chord_hits.entry(part.id).or_default();
                let Some(d) = degrees.get(*n % degrees.len().max(1)).copied() else { continue };
                *n += 1;
                for (k, pitch) in chord(key, d, *size, *octave).into_iter().enumerate() {
                    note(st, at + k as f64 * f64::from(*strum) / 96.0, pitch, *vel);
                }
            }
            Play::Clip { .. } => {
                let mine: Vec<u64> = st.hits.iter().filter(|v| v.part == part.id).map(|v| v.id).collect();
                for old in mine.iter().take((mine.len() + 1).saturating_sub(MAX_CLIP_VOICES)) {
                    end_voice(st, *old, out);
                }
                st.hits.push(HitVoice { id: voice, part: part.id, base: at, scanned: None, gain });
            }
        }
        started += 1;
    }
    tick_at(st, pos, now, out);
    started
}

/// Ends the notes of the owners `gone` picks: their queued notes are
/// dropped, their sounding ones lose that owner, and a note goes off (once)
/// when no owner is left.
fn release(st: &mut State, gone: impl Fn(u64, u64) -> bool, out: &mut Vec<(u32, u8, u8)>) {
    st.queue.retain(|e| !gone(e.part, e.voice));
    let (ended, keep): (Vec<Sound>, Vec<Sound>) = std::mem::take(&mut st.sounding).into_iter().partition(|s| gone(s.part, s.voice));
    st.sounding = keep;
    let mut offs: Vec<(u32, u8, u8)> = Vec::new();
    for s in ended {
        let off = (s.ch, s.pitch, 0);
        if !st.sounding.iter().any(|k| (k.ch, k.pitch) == (s.ch, s.pitch)) && !offs.contains(&off) {
            offs.push(off);
        }
    }
    out.extend(offs);
}

/// Ends a part's route-held play (its trigger off, a new chord, an edit);
/// its hit voices play on.
fn end_route(st: &mut State, part: u64, out: &mut Vec<(u32, u8, u8)>) {
    release(st, |p, v| p == part && v == 0, out);
}

/// Ends every voice of a part, hits included, and drops it from `active`.
fn end_part(st: &mut State, part: u64, out: &mut Vec<(u32, u8, u8)>) {
    st.active.remove(&part);
    st.hits.retain(|v| v.part != part);
    release(st, |p, _| p == part, out);
}

fn end_voice(st: &mut State, voice: u64, out: &mut Vec<(u32, u8, u8)>) {
    st.hits.retain(|v| v.id != voice);
    release(st, |_, v| v == voice, out);
}

fn end_hits(st: &mut State, out: &mut Vec<(u32, u8, u8)>) {
    st.hits.clear();
    release(st, |_, v| v != 0, out);
}

fn all_off(st: &mut State, out: &mut Vec<(u32, u8, u8)>) {
    st.active.clear();
    st.hits.clear();
    release(st, |_, _| true, out);
}

/// Beats at `now`, a host jump since the last look handled first.
fn look(st: &mut State, pos: &Pos, now: u64, out: &mut Vec<(u32, u8, u8)>) -> f64 {
    let beats = st.clock.beats(pos, now);
    if let Some((moved, started)) = st.clock.jump.take() {
        rejoin(st, beats, moved, started, out);
    }
    beats
}

/// The host jumped by `moved` beats. A loop back by whole bars while it
/// plays is seamless: every part counts its bars on (`shift`), so a
/// progression or a long clip moves on, a Once part keeps its length, and
/// what the clock sent past the loop point does not play twice; hit voices
/// move with the timeline. A seek, a start or a stop ends every note and
/// every hit voice and re-joins the parts at the new position, each keeping
/// its bar count and the rest of a Once.
fn rejoin(st: &mut State, beats: f64, moved: f64, started: bool, out: &mut Vec<(u32, u8, u8)>) {
    let bars = (moved / 4.0).round();
    let seamless = !started && (moved - bars * 4.0).abs() < 0.125;
    if seamless {
        // queued notes move with the timeline
        for e in &mut st.queue {
            e.beat += bars * 4.0;
        }
        for v in &mut st.hits {
            v.base += bars * 4.0;
        }
    } else {
        st.hits.clear();
        release(st, |_, _| true, out);
    }
    let now = step_of(beats + st.lead);
    for a in st.active.values_mut() {
        a.shift -= bars as i64 * i64::from(STEPS);
        if !seamless {
            a.scanned = None;
            a.chord_at = None;
            a.from = a.from.max(now + a.shift);
        }
    }
}

/// Queues the next steps of the clip hit voices; a voice past its clip's
/// end, or whose part is gone, has no channel or is no clip any more, ends.
fn scan_hits(st: &mut State, horizon: f64, out: &mut Vec<(u32, u8, u8)>) {
    let mut ended = Vec::new();
    let State { store, hits, queue, .. } = st;
    for v in hits.iter_mut() {
        let Some(p) = store.get(v.part) else {
            ended.push(v.id);
            continue;
        };
        let (Some(ch), Play::Clip { steps, .. }) = (p.channel.as_ref().map(|c| c.index), &p.play) else {
            ended.push(v.id);
            continue;
        };
        let len = clip_len(&p.play);
        let now_n = step_of(horizon - v.base);
        if now_n >= len {
            ended.push(v.id);
            continue;
        }
        // one step ahead, as route-held clips; never more than 4 behind
        let to = (now_n + 1).min(len - 1);
        let from = v.scanned.map_or(0, |s| s + 1).max(to - 4).max(0);
        for n in from..=to {
            step_notes(steps, n as u16, v.base + n as f64 * 0.25, |b, dur, pitch, vel| {
                let vel = scaled(vel, v.gain);
                queue.push(Ev { beat: b, part: v.part, voice: v.id, ch, pitch, vel });
                queue.push(Ev { beat: b + dur, part: v.part, voice: v.id, ch, pitch, vel: 0 });
            });
        }
        if to >= from {
            v.scanned = Some(to);
        }
    }
    for id in ended {
        end_voice(st, id, out);
    }
}

/// Plays one due note for its owner (part, voice). An on of a note that
/// already sounds re-strikes it (off, then on) and adds the owner; an off
/// removes its owner and goes out once no owner is left. A hit's on over
/// the budget is dropped, so its off finds no owner either.
fn sound(st: &mut State, e: &Ev, now: u64, out: &mut Vec<(u32, u8, u8)>) {
    let same = |s: &Sound| (s.ch, s.pitch) == (e.ch, e.pitch);
    let mine = st.sounding.iter().position(|s| same(s) && (s.part, s.voice) == (e.part, e.voice));
    if e.vel > 0 {
        if e.voice != 0 && !st.budget().take(now) {
            return;
        }
        if st.sounding.iter().any(same) {
            out.push((e.ch, e.pitch, 0));
        }
        if mine.is_none() {
            st.sounding.push(Sound { part: e.part, voice: e.voice, ch: e.ch, pitch: e.pitch });
        }
        out.push((e.ch, e.pitch, e.vel));
    } else if let Some(i) = mine {
        st.sounding.remove(i);
        if !st.sounding.iter().any(same) {
            out.push((e.ch, e.pitch, 0));
        }
    }
}

fn tick_at(st: &mut State, pos: &Pos, now: u64, out: &mut Vec<(u32, u8, u8)>) -> bool {
    let beats = look(st, pos, now, out);
    let horizon = beats + st.lead;
    let host_step = step_of(horizon);
    let key = st.store.key;
    let ids: Vec<u64> = st.active.keys().copied().collect();
    let mut gone = Vec::new();
    let mut done = Vec::new();
    for id in ids {
        let Some(part) = st.store.get(id).cloned() else {
            gone.push(id);
            continue;
        };
        let Some(ch) = part.channel.as_ref().map(|c| c.index) else { continue };
        let Some(a) = st.active.get_mut(&id) else { continue };
        let now_step = host_step + a.shift;
        if a.until.is_some_and(|u| now_step >= u) {
            done.push(id);
            continue;
        }
        let mut evs = Vec::new();
        match &part.play {
            Play::Clip { steps, .. } => {
                let len = clip_len(&part.play);
                // scan one step ahead so ratchets inside it are queued in time
                let to = now_step + 1;
                let from = a.scanned.map_or(a.from, |s| s + 1).max(a.from).max(to - 4);
                for s in from..=to {
                    if a.until.is_some_and(|u| s >= u) {
                        break;
                    }
                    step_notes(steps, s.rem_euclid(len) as u16, (s - a.shift) as f64 * 0.25, |b, dur, pitch, vel| {
                        evs.push(Ev { beat: b, part: id, voice: 0, ch, pitch, vel });
                        evs.push(Ev { beat: b + dur, part: id, voice: 0, ch, pitch, vel: 0 });
                    });
                }
                a.scanned = Some(to.max(a.scanned.unwrap_or(i64::MIN)));
            }
            Play::Chords { degrees, size, octave, vel, strum } => {
                // a progression moves on with the bar, one chord just holds;
                // only forward, so a block a little behind the last
                // extrapolation does not switch back
                let bar = if degrees.len() > 1 { now_step.div_euclid(16) } else { 0 };
                if now_step >= a.from && a.chord_at.is_none_or(|c| bar > c) {
                    a.chord_at = Some(bar);
                    let i = bar.rem_euclid(degrees.len().max(1) as i64) as usize;
                    let notes = degrees.get(i).map(|d| chord(key, *d, *size, *octave)).unwrap_or_default();
                    evs.push(Ev { beat: f64::NAN, part: id, voice: 0, ch, pitch: 0, vel: 0 });
                    for (k, n) in notes.into_iter().enumerate() {
                        let b = horizon + k as f64 * f64::from(*strum) / 96.0;
                        evs.push(Ev { beat: b, part: id, voice: 0, ch, pitch: n, vel: (*vel).clamp(1, 127) });
                    }
                }
            }
            Play::Note { pitch, vel } => {
                if now_step >= a.from && a.chord_at.is_none() {
                    a.chord_at = Some(0);
                    evs.push(Ev { beat: horizon, part: id, voice: 0, ch, pitch: *pitch, vel: (*vel).clamp(1, 127) });
                }
            }
        }
        for e in evs {
            if e.beat.is_nan() {
                // a new chord: the old one ends first, its strummed notes
                // still queued with it
                end_route(st, id, out);
            } else {
                st.queue.push(e);
            }
        }
    }
    for id in gone {
        end_part(st, id, out);
    }
    for id in done {
        st.active.remove(&id);
        end_route(st, id, out);
    }
    scan_hits(st, horizon, out);
    // due notes, in beat order; an off before the on of the same note
    st.queue.sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.vel.cmp(&b.vel)));
    let due = st.queue.iter().take_while(|e| e.beat <= horizon).count();
    for e in st.queue.drain(..due).collect::<Vec<_>>() {
        sound(st, &e, now, out);
    }
    !st.active.is_empty() || !st.queue.is_empty() || !st.sounding.is_empty() || !st.hits.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Rec(Mutex<Vec<(u32, u8, u8)>>);

    impl Out for Rec {
        fn notes(&self, ev: &[(u32, u8, u8)]) {
            self.0.lock().extend_from_slice(ev);
        }
    }

    const BEAT_NS: u64 = 500_000_000; // 120 BPM

    fn setup(play: Play, mode: Mode, start: Start) -> (Arc<Player>, Arc<Rec>, u64) {
        let rec = Arc::new(Rec::default());
        let p = Player::manual(rec.clone());
        let mut s = Store::default();
        let id = s.add("t", play, Some(Channel { index: 3, name: "FPC".into() }));
        if let Some(x) = s.get_mut(id) {
            x.mode = mode;
            x.start = start;
        }
        p.set_store(s);
        (p, rec, id)
    }

    /// Host playing from beat 0 at t = 1 s, one block per 10 ms up to `until_ns`.
    fn run_host(p: &Player, from_ns: u64, until_ns: u64) {
        let mut t = from_ns;
        while t <= until_ns {
            let beats = (t - 1_000_000_000) as f64 / BEAT_NS as f64;
            p.transport.store(true, Some(120.0), Some(beats), t);
            p.tick(t);
            t += 1_000_000;
        }
    }

    /// Like `run_host`, the host looping every `loop_beats` and handing a
    /// block every `block_ns`; the player extrapolates in between.
    fn run_loop(p: &Player, from_ns: u64, until_ns: u64, block_ns: u64, loop_beats: f64) {
        let mut t = from_ns;
        while t <= until_ns {
            if (t - from_ns).is_multiple_of(block_ns) {
                let beats = (t - 1_000_000_000) as f64 / BEAT_NS as f64;
                p.transport.store(true, Some(120.0), Some(beats.rem_euclid(loop_beats)), t);
            }
            p.tick(t);
            t += 1_000_000;
        }
    }

    fn trig(p: &Player, route: u64, part: u64, on: bool, at: u64) {
        p.with(|st, out| trigger_at(st, &p.transport.load(), at, route, part, on, out));
    }

    fn ons(rec: &Rec) -> Vec<u8> {
        rec.0.lock().iter().filter(|e| e.2 > 0).map(|e| e.1).collect()
    }

    fn edit(p: &Player, id: u64, f: impl FnOnce(&mut Store, u64)) {
        let mut s = p.store();
        f(&mut s, id);
        p.set_store(s);
    }

    fn balanced(ev: &[(u32, u8, u8)]) -> bool {
        let mut held: HashMap<(u32, u8), i32> = HashMap::new();
        for &(c, n, v) in ev {
            *held.entry((c, n)).or_default() += if v > 0 { 1 } else { -1 };
        }
        held.values().all(|v| *v <= 0)
    }

    #[test]
    fn note_names_follow_fl() {
        assert_eq!(note_name(60), "C5");
        assert_eq!(note_name(42), "F#3");
        assert_eq!(parse_note("C5"), Some(60));
        assert_eq!(parse_note("f#3"), Some(42));
        assert_eq!(parse_note("Bb2"), Some(34));
        assert_eq!(parse_note("X1"), None);
    }

    #[test]
    fn chords_stay_in_the_key() {
        let am = Key { root: 9, scale: Scale::Minor };
        assert_eq!(chord(am, 0, 3, 4), vec![57, 60, 64]); // A4 C5 E5
        assert_eq!(chord_name(am, 0, 3), "Am");
        assert_eq!(chord_name(am, 5, 3), "F");
        assert_eq!(chord_name(am, 1, 3), "B°");
        assert_eq!(degree_name(am, 0), "i");
        assert_eq!(degree_name(am, 2), "III");
        let c = Key { root: 0, scale: Scale::Major };
        assert_eq!(chord_name(c, 4, 4), "G7");
        assert_eq!(chord(c, 6, 3, 4), vec![59, 62, 65]);
    }

    #[test]
    fn drum_templates_move_to_c5_off_fpc() {
        let mut p = template("trap roll").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        fit_to_channel(&mut p, "FPC");
        assert!(matches!(&p, Play::Clip { steps, .. } if steps.iter().all(|s| s.pitch == HAT)));
        fit_to_channel(&mut p, "pv002 - Hit 1");
        assert!(matches!(&p, Play::Clip { steps, rows, .. } if steps.iter().all(|s| s.pitch == 60) && rows == &vec![60]));
        // a melody keeps its notes
        let mut m = Play::Clip { bars: 1, steps: vec![Step { at: 0, len: 1, pitch: 64, vel: 90, ratchet: 1, nudge: 0 }], rows: vec![] };
        fit_to_channel(&mut m, "3x Osc");
        assert!(matches!(&m, Play::Clip { steps, .. } if steps[0].pitch == 64));
    }

    #[test]
    fn keys_from_frame_tokens() {
        let old = Key { root: 0, scale: Scale::Major };
        let t = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(key_from(&t(&["key:A", "scale:minor"]), old), Some(Key { root: 9, scale: Scale::Minor }));
        assert_eq!(key_from(&t(&["key:F#"]), old), Some(Key { root: 6, scale: Scale::Major }));
        assert_eq!(key_from(&t(&["scale:moll"]), old), Some(Key { root: 0, scale: Scale::Minor }));
        assert_eq!(key_from(&t(&["key:Bb", "scale:dorian"]), old), Some(Key { root: 10, scale: Scale::Dorian }));
        assert_eq!(key_from(&t(&["scope:all"]), old), None);
    }

    #[test]
    fn euclid_spreads_hits() {
        let e = euclid(4, 16);
        assert_eq!(e.iter().filter(|x| **x).count(), 4);
        assert!(e[0] && e[4] && e[8] && e[12]);
        assert_eq!(euclid(3, 8).iter().filter(|x| **x).count(), 3);
        assert_eq!(euclid(0, 16).iter().filter(|x| **x).count(), 0);
        assert_eq!(euclid(16, 16).iter().filter(|x| **x).count(), 16);
    }

    #[test]
    fn a_held_finger_plays_the_hats_on_the_grid_and_stops_clean() {
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Gate, Start::Sixteenth);
        run_host(&p, 1_000_000_000, 1_100_000_000);
        // shown at beat 0.2: the first hat plays on step 2 (beat 0.5), bar-aligned
        trig(&p, 7, id, true, 1_100_000_000);
        run_host(&p, 1_100_000_000, 1_000_000_000 + 4 * BEAT_NS - 2_000_000);
        let ons: Vec<_> = rec.0.lock().iter().filter(|e| e.2 > 0).copied().collect();
        // steps 2, 4, .. 14 of bar 1: 7 hats on channel 3, pitch 42
        assert_eq!(ons.len(), 7, "{ons:?}");
        assert!(ons.iter().all(|e| e.0 == 3 && e.1 == HAT));
        trig(&p, 7, id, false, 1_000_000_000 + 4 * BEAT_NS);
        run_host(&p, 1_000_000_000 + 4 * BEAT_NS, 1_000_000_000 + 6 * BEAT_NS);
        let all = rec.0.lock().clone();
        assert_eq!(all.iter().filter(|e| e.2 > 0).count(), 7, "nothing after release");
        assert!(balanced(&all), "every on has its off: {all:?}");
        assert!(!p.tick(1_000_000_000 + 6 * BEAT_NS), "idle after release");
    }

    #[test]
    fn two_routes_hold_one_part_until_both_let_go() {
        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        trig(&p, 2, id, true, 1_020_000_000);
        trig(&p, 1, id, false, 1_030_000_000);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100)]);
        p.release_route(2);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100), (3, 60, 0)]);
    }

    #[test]
    fn chords_change_with_the_bar_and_end_on_release() {
        let prog = Play::Chords { degrees: vec![0, 5], size: 3, octave: 4, vel: 90, strum: 0 };
        let (p, rec, id) = setup(prog, Mode::Gate, Start::Now);
        p.set_store({
            let mut s = p.store();
            s.key = Key { root: 9, scale: Scale::Minor };
            s
        });
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        run_host(&p, 1_010_000_000, 1_000_000_000 + 5 * BEAT_NS);
        let ev = rec.0.lock().clone();
        let ons: Vec<u8> = ev.iter().filter(|e| e.2 > 0).map(|e| e.1).collect();
        assert_eq!(ons, vec![57, 60, 64, 53, 57, 60], "Am then F: {ev:?}");
        trig(&p, 1, id, false, 1_000_000_000 + 5 * BEAT_NS);
        assert!(balanced(&rec.0.lock()));
    }

    #[test]
    fn live_off_and_a_host_stop_leave_nothing_hanging() {
        let (p, rec, id) = setup(Play::Note { pitch: 64, vel: 100 }, Mode::Latch, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        trig(&p, 1, id, false, 1_020_000_000);
        assert_eq!(rec.0.lock().len(), 1, "latched: still on after release");
        // the host stops: the note ends, the latched part plays on our clock
        p.transport.store(false, Some(120.0), Some(0.5), 1_250_000_000);
        p.tick(1_260_000_000);
        p.tick(1_270_000_000);
        let ev = rec.0.lock().clone();
        assert!(ev.contains(&(3, 64, 0)) && ev.iter().filter(|e| e.2 > 0).count() == 2, "{ev:?}");
        p.set_live(false);
        assert!(balanced(&rec.0.lock()));
        trig(&p, 1, id, true, 1_300_000_000);
        assert!(balanced(&rec.0.lock()), "nothing starts while Live is off");
    }

    #[test]
    fn once_plays_one_clip_length() {
        let kick = template("kick").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(kick, Mode::Once, Start::Bar);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        trig(&p, 1, id, false, 1_020_000_000);
        run_host(&p, 1_020_000_000, 1_000_000_000 + 12 * BEAT_NS);
        let ons = rec.0.lock().iter().filter(|e| e.2 > 0).count();
        assert_eq!(ons, 3, "one bar of kick, from bar 2");
        assert!(balanced(&rec.0.lock()));
    }

    #[test]
    fn trap_roll_ratchets_and_store_round_trip() {
        let roll = template("trap roll").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(roll, Mode::Gate, Start::Bar);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        run_host(&p, 1_010_000_000, 1_000_000_000 + 8 * BEAT_NS - 2_000_000);
        let ons = rec.0.lock().iter().filter(|e| e.2 > 0).count();
        // 16 steps with ratchets 2, 3, 2, 4, 2 add 1 + 2 + 1 + 3 + 1 hits: 24 per bar, bar 2 only
        assert_eq!(ons, 24);
        let s = p.store();
        assert_eq!(Store::load(&s.save()), s);
        assert_eq!(Store::load("{oops"), Store::default());
    }

    #[test]
    fn once_keeps_its_length_across_a_host_loop() {
        // FL loops one bar, the blocks split at the loop point: the
        // extrapolation never reaches beat 4.0 before the position wraps
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Once, Start::Sixteenth);
        run_loop(&p, 1_000_000_000, 1_500_000_000, 1_000_000, 4.0);
        // shown on beat 1 (step 4): steps 4..15, then 0..3 after the loop
        trig(&p, 1, id, true, 1_500_000_000);
        trig(&p, 1, id, false, 1_510_000_000);
        run_loop(&p, 1_510_000_000, 1_000_000_000 + 12 * BEAT_NS, 1_000_000, 4.0);
        assert_eq!(ons(&rec).len(), 8, "one clip length: {:?}", rec.0.lock());
        assert!(balanced(&rec.0.lock()));
        assert!(!p.tick(1_000_000_000 + 12 * BEAT_NS + 1_000_000), "done");
    }

    #[test]
    fn a_progression_and_a_long_clip_move_on_in_a_one_bar_host_loop() {
        let prog = Play::Chords { degrees: vec![0, 4, 5, 3], size: 3, octave: 4, vel: 90, strum: 0 };
        let (p, rec, id) = setup(prog, Mode::Gate, Start::Now);
        run_loop(&p, 1_000_000_000, 1_010_000_000, 1_000_000, 4.0);
        trig(&p, 1, id, true, 1_010_000_000);
        run_loop(&p, 1_010_000_000, 1_000_000_000 + 16 * BEAT_NS - 2_000_000, 1_000_000, 4.0);
        let want: Vec<u8> = [0, 4, 5, 3].iter().flat_map(|d| chord(Key::default(), *d, 3, 4)).collect();
        assert_eq!(ons(&rec), want, "i v VI iv, one per loop");
        let two = Play::Clip {
            bars: 2,
            steps: vec![Step { at: 0, len: 1, pitch: 60, vel: 100, ratchet: 1, nudge: 0 }, Step { at: 16, len: 1, pitch: 62, vel: 100, ratchet: 1, nudge: 0 }],
            rows: vec![],
        };
        let (p, rec, id) = setup(two, Mode::Gate, Start::Now);
        run_loop(&p, 1_000_000_000, 1_000_000_000, 1_000_000, 4.0);
        trig(&p, 1, id, true, 1_000_000_000);
        run_loop(&p, 1_000_000_000, 1_000_000_000 + 8 * BEAT_NS - 2_000_000, 1_000_000, 4.0);
        assert_eq!(ons(&rec), vec![60, 62], "bar 2 of the clip in the second loop");
    }

    #[test]
    fn a_loop_past_the_extrapolation_plays_its_first_step_once() {
        // blocks of 10 ms, one spans the loop point: the player sends step
        // 0 of the next bar before the wrapped position arrives
        let kick = template("kick").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(kick, Mode::Gate, Start::Sixteenth);
        let t0 = 1_005_000_000;
        run_loop(&p, t0, t0, 10_000_000, 4.0);
        trig(&p, 1, id, true, t0);
        run_loop(&p, t0, t0 + 8 * BEAT_NS - 100_000_000, 10_000_000, 4.0);
        // steps 6 and 8, then 0, 6 and 8 after the loop
        assert_eq!(ons(&rec).len(), 5, "{:?}", rec.0.lock());
    }

    #[test]
    fn a_host_jump_just_before_a_gesture_keeps_its_start() {
        let kick = template("kick").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(kick, Mode::Gate, Start::Bar);
        run_loop(&p, 1_000_000_000, 1_000_000_000 + 1_850_000_000, 1_000_000, 4.0);
        // the player idles; the host loops and the finger shows 200 ms
        // later, before the player's next tick
        let t = 1_000_000_000 + 2_050_000_000;
        p.transport.store(true, Some(120.0), Some(0.1), t);
        trig(&p, 1, id, true, t);
        assert!(rec.0.lock().is_empty(), "waits for the next bar");
        run_loop(&p, t, 1_000_000_000 + 4_010_000_000, 1_000_000, 4.0);
        assert_eq!(ons(&rec), vec![KICK], "the kick on the next bar");
    }

    #[test]
    fn a_once_pressed_again_while_it_plays_sends_each_step_once() {
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Once, Start::Sixteenth);
        run_host(&p, 1_000_000_000, 1_050_000_000);
        trig(&p, 1, id, true, 1_050_000_000);
        run_host(&p, 1_050_000_000, 1_100_000_000);
        trig(&p, 1, id, false, 1_100_000_000);
        // again at beat 0.85: step 4 is queued already
        run_host(&p, 1_100_000_000, 1_425_000_000);
        trig(&p, 1, id, true, 1_425_000_000);
        run_host(&p, 1_425_000_000, 1_000_000_000 + 6 * BEAT_NS);
        // steps 2..16 from the first press, on to step 18 from the second
        assert_eq!(ons(&rec).len(), 9, "{:?}", rec.0.lock());
        assert!(balanced(&rec.0.lock()));
    }

    #[test]
    fn strummed_notes_of_the_old_chord_stop_at_the_chord_change() {
        let prog = Play::Chords { degrees: vec![0, 4], size: 4, octave: 4, vel: 90, strum: 24 };
        let (p, rec, id) = setup(prog, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_000_000_000 + 7 * BEAT_NS / 2);
        // shown on beat 3.5: i's notes due on 3.5, 3.75, 4.0 and 4.25
        trig(&p, 1, id, true, 1_000_000_000 + 7 * BEAT_NS / 2);
        run_host(&p, 1_000_000_000 + 7 * BEAT_NS / 2, 1_000_000_000 + 49 * BEAT_NS / 10);
        let key = Key::default();
        let mut want = chord(key, 0, 4, 4)[..2].to_vec();
        want.extend(chord(key, 4, 4, 4));
        assert_eq!(ons(&rec), want, "{:?}", rec.0.lock());
        trig(&p, 1, id, false, 1_000_000_000 + 49 * BEAT_NS / 10);
        assert!(balanced(&rec.0.lock()));
    }

    #[test]
    fn a_tempo_change_on_our_own_clock_cuts_nothing() {
        let (p, rec, id) = setup(Play::Note { pitch: 64, vel: 100 }, Mode::Latch, Start::Now);
        p.transport.store(false, Some(120.0), None, 1_000_000_000);
        p.tick(1_000_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        trig(&p, 1, id, false, 1_020_000_000);
        let mut t = 1_020_000_000;
        while t < 11_000_000_000 {
            p.tick(t);
            t += 10_000_000;
        }
        p.transport.store(false, Some(121.0), None, t);
        p.tick(t);
        p.tick(t + 10_000_000);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 64, 100)]);
    }

    #[test]
    fn a_late_block_does_not_switch_the_chord_back() {
        let prog = Play::Chords { degrees: vec![0, 4], size: 3, octave: 4, vel: 90, strum: 0 };
        let (p, rec, id) = setup(prog, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        let t0 = 1_000_000_000 + 1_990_000_000;
        run_host(&p, 1_010_000_000, t0);
        // the extrapolation from beat 3.98 crosses the bar line: v
        p.tick(t0 + 12_000_000);
        // the next block comes late, 0.008 beats behind it, then on
        p.transport.store(true, Some(120.0), Some(3.996), t0 + 12_000_000);
        p.tick(t0 + 12_000_000);
        p.tick(t0 + 15_000_000);
        let key = Key::default();
        let want: Vec<u8> = chord(key, 0, 3, 4).into_iter().chain(chord(key, 4, 3, 4)).collect();
        assert_eq!(ons(&rec), want, "{:?}", rec.0.lock());
    }

    #[test]
    fn an_edit_to_a_playing_part_sounds_at_once() {
        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Latch, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, id, true, 1_010_000_000);
        trig(&p, 1, id, false, 1_010_000_000);
        let set = |s: &mut Store, id: u64, play: Play| {
            if let Some(x) = s.get_mut(id) {
                x.play = play;
            }
        };
        // another pitch, then a chord, then another key
        edit(&p, id, |s, id| set(s, id, Play::Note { pitch: 64, vel: 100 }));
        run_host(&p, 1_020_000_000, 1_020_000_000);
        edit(&p, id, |s, id| set(s, id, Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 90, strum: 0 }));
        run_host(&p, 1_040_000_000, 1_040_000_000);
        let c = Key { root: 0, scale: Scale::Major };
        edit(&p, id, |s, _| s.key = c);
        run_host(&p, 1_060_000_000, 1_150_000_000);
        let mut want = vec![60, 64];
        want.extend(chord(Key::default(), 0, 3, 4));
        want.extend(chord(c, 0, 3, 4));
        assert_eq!(ons(&rec), want, "{:?}", rec.0.lock());
        // to the hats on beat 0.3: the chord ends, the first hat waits for step 2
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        edit(&p, id, |s, id| set(s, id, hats));
        run_host(&p, 1_150_000_000, 1_150_000_000);
        assert!(balanced(&rec.0.lock()), "nothing left of the chord: {:?}", rec.0.lock());
        assert_eq!(ons(&rec).len(), 8, "no hat late");
        run_host(&p, 1_150_000_000, 1_260_000_000);
        assert_eq!(ons(&rec).get(8..), Some(&[HAT][..]));
    }

    #[test]
    fn unbind_drops_only_its_own_part() {
        let mut s = Store::default();
        let note = |pitch| Play::Note { pitch, vel: 100 };
        let shape = |mask| Trigger::Shape { mask, care: mask };
        let own = s.add("own", note(60), None);
        let shared = s.add("shared", note(62), None);
        let pending = s.add("pending", note(64), None);
        s.bind("hand.R", shape(1), own);
        s.bind("hand.R", shape(2), shared);
        s.bind("hand.L", shape(2), shared);
        s.unbind("hand.R", shape(1));
        s.unbind("hand.R", shape(2));
        assert!(s.get(own).is_none(), "its part goes with the bind");
        assert!(s.get(shared).is_some(), "another bind still plays it");
        assert!(s.get(pending).is_some(), "a line before Keep keeps its part");
    }

    #[test]
    fn a_moved_channel_is_found_again_by_its_name() {
        let row = |index, name: &str, plugin: &str| ChannelRow { index, name: name.into(), plugin: plugin.into() };
        let ch = |index, name: &str| Some(Channel { index, name: name.into() });
        let note = || Play::Note { pitch: 60, vel: 100 };
        let mut s = Store::default();
        let pad = s.add("pad", note(), ch(3, "Serum 2 · Pad"));
        let kit = s.add("kit", note(), ch(1, "FPC"));
        let kick = s.add("kick", note(), ch(5, "Kick"));
        let lead = s.add("lead", note(), ch(6, "Serum 2 · Lead"));
        // a channel inserted before the pad, the kick deleted, the lead
        // renamed in place
        let rows = [row(0, "Sampler", ""), row(1, "FPC", "FPC"), row(2, "Bass", "Serum 2"), row(4, "Pad", "Serum 2"), row(6, "Lead 2", "Serum 2")];
        assert!(s.rebind_channels(&rows));
        let at = |id| s.get(id).and_then(|p| p.channel.clone());
        assert_eq!(at(pad), ch(4, "Serum 2 · Pad"));
        assert_eq!(at(kit), ch(1, "FPC"));
        assert_eq!(at(kick), None, "never another channel");
        assert_eq!(at(lead), ch(6, "Serum 2 · Lead 2"));
        assert!(!s.rebind_channels(&rows), "found once, stays");
        assert!(!s.rebind_channels(&[]), "no inventory yet: nothing changes");
    }

    /// An output that notes, per send, whether the player's state was
    /// still locked.
    #[derive(Default)]
    struct Probe {
        player: OnceLock<Weak<Player>>,
        locked: Mutex<Vec<bool>>,
    }

    impl Out for Probe {
        fn notes(&self, _: &[(u32, u8, u8)]) {
            if let Some(p) = self.player.get().and_then(Weak::upgrade) {
                let held = p.state.try_lock().is_none();
                self.locked.lock().push(held);
            }
        }
    }

    #[test]
    fn notes_leave_under_the_state_lock_so_an_off_never_overtakes_its_on() {
        let probe = Arc::new(Probe::default());
        let p = Player::manual(probe.clone());
        let _ = probe.player.set(Arc::downgrade(&p));
        let mut s = Store::default();
        let id = s.add("t", Play::Note { pitch: 60, vel: 100 }, Some(Channel { index: 3, name: "FPC".into() }));
        p.set_store(s.clone());
        p.trigger(1, id, true);
        if let Some(x) = s.get_mut(id) {
            x.play = Play::Note { pitch: 64, vel: 100 };
        }
        p.set_store(s);
        p.tick(now_ns());
        p.trigger(1, id, false);
        let locked = probe.locked.lock().clone();
        assert_eq!(locked.len(), 4, "on, off by the edit, on, off");
        assert!(locked.iter().all(|l| *l), "{locked:?}");
    }

    fn hit(p: &Player, part: u64, strength: f32, len: f64, at: u64) -> usize {
        p.with(|st, out| hits_at(st, &p.transport.load(), at, &[HitReq { part, strength, at_ns: at }], true, len, out))
    }

    fn hit_voices(p: &Player) -> usize {
        p.state.lock().hits.len()
    }

    /// Every on reaches a silent note, every off a sounding one, and
    /// nothing is left on.
    fn clean(ev: &[(u32, u8, u8)]) -> bool {
        let mut on: Vec<(u32, u8)> = Vec::new();
        for &(c, n, v) in ev {
            match (v > 0, on.iter().position(|x| *x == (c, n))) {
                (true, None) => on.push((c, n)),
                (false, Some(i)) => {
                    on.swap_remove(i);
                }
                _ => return false,
            }
        }
        on.is_empty()
    }

    #[test]
    fn a_note_hit_plays_at_once_and_ends_after_its_length() {
        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Latch, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_010_000_000), 1);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100)], "on at once, the mode ignored");
        run_host(&p, 1_010_000_000, 1_130_000_000);
        assert_eq!(rec.0.lock().len(), 1, "still on 120 ms later");
        run_host(&p, 1_130_000_000, 1_140_000_000);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100), (3, 60, 0)], "off after a 1/16 at 120 BPM");
        // a soft shot plays softer, a tiny length still lasts 40 ms
        run_host(&p, 1_140_000_000, 1_200_000_000);
        assert_eq!(hit(&p, id, 0.0, 0.001, 1_200_000_000), 1);
        assert_eq!(rec.0.lock().last(), Some(&(3, 60, 55)));
        run_host(&p, 1_200_000_000, 1_235_000_000);
        assert_eq!(rec.0.lock().len(), 3);
        run_host(&p, 1_235_000_000, 1_245_000_000);
        assert_eq!(rec.0.lock().last(), Some(&(3, 60, 0)));
        assert_eq!(hit(&p, 999, 1.0, 0.25, 1_245_000_000), 0, "no such part");
        assert!(!p.tick(1_250_000_000), "idle");
    }

    #[test]
    fn two_thousand_hits_in_two_seconds_end_clean_within_the_budget() {
        let rec = Arc::new(Rec::default());
        let p = Player::manual(rec.clone());
        let mut s = Store::default();
        let ids: Vec<u64> = (0..4).map(|i| s.add("wall", Play::Note { pitch: 60 + i, vel: 100 }, Some(Channel { index: 3, name: "FPC".into() }))).collect();
        p.set_store(s);
        let mut t = 1_000_000_000;
        for i in 0..2000usize {
            let beats = (t - 1_000_000_000) as f64 / BEAT_NS as f64;
            p.transport.store(true, Some(120.0), Some(beats), t);
            hit(&p, ids[i % 4], 0.5, 0.25, t);
            p.tick(t);
            t += 1_000_000;
        }
        run_host(&p, t, t + 1_000_000_000);
        let ev = rec.0.lock().clone();
        assert!(clean(&ev), "every on has its off");
        let ons = ev.iter().filter(|e| e.2 > 0).count() as u64;
        // 8 at once, then 40 per second over the 2 s of hits
        assert!((80..=88).contains(&ons), "{ons} ons");
        let dropped = p.state.lock().budget().dropped.load(Ordering::Relaxed);
        assert_eq!(ons + dropped, 2000, "each hit sent or counted");
        assert!(!p.tick(t + 1_000_000_001), "idle");
    }

    #[test]
    fn a_clip_hit_plays_the_clip_once_with_at_most_two_voices_per_part() {
        let kick = template("kick").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(kick, Mode::Latch, Start::Now);
        run_host(&p, 1_000_000_000, 1_100_000_000);
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_100_000_000), 1);
        run_host(&p, 1_100_000_000, 1_000_000_000 + 8 * BEAT_NS);
        assert_eq!(ons(&rec), vec![KICK; 3], "one bar of kick from the hit: {:?}", rec.0.lock());
        assert!(clean(&rec.0.lock()));
        assert_eq!(hit_voices(&p), 0, "ended at the clip's end");

        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_100_000_000);
        for k in 0..3 {
            let t = 1_100_000_000 + k * 10_000_000;
            run_host(&p, t, t);
            hit(&p, id, 1.0, 0.25, t);
            assert!(hit_voices(&p) <= 2);
        }
        assert_eq!(hit_voices(&p), 2, "the third ended the oldest");
        run_host(&p, 1_120_000_000, 1_000_000_000 + 8 * BEAT_NS);
        let ev = rec.0.lock().clone();
        assert!(clean(&ev), "{ev:?}");
        // the first voice's step 0, then all 8 hats of the other two
        assert_eq!(ons(&rec).len(), 17, "{ev:?}");
        assert_eq!(hit_voices(&p), 0);
    }

    #[test]
    fn a_clip_hit_plays_on_across_a_host_loop() {
        let kick = template("kick").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(kick, Mode::Gate, Start::Now);
        let t = 1_000_000_000 + 7 * BEAT_NS / 2;
        run_loop(&p, 1_000_000_000, t, 1_000_000, 4.0);
        hit(&p, id, 1.0, 0.25, t);
        run_loop(&p, t, 1_000_000_000 + 12 * BEAT_NS, 1_000_000, 4.0);
        // on beat 3.5, then 1.0 and 1.5 after the loop point
        assert_eq!(ons(&rec), vec![KICK; 3], "{:?}", rec.0.lock());
        assert!(clean(&rec.0.lock()));
    }

    #[test]
    fn a_hit_on_a_note_a_finger_holds_restrikes_it_and_the_finger_keeps_it() {
        let rec = Arc::new(Rec::default());
        let p = Player::manual(rec.clone());
        let mut s = Store::default();
        let ch = || Some(Channel { index: 3, name: "FPC".into() });
        let finger = s.add("finger", Play::Note { pitch: 60, vel: 100 }, ch());
        let wall = s.add("wall", Play::Note { pitch: 60, vel: 90 }, ch());
        p.set_store(s);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        trig(&p, 1, finger, true, 1_010_000_000);
        run_host(&p, 1_010_000_000, 1_020_000_000);
        hit(&p, wall, 1.0, 0.25, 1_020_000_000);
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100), (3, 60, 0), (3, 60, 90)], "struck again");
        run_host(&p, 1_020_000_000, 1_300_000_000);
        assert_eq!(rec.0.lock().len(), 3, "the hit ended, the finger's note sounds on");
        // Pong off while a hit sounds: the finger's note stays too
        hit(&p, wall, 1.0, 4.0, 1_300_000_000);
        p.end_hits();
        assert_eq!(rec.0.lock().len(), 5);
        trig(&p, 1, finger, false, 1_310_000_000);
        let ev = rec.0.lock().clone();
        assert_eq!(ev.last(), Some(&(3, 60, 0)), "off with the finger");
        assert!(clean(&ev), "{ev:?}");
    }

    #[test]
    fn live_off_and_stop_all_end_hit_voices() {
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_100_000_000);
        hit(&p, id, 1.0, 0.25, 1_100_000_000);
        // step 2 sounds at beat 0.72
        run_host(&p, 1_100_000_000, 1_360_000_000);
        assert_eq!(hit_voices(&p), 1);
        p.set_live(false);
        assert!(clean(&rec.0.lock()), "{:?}", rec.0.lock());
        assert_eq!(hit_voices(&p), 0);
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_360_000_000), 0, "nothing while Live is off");
        p.set_live(true);
        run_host(&p, 1_360_000_000, 1_400_000_000);
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_400_000_000), 1);
        run_host(&p, 1_400_000_000, 1_450_000_000);
        p.stop_all();
        assert!(clean(&rec.0.lock()), "{:?}", rec.0.lock());
        assert_eq!(hit_voices(&p), 0);
        let n = rec.0.lock().len();
        run_host(&p, 1_450_000_000, 1_000_000_000 + 6 * BEAT_NS);
        assert_eq!(rec.0.lock().len(), n, "nothing after");
    }

    #[test]
    fn a_part_gone_or_moved_to_another_channel_ends_its_hits() {
        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        hit(&p, id, 1.0, 4.0, 1_010_000_000);
        edit(&p, id, |s, id| s.parts.retain(|x| x.id != id));
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100), (3, 60, 0)]);
        assert!(p.state.lock().queue.is_empty(), "its off dropped with it");

        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_010_000_000);
        hit(&p, id, 1.0, 4.0, 1_010_000_000);
        let move_to = |c: Option<Channel>| {
            let mut s = p.store();
            if let Some(x) = s.get_mut(id) {
                x.channel = c;
            }
            p.set_store(s);
        };
        move_to(Some(Channel { index: 4, name: "Kick".into() }));
        assert_eq!(rec.0.lock().as_slice(), &[(3, 60, 100), (3, 60, 0)]);
        move_to(None);
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_010_000_000), 0, "no channel, no hit");
    }

    #[test]
    fn a_host_seek_ends_hit_voices() {
        let hats = template("hi-hat basic").unwrap_or(Play::Note { pitch: 0, vel: 0 });
        let (p, rec, id) = setup(hats, Mode::Gate, Start::Now);
        run_host(&p, 1_000_000_000, 1_100_000_000);
        hit(&p, id, 1.0, 0.25, 1_100_000_000);
        run_host(&p, 1_100_000_000, 1_360_000_000);
        // the host jumps to beat 10.37
        let t0 = 1_361_000_000;
        let mut t = t0;
        while t <= t0 + 3_000_000_000 {
            p.transport.store(true, Some(120.0), Some(10.37 + (t - t0) as f64 / BEAT_NS as f64), t);
            p.tick(t);
            if t == t0 {
                assert!(clean(&rec.0.lock()), "{:?}", rec.0.lock());
                assert_eq!(hit_voices(&p), 0);
            }
            t += 1_000_000;
        }
        assert!(clean(&rec.0.lock()), "nothing more: {:?}", rec.0.lock());
    }

    #[test]
    fn a_chords_part_plays_the_next_degree_on_each_hit() {
        let prog = Play::Chords { degrees: vec![0, 4, 5, 3], size: 3, octave: 4, vel: 90, strum: 0 };
        let (p, rec, id) = setup(prog, Mode::Gate, Start::Now);
        let mut last = 1_000_000_000;
        for k in 0..5 {
            let t = 1_010_000_000 + k * 200_000_000;
            run_host(&p, last, t);
            hit(&p, id, 1.0, 0.25, t);
            last = t;
        }
        run_host(&p, last, last + 1_000_000_000);
        let want: Vec<u8> = [0, 4, 5, 3, 0].iter().flat_map(|d| chord(Key::default(), *d, 3, 4)).collect();
        assert_eq!(ons(&rec), want);
        assert!(clean(&rec.0.lock()));
    }

    #[test]
    fn a_hit_on_a_sixteenth_part_waits_for_the_grid() {
        let (p, rec, id) = setup(Play::Note { pitch: 60, vel: 100 }, Mode::Gate, Start::Sixteenth);
        run_host(&p, 1_000_000_000, 1_050_000_000);
        // beat 0.1: the on waits for beat 0.25
        assert_eq!(hit(&p, id, 1.0, 0.25, 1_050_000_000), 1);
        run_host(&p, 1_050_000_000, 1_120_000_000);
        assert!(rec.0.lock().is_empty());
        run_host(&p, 1_120_000_000, 1_130_000_000);
        assert_eq!(ons(&rec), vec![60]);
    }

    #[test]
    fn a_wall_keeps_its_part_through_unbind() {
        let mut s = Store::default();
        let shape = Trigger::Shape { mask: 1, care: 1 };
        let w = s.add("wall", Play::Note { pitch: 60, vel: 100 }, None);
        s.bind("hand.R", shape, w);
        s.walls.push(w);
        s.unbind("hand.R", shape);
        assert!(s.get(w).is_some());
        assert_eq!(Store::load(&s.save()), s);
        let old = r#"{"version":1,"key":{"root":9,"scale":"minor"},"parts":[],"next_id":1}"#;
        assert!(Store::load(old).walls.is_empty(), "a project from before Pong loads");
    }
}
