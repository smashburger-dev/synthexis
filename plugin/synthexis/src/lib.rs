//! synthexis — camera-to-parameter macro plugin (Welle 3 skeleton).
//!
//! 32 "Signal N" macros driven by the shared camera engine through the
//! conditioning/routing/writer chain. VST3 parameter edits go out on the
//! main thread via the vendored host_context_available hook (H2: FL Studio
//! records main-thread edits with the window closed). Audio: stereo silence.

mod alias;
mod assign;
mod assistant;
mod cond;
mod editor;
mod engine;
mod fl;
mod log;
mod params;
mod route;
mod sense;
mod sentence;
mod signal;
mod writer;

use std::num::NonZeroU32;
use std::sync::Arc;

use nih_plug::prelude::*;

use params::SynthexisParams;
use route::NUM_MACROS;
use writer::Writer;

/// Offline perception bench over a recording (engine/bench.rs).
pub fn perception_bench(path: &str) -> i32 {
    engine::bench::run(path)
}

pub struct Synthexis {
    params: Arc<SynthexisParams>,
    writer: Arc<Writer>,
    macro_block: [f32; NUM_MACROS],
}

impl Default for Synthexis {
    fn default() -> Self {
        log::log(&format!(
            "synthexis v0.1.0 build={}",
            env!("SYNTHEXIS_BUILD")
        ));
        let params = Arc::new(SynthexisParams::default());
        let writer = Writer::new(params.clone());
        let engine = engine::engine();
        engine::Engine::subscribe(&(writer.clone() as Arc<dyn engine::SignalSubscriber>));
        engine::start_triangle(engine);
        Self {
            params,
            writer,
            macro_block: [0.0; NUM_MACROS],
        }
    }
}

impl Plugin for Synthexis {
    const NAME: &'static str = "synthexis";
    const VENDOR: &'static str = "synthexis";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::make_editor(
            self.params.clone(),
            engine::engine().camera.clone(),
            self.writer.clone(),
        )
    }

    fn host_context_available(&mut self, context: Arc<dyn GuiContext>) {
        self.writer.set_context(context);
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Not called during plugin scans; loads the parser in the background
        // and opens the FL bridge, so focus is tracked with the editor closed.
        assistant::start();
        fl::bridge();
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer<'_>,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Snapshot once per block (spec rule); silence for now.
        self.writer.read_snapshot(&mut self.macro_block);
        buffer.iter_samples().for_each(|mut s| {
            s.iter_mut().for_each(|v| *v = 0.0);
        });
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Synthexis {
    const CLAP_ID: &'static str = "com.synthexis.synthexis";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("camera-driven parameter macros");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::NoteEffect,
        ClapFeature::Analyzer,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Synthexis {
    // Randomly generated for this plugin (not the probe's id).
    const VST3_CLASS_ID: [u8; 16] = [
        0x5a, 0x1b, 0x9c, 0x2d, 0x4e, 0x6f, 0x8a, 0x0b,
        0xc1, 0xd2, 0xe3, 0xf4, 0x15, 0x26, 0x37, 0x48,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Tools,
    ];
}

nih_export_clap!(Synthexis);
nih_export_vst3!(Synthexis);
