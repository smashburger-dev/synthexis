//! synthexis — camera-to-parameter macro plugin (Welle 3 skeleton).
//!
//! 32 "Signal N" macros driven by the shared camera engine through the
//! conditioning/routing/writer chain. VST3 parameter edits go out on the
//! main thread via the vendored host_context_available hook (H2: FL Studio
//! records main-thread edits with the window closed). Audio: stereo silence.

mod alias;
mod assign;
mod assistant;
mod backdrop;
mod calib;
mod cond;
mod edges;
mod editor;
mod engine;
mod fingers;
mod fl;
mod glass;
mod head_ui;
mod i18n;
mod log;
mod midi;
mod param_find;
mod params;
mod pong;
mod pong_ui;
mod preset;
mod route;
mod saved;
mod sense;
mod sentence;
mod settings;
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

/// Load-time probe (probes/plugin-settings): loads parser, completion data
/// and the perception backend on their own threads, as an instance does,
/// and prints when each was ready, in ms from the start, as one JSON line
/// (completion: the vocabulary, typing can start; labels: the picker's
/// classes, loaded after it on the same thread). No camera (it needs the
/// host's permission).
pub fn load_probe() -> i32 {
    let t0 = std::time::Instant::now();
    let ms = move || (t0.elapsed().as_secs_f64() * 1e3).round();
    let parser = std::thread::spawn(move || (assistant::load().is_some(), ms()));
    let completion = std::thread::spawn(move || {
        let vocab = (sentence::load_vocab().is_ok(), ms());
        (vocab, (sentence::load_labels().1.is_empty(), ms()))
    });
    let perception = std::thread::spawn(move || (engine::perception::load_backend().is_some(), ms()));
    let failed = (false, -1.0);
    let (vocab, labels) = completion.join().unwrap_or((failed, failed));
    let parts = [
        ("parser", parser.join().unwrap_or(failed)),
        ("completion", vocab),
        ("labels", labels),
        ("perception", perception.join().unwrap_or(failed)),
    ];
    let mut out = serde_json::Map::new();
    for (name, (_, at)) in parts {
        out.insert(format!("{name}_ms"), serde_json::json!(at));
    }
    out.insert("all_ms".into(), serde_json::json!(ms()));
    println!("{}", serde_json::Value::Object(out));
    i32::from(!parts.iter().all(|(_, (ok, _))| *ok))
}

/// This process's physical footprint in MB, what Activity Monitor shows
/// as its memory (mem_probe).
#[allow(unsafe_code)]
fn footprint_mb() -> f64 {
    /// struct rusage_info_v0 of <sys/resource.h>.
    #[repr(C)]
    struct RusageInfoV0 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
    }
    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV0) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<RusageInfoV0>::zeroed();
    // SAFETY: flavor 0 (RUSAGE_INFO_V0) writes exactly one rusage_info_v0
    // into the buffer; on failure it stays zeroed.
    let info = unsafe {
        proc_pid_rusage(std::process::id() as i32, 0, info.as_mut_ptr());
        info.assume_init()
    };
    info.phys_footprint as f64 / 1_048_576.0
}

/// Memory probe (probes/plugin-settings, section 5): the footprint before,
/// with and after each on-demand part, for `cycles` load/drop cycles, one
/// JSON line per cycle; then the perception backend once.
pub fn mem_probe(cycles: usize) -> i32 {
    // the allocator hands freed memory back to the system only now and
    // then (RESULT.md, section 5): optionally wait before each reading
    let settle = std::env::var("SYNTHEXIS_MEMPROBE_SETTLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mb = || {
        std::thread::sleep(std::time::Duration::from_millis(settle));
        (footprint_mb() * 10.0).round() / 10.0
    };
    println!("{}", serde_json::json!({"base_mb": mb()}));
    for cycle in 1..=cycles {
        let t0 = std::time::Instant::now();
        let parser = assistant::load();
        let parser_ms = t0.elapsed().as_millis();
        let with_parser = mb();
        drop(parser);
        let no_parser = mb();
        let t0 = std::time::Instant::now();
        let labels = sentence::load_labels();
        let labels_ms = t0.elapsed().as_millis();
        let with_labels = mb();
        drop(labels);
        println!(
            "{}",
            serde_json::json!({"cycle": cycle, "parser_ms": parser_ms, "with_parser_mb": with_parser, "after_parser_mb": no_parser,
                "labels_ms": labels_ms, "with_labels_mb": with_labels, "after_labels_mb": mb()})
        );
    }
    let backend = engine::perception::load_backend();
    println!("{}", serde_json::json!({"with_perception_mb": mb()}));
    i32::from(backend.is_none())
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
    const VERSION: &'static str = "0.1.4";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    // Unused: process() reads no events. An event input bus is needed
    // because Live 12 refuses a VST3 instrument without one
    // (clap-wrapper#635, docs/host-support.md).
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
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
        // Not called during plugin scans; loads the perception models in
        // the background (about 15 MB, kept so perception resumes at once;
        // they no longer wait for the first camera frame) and opens the FL
        // bridge, so focus is tracked with the editor closed. The parser
        // loads when the sentence field is used (assistant.rs).
        engine::set_finger_thresholds(settings::store().get().fingers);
        engine::perception::start();
        self.writer.follow_channels(fl::bridge());
        saved::run(&saved::load(&self.params.routes.read()), &self.writer);
        let store = midi::Store::load(&self.params.midi.read());
        self.writer.set_midi(store);
        // after the store: set_setup gives it the walls
        let setup = pong::setup::Setup::load(&self.params.pong.read());
        self.writer.pong.set_setup(setup);
        // a window open before this shows the kept lines and parts now
        self.writer.bump_state();
        true
    }

    fn deactivate(&mut self) {
        // nothing may hang while FL does not call process(); no shots
        // until it does again
        self.writer.pong.suspend();
        self.writer.player.stop_all();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer<'_>,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Snapshot once per block (spec rule); silence for now. The host
        // transport goes to the MIDI player without locks (midi.rs).
        self.writer.read_snapshot(&mut self.macro_block);
        self.writer.pong.resume();
        let t = context.transport();
        self.writer.player.transport.store(t.playing, t.tempo, t.pos_beats(), midi::now_ns());
        buffer.iter_samples().for_each(|mut s| {
            s.iter_mut().for_each(|v| *v = 0.0);
        });
        // a silent generator may be switched off by FL's Smart Disable;
        // the transport has to keep coming while parts play
        ProcessStatus::KeepAlive
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
// The Audio Unit (synthexis.component, bundled by xtask) is the same
// binary: clap-wrapper's AUv2 factory hosts the CLAP plugin above.
clap_wrapper::export_auv2!();
