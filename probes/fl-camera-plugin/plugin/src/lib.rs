//! synthexis probe — camera-to-parameter + MIDI-out test plugin.
//!
//! Covers probes P1 (camera capture inside a plugin), P2 (automation written
//! from a background thread through GuiContext) and P6 (sample-accurate MIDI
//! out on the beat grid). Outputs silence; sits in the FL Channel Rack as an
//! instrument.

mod camera;
mod editor;
mod probe_log;

use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use parking_lot::Mutex;

static GUI_CONTEXT: Mutex<Option<Arc<dyn GuiContext>>> = Mutex::new(None);
static PARAMS: OnceLock<Arc<ProbeParams>> = OnceLock::new();
/// Number of param-set failures/no-ops so far (also logged).
static PUMP_NOOPS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn gui_context_slot() -> &'static Mutex<Option<Arc<dyn GuiContext>>> {
    &GUI_CONTEXT
}

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
pub enum Drive {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "editor"]
    #[name = "Editor open only"]
    EditorOnly,
    #[id = "always"]
    #[name = "Always"]
    Always,
}

#[derive(Params)]
pub struct ProbeParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// Written every frame by the camera thread, 0..1.
    #[id = "signal1"]
    pub signal_1: FloatParam,

    #[id = "drive"]
    pub drive: EnumParam<Drive>,

    #[id = "midi_test"]
    pub midi_test: BoolParam,
}

impl Default for ProbeParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(640, 520),
            signal_1: FloatParam::new(
                "Signal 1",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(10.0)),
            drive: EnumParam::new("Drive", Drive::Off),
            midi_test: BoolParam::new("MIDI test", false),
        }
    }
}

/// Called from the camera thread at frame rate with the latest luminance.
/// This is the P2 experiment: does FL record automation when the host-visible
/// param setter is driven off the GUI thread?
pub(crate) fn pump_signal_param(value: f32) {
    let Some(params) = PARAMS.get() else {
        return;
    };
    let drive = params.drive.value();
    if drive == Drive::Off {
        return;
    }
    if drive == Drive::EditorOnly && !editor::EDITOR_OPEN.load(Ordering::Acquire) {
        return;
    }
    let ctx = GUI_CONTEXT.lock().clone();
    let Some(ctx) = ctx else {
        // No editor has ever been opened; expected no-op. Log at most once
        // per 30 s so the file doesn't flood.
        static LAST_LOG: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        let mut g = LAST_LOG.lock();
        if g.map(|t| t.elapsed().as_secs() > 30).unwrap_or(true) {
            probe_log::log("pump: no GuiContext captured yet (open the editor once)");
            *g = Some(std::time::Instant::now());
        }
        PUMP_NOOPS.fetch_add(1, Ordering::Relaxed);
        return;
    };

    let setter = ParamSetter::new(ctx.as_ref());
    let result = catch_unwind(AssertUnwindSafe(|| {
        setter.begin_set_parameter(&params.signal_1);
        setter.set_parameter(&params.signal_1, value);
        setter.end_set_parameter(&params.signal_1);
    }));
    if let Err(e) = result {
        let n = PUMP_NOOPS.fetch_add(1, Ordering::Relaxed) + 1;
        probe_log::log(&format!("pump: param set panicked ({e:?}) — #{n}"));
    }
}

pub struct SynthexisProbe {
    params: Arc<ProbeParams>,
    shared: Arc<camera::CaptureShared>,
    /// Beat-time (quarter notes) of the next note-on to emit.
    next_note_on_beat: f64,
    /// Pending note-off: (beat at which it is due).
    pending_note_off: Option<f64>,
}

impl Default for SynthexisProbe {
    fn default() -> Self {
        let params = Arc::new(ProbeParams::default());
        let _ = PARAMS.set(params.clone());
        let engine = camera::start();
        Self {
            params,
            shared: engine.shared.clone(),
            next_note_on_beat: 0.0,
            pending_note_off: None,
        }
    }
}

impl Plugin for SynthexisProbe {
    const NAME: &'static str = "synthexis probe";
    const VENDOR: &'static str = "synthexis";
    const URL: &'static str = "https://github.com/synthexis";
    const EMAIL: &'static str = "probe@synthexis.invalid";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Instrument: stereo out, no audio input. Outputs silence.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::make_editor(self.params.clone(), self.shared.clone())
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Keep output silent regardless of what the host left in the buffer.
        for channel in buffer.as_slice() {
            channel.fill(0.0);
        }

        if !self.params.midi_test.value() {
            self.pending_note_off = None;
            return ProcessStatus::Normal;
        }

        let transport = context.transport();
        if !transport.playing {
            return ProcessStatus::Normal;
        }
        let (Some(pos_beats), Some(tempo)) = (transport.pos_beats(), transport.tempo) else {
            return ProcessStatus::Normal;
        };
        let sample_rate = transport.sample_rate as f64;
        let num_samples = buffer.samples() as f64;
        let beats_per_sample = tempo / 60.0 / sample_rate;
        let block_end_beats = pos_beats + num_samples * beats_per_sample;

        // Resync if the transport jumped (loop, seek, re-trigger).
        if self.next_note_on_beat < pos_beats
            || self.next_note_on_beat >= block_end_beats + 4.0
        {
            self.next_note_on_beat = pos_beats.ceil();
        }

        // Note-on at every beat start inside this block.
        while self.next_note_on_beat < block_end_beats {
            let offset =
                ((self.next_note_on_beat - pos_beats) / beats_per_sample).round();
            if offset >= 0.0 && offset < num_samples {
                context.send_event(NoteEvent::NoteOn {
                    timing: offset as u32,
                    voice_id: None,
                    channel: 0,
                    note: 60,
                    velocity: 100.0 / 127.0,
                });
                self.pending_note_off = Some(self.next_note_on_beat + 0.125);
            }
            self.next_note_on_beat += 1.0;
        }

        // Note-off 1/8 beat after the note-on, possibly in a later block.
        if let Some(off_beat) = self.pending_note_off {
            if off_beat < block_end_beats {
                let offset = ((off_beat - pos_beats) / beats_per_sample).round();
                if offset >= 0.0 && offset < num_samples {
                    context.send_event(NoteEvent::NoteOff {
                        timing: offset as u32,
                        voice_id: None,
                        channel: 0,
                        note: 60,
                        velocity: 0.0,
                    });
                }
                self.pending_note_off = None;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for SynthexisProbe {
    const CLAP_ID: &'static str = "com.synthexis.probe";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("synthexis hardware probe: camera->param and MIDI out");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Utility];
}

impl Vst3Plugin for SynthexisProbe {
    const VST3_CLASS_ID: [u8; 16] = *b"synthxsprb000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(SynthexisProbe);
nih_export_vst3!(SynthexisProbe);
