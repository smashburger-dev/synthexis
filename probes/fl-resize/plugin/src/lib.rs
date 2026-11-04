//! fl resize probe: does the host honour a plug-in-initiated window resize?
//! The editor asks for 1200 x 720, 820 and 900 through
//! `EguiState::set_requested_size` (vendor patch 1) and shows what the egui
//! view and the host's NSView reach within 500 ms. Outputs silence.

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::Arc;

mod editor;
mod log;
mod mac;
pub mod probe;

/// The size a fresh instance opens at: smaller than every request, so each
/// request changes both width and height.
pub const START: (u32, u32) = (1000, 640);

#[derive(Params)]
pub struct ProbeParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
}

pub struct FlResizeProbe {
    params: Arc<ProbeParams>,
}

impl Default for FlResizeProbe {
    fn default() -> Self {
        log::log(&format!(
            "fl resize probe v{} build={}",
            env!("CARGO_PKG_VERSION"),
            env!("FL_RESIZE_PROBE_BUILD")
        ));
        Self {
            params: Arc::new(ProbeParams {
                editor_state: EguiState::from_size(START.0, START.1),
            }),
        }
    }
}

impl Plugin for FlResizeProbe {
    const NAME: &'static str = "fl resize probe";
    const VENDOR: &'static str = "synthexis";
    const URL: &'static str = "https://github.com/synthexis";
    const EMAIL: &'static str = "probe@synthexis.invalid";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Instrument: stereo out, no audio input.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::make_editor(self.params.editor_state.clone())
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel in buffer.as_slice() {
            channel.fill(0.0);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for FlResizeProbe {
    const CLAP_ID: &'static str = "com.synthexis.fl-resize-probe";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("synthexis probe: plug-in-initiated window resize");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Utility];
}

impl Vst3Plugin for FlResizeProbe {
    const VST3_CLASS_ID: [u8; 16] = *b"synthxsrsz000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(FlResizeProbe);
nih_export_vst3!(FlResizeProbe);
