//! What Pong mode is set to: saved with the project (params `pong`) and in
//! presets. Every field has a serde default, so a project from before Pong
//! and a preset from an older build load.

use serde::{Deserialize, Serialize};

use super::arena::{Life, Side};

/// What a wall hit does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    #[default]
    Off,
    /// Plays a synthexis MIDI part (midi::Store) once.
    Part { id: u64 },
    /// A parameter of a plugin in FL, written through the bridge script.
    /// `key` is the fl::Key id, `name` the parameter's name (display and
    /// rebinding).
    Param {
        key: String,
        index: u32,
        #[serde(default)]
        name: String,
        #[serde(default)]
        how: How,
    },
    /// Signal macro 0..31 (Signal N = N - 1).
    Macro {
        index: usize,
        #[serde(default)]
        how: How,
    },
}

/// How a hit moves a parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Jumps up by the hit's depth and glides back to `lo`.
    #[default]
    Ramp,
    /// Goes to where the side was hit (0..1 along it) and stays.
    Position,
    /// Each hit flips between `lo` and `hi`.
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct How {
    pub mode: Mode,
    pub lo: f32,
    pub hi: f32,
    /// Ramp depth from the projectile's speed; off: always to `hi`.
    pub depth_from_speed: bool,
    /// Ramp: the glide back to `lo`, ms.
    pub back_ms: u32,
}

impl Default for How {
    fn default() -> Self {
        Self { mode: Mode::Ramp, lo: 0.0, hi: 1.0, depth_from_speed: true, back_ms: 400 }
    }
}

/// Which hands shoot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hands {
    Left,
    #[default]
    Right,
    Both,
}

impl Hands {
    /// Whether hand `side` (0 = L, 1 = R) shoots.
    pub fn has(self, side: usize) -> bool {
        matches!((self, side), (Hands::Both, _) | (Hands::Left, 0) | (Hands::Right, 1))
    }
}

/// What shoots. Only Point runs since 11-04 (Noa: one built-in shot, the
/// best); the field stays so older projects and presets load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The finger mask turns into a point (shot.rs; Noa, 11-04 night).
    #[default]
    Point,
    /// The index springs off the thumb; shoots as a point now.
    Flick,
}

pub const SPEED_MIN: f32 = 0.25;
pub const SPEED_MAX: f32 = 4.0;
pub const MAX_SHOTS: u32 = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setup {
    pub version: u32,
    /// Pong mode on: flicks shoot, hits fire.
    pub on: bool,
    pub hands: Hands,
    pub trigger: Trigger,
    /// Frame heights per second.
    pub speed: f32,
    /// A harder shot flies faster (0.6x to 1.4x of `speed`).
    pub speed_from_flick: bool,
    pub life: Life,
    /// Projectiles at once; a shot over it removes the oldest.
    pub max: u32,
    /// Note velocities scaled by the shot's strength (0.55x .. 1x); off:
    /// the part's own. When a hit plays (at once, 1/16, beat, bar) is the
    /// wall part's own `start`.
    pub vel_from_speed: bool,
    /// Length of a hit's note or chord, beats.
    pub hit_len: f32,
    /// While Pong is on, the shooting hands' finger and shape parts pause
    /// (a flick folds and shows the index).
    pub pause_fingers: bool,
    /// What each side does, in Side order (top, right, bottom, left).
    pub walls: [Action; 4],
    /// Parts that get the generator FL has in focus (else the first
    /// channel) once FL's channel list is there (a built-in preset loaded
    /// before the link was up).
    pub auto_channel: Vec<u64>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            version: 1,
            on: false,
            hands: Hands::Right,
            trigger: Trigger::Point,
            speed: 1.0,
            speed_from_flick: false,
            life: Life { bounces: Some(3), seconds: None },
            max: 16,
            vel_from_speed: true,
            hit_len: 0.25,
            pause_fingers: true,
            walls: Default::default(),
            auto_channel: Vec::new(),
        }
    }
}

impl Setup {
    pub fn load(json: &str) -> Setup {
        if json.trim().is_empty() {
            return Setup::default();
        }
        serde_json::from_str(json).unwrap_or_else(|e| {
            crate::log::log(&format!("pong: setup not read ({e})"));
            Setup::default()
        })
    }

    pub fn save(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn wall(&self, side: Side) -> &Action {
        &self.walls[side.index()]
    }

    /// Ids of the MIDI parts the walls play.
    pub fn wall_parts(&self) -> Vec<u64> {
        self.walls.iter().filter_map(|w| if let Action::Part { id } = w { Some(*id) } else { None }).collect()
    }

    /// The setup with every value inside its range (a hand-edited preset).
    pub fn clamped(mut self) -> Setup {
        self.speed = if self.speed.is_finite() { self.speed.clamp(SPEED_MIN, SPEED_MAX) } else { 1.0 };
        self.max = self.max.clamp(1, MAX_SHOTS);
        self.hit_len = if self.hit_len.is_finite() { self.hit_len.clamp(0.0625, 4.0) } else { 0.25 };
        self.life.bounces = self.life.bounces.map(|b| b.clamp(1, 1000));
        self.life.seconds = self.life.seconds.filter(|s| s.is_finite()).map(|s| s.clamp(0.5, 600.0));
        for w in &mut self.walls {
            if let Action::Param { how, .. } | Action::Macro { how, .. } = w {
                how.lo = if how.lo.is_finite() { how.lo.clamp(0.0, 1.0) } else { 0.0 };
                how.hi = if how.hi.is_finite() { how.hi.clamp(0.0, 1.0) } else { 1.0 };
                how.back_ms = how.back_ms.clamp(20, 10_000);
            }
            if let Action::Macro { index, .. } = w {
                *index = (*index).min(crate::route::NUM_MACROS - 1);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_project_and_a_partial_setup_load_with_defaults() {
        assert_eq!(Setup::load(""), Setup::default());
        assert_eq!(Setup::load("{not json"), Setup::default());
        let s = Setup::load(r#"{"on":true,"walls":[{"kind":"part","id":3},{"kind":"macro","index":1},{"kind":"off"},{"kind":"param","key":"fx:1:0:Portal","index":12}]}"#);
        assert!(s.on);
        assert_eq!(s.max, 16);
        assert_eq!(s.walls[0], Action::Part { id: 3 });
        assert_eq!(s.walls[1], Action::Macro { index: 1, how: How::default() });
        assert!(matches!(&s.walls[3], Action::Param { name, how, .. } if name.is_empty() && how.mode == Mode::Ramp));
    }

    #[test]
    fn round_trip_and_clamp() {
        let mut s = Setup { on: true, speed: 99.0, max: 0, ..Setup::default() };
        s.walls[2] = Action::Macro { index: 77, how: How { lo: -1.0, hi: 2.0, ..How::default() } };
        assert_eq!(Setup::load(&s.save()), s);
        s.walls[2] = Action::Macro { index: 77, how: How { lo: -1.0, hi: f32::NAN, ..How::default() } };
        let c = s.clamped();
        assert_eq!((c.speed, c.max), (SPEED_MAX, 1));
        assert_eq!(c.walls[2], Action::Macro { index: 31, how: How { lo: 0.0, hi: 1.0, ..How::default() } });
    }

    #[test]
    fn hands() {
        assert!(Hands::Right.has(1) && !Hands::Right.has(0));
        assert!(Hands::Both.has(0) && Hands::Both.has(1));
        assert!(Hands::Left.has(0) && !Hands::Left.has(1));
    }
}
