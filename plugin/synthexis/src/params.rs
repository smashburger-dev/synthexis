//! Parameter definitions: 32 signal macros + control params.

use nih_plug::prelude::*;

/// "Live" gate: when off, the writer skips automation writes.
/// `test_signal`: turns the built-in test routes on (camera brightness and
/// motion -> Signal 1/2, test.triangle -> Signal 32, perception -> Signal
/// 3..10); off, only assigned routes move macros.
/// Hand roles are stored with the project but have no behaviour yet.
/// Language and completion on/off are per-user settings (settings.rs), not
/// project state; the "completion" key of older projects is ignored.

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
pub enum HandRole {
    #[id = "midi"]
    #[name = "MIDI"]
    Midi,
    #[id = "parameter"]
    #[name = "Parameter"]
    Parameter,
    #[id = "motion"]
    #[name = "Motion"]
    Motion,
    #[id = "off"]
    #[name = "Off"]
    Off,
}

fn signal_param(i: usize) -> FloatParam {
    FloatParam::new(
        format!("Signal {}", i + 1),
        0.0,
        FloatRange::Linear { min: 0.0, max: 1.0 },
    )
    // Host-visible values are written by us; no smoothing on the param side.
    .with_smoother(SmoothingStyle::None)
    .with_unit("")
}

#[derive(Params)]
pub struct SynthexisParams {
    #[id = "sig01"]
    pub sig01: FloatParam,
    #[id = "sig02"]
    pub sig02: FloatParam,
    #[id = "sig03"]
    pub sig03: FloatParam,
    #[id = "sig04"]
    pub sig04: FloatParam,
    #[id = "sig05"]
    pub sig05: FloatParam,
    #[id = "sig06"]
    pub sig06: FloatParam,
    #[id = "sig07"]
    pub sig07: FloatParam,
    #[id = "sig08"]
    pub sig08: FloatParam,
    #[id = "sig09"]
    pub sig09: FloatParam,
    #[id = "sig10"]
    pub sig10: FloatParam,
    #[id = "sig11"]
    pub sig11: FloatParam,
    #[id = "sig12"]
    pub sig12: FloatParam,
    #[id = "sig13"]
    pub sig13: FloatParam,
    #[id = "sig14"]
    pub sig14: FloatParam,
    #[id = "sig15"]
    pub sig15: FloatParam,
    #[id = "sig16"]
    pub sig16: FloatParam,
    #[id = "sig17"]
    pub sig17: FloatParam,
    #[id = "sig18"]
    pub sig18: FloatParam,
    #[id = "sig19"]
    pub sig19: FloatParam,
    #[id = "sig20"]
    pub sig20: FloatParam,
    #[id = "sig21"]
    pub sig21: FloatParam,
    #[id = "sig22"]
    pub sig22: FloatParam,
    #[id = "sig23"]
    pub sig23: FloatParam,
    #[id = "sig24"]
    pub sig24: FloatParam,
    #[id = "sig25"]
    pub sig25: FloatParam,
    #[id = "sig26"]
    pub sig26: FloatParam,
    #[id = "sig27"]
    pub sig27: FloatParam,
    #[id = "sig28"]
    pub sig28: FloatParam,
    #[id = "sig29"]
    pub sig29: FloatParam,
    #[id = "sig30"]
    pub sig30: FloatParam,
    #[id = "sig31"]
    pub sig31: FloatParam,
    #[id = "sig32"]
    pub sig32: FloatParam,

    #[id = "live"]
    pub live: BoolParam,
    #[id = "test_signal"]
    pub test_signal: BoolParam,
    #[id = "hand_role_l"]
    pub hand_role_left: EnumParam<HandRole>,
    #[id = "hand_role_r"]
    pub hand_role_right: EnumParam<HandRole>,

    /// Editor state (the window's size). Not persisted (v3 2.8): the window
    /// has one fixed size, and a project saved at 1200 x 820 opens at it.
    pub editor_state: Arc<EguiState>,

    /// Pinned plugin picker target (fl::Key id); empty follows FL focus.
    #[persist = "target-pin"]
    pub target_pin: Arc<RwLock<String>>,
    /// Kept routes of the sentence field (saved.rs JSON).
    #[persist = "routes"]
    pub routes: Arc<RwLock<String>>,
    /// MIDI parts, key and binds (midi::Store JSON).
    #[persist = "midi"]
    pub midi: Arc<RwLock<String>>,
    /// Pong mode (pong::setup::Setup JSON).
    #[persist = "pong"]
    pub pong: Arc<RwLock<String>>,
    /// The open sections and the picked preset (preset::top::View JSON);
    /// empty in projects from before the redesign.
    #[persist = "view"]
    pub view: Arc<RwLock<String>>,
    /// The profiles 1 2 3 and the active one (preset::profile::Profiles
    /// JSON); empty in older projects, which open on profile 1 holding
    /// their setup.
    #[persist = "profiles"]
    pub profiles: Arc<RwLock<String>>,
}

impl SynthexisParams {
    pub fn signals(&self) -> [&FloatParam; 32] {
        [
            &self.sig01, &self.sig02, &self.sig03, &self.sig04,
            &self.sig05, &self.sig06, &self.sig07, &self.sig08,
            &self.sig09, &self.sig10, &self.sig11, &self.sig12,
            &self.sig13, &self.sig14, &self.sig15, &self.sig16,
            &self.sig17, &self.sig18, &self.sig19, &self.sig20,
            &self.sig21, &self.sig22, &self.sig23, &self.sig24,
            &self.sig25, &self.sig26, &self.sig27, &self.sig28,
            &self.sig29, &self.sig30, &self.sig31, &self.sig32,
        ]
    }
}

impl Default for SynthexisParams {
    fn default() -> Self {
        // Headless verification hook, e.g. SYNTHEXIS_TEST="test_signal=on".
        let test_on = std::env::var("SYNTHEXIS_TEST")
            .map(|v| v.contains("test_signal=on"))
            .unwrap_or(false);
        if test_on {
            crate::log::log("SYNTHEXIS_TEST applied: test_signal=on");
        }
        Self {
            sig01: signal_param(0), sig02: signal_param(1),
            sig03: signal_param(2), sig04: signal_param(3),
            sig05: signal_param(4), sig06: signal_param(5),
            sig07: signal_param(6), sig08: signal_param(7),
            sig09: signal_param(8), sig10: signal_param(9),
            sig11: signal_param(10), sig12: signal_param(11),
            sig13: signal_param(12), sig14: signal_param(13),
            sig15: signal_param(14), sig16: signal_param(15),
            sig17: signal_param(16), sig18: signal_param(17),
            sig19: signal_param(18), sig20: signal_param(19),
            sig21: signal_param(20), sig22: signal_param(21),
            sig23: signal_param(22), sig24: signal_param(23),
            sig25: signal_param(24), sig26: signal_param(25),
            sig27: signal_param(26), sig28: signal_param(27),
            sig29: signal_param(28), sig30: signal_param(29),
            sig31: signal_param(30), sig32: signal_param(31),
            live: BoolParam::new("Live", true),
            test_signal: BoolParam::new("Test signal", test_on),
            hand_role_left: EnumParam::new("Hand role left", HandRole::Parameter),
            hand_role_right: EnumParam::new("Hand role right", HandRole::Parameter),
            // the one fixed size (ui-redesign.md v3 2.8); not saved with the
            // project, so an old project opens at it too
            editor_state: EguiState::from_size(crate::editor::WIN_W, crate::editor::WIN_H),
            target_pin: Arc::new(RwLock::new(String::new())),
            routes: Arc::new(RwLock::new(String::new())),
            midi: Arc::new(RwLock::new(String::new())),
            pong: Arc::new(RwLock::new(String::new())),
            view: Arc::new(RwLock::new(String::new())),
            profiles: Arc::new(RwLock::new(String::new())),
        }
    }
}

use nih_plug_egui::EguiState;
use parking_lot::RwLock;
use std::sync::Arc;
