//! Process-wide 30 Hz pump thread "synthexis-pump".
//!
//! Each tick it computes the drive value from the selected source and pushes
//! it to the host through one of three write paths:
//!
//! - A: `ParamSetter` calls directly on this background thread.
//! - B: coalesced dispatch to the main GCD queue (latest value wins, at most
//!   one queued value-write block plus control blocks, so a stalled main
//!   thread never builds a backlog).
//! - D: MIDI CC 20 on channel 1 through the "synthexis" virtual CoreMIDI
//!   source; no GuiContext needed.
//!
//! `gesture` selects whether every write is wrapped in begin/end (PerWrite)
//! or whether a single gesture stays open while writing (Held). Begin/end
//! run on the same thread as the writes for that path.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use nih_plug::prelude::*;
use objc2_foundation::NSThread;
use parking_lot::Mutex;

use crate::camera::CaptureShared;
use crate::editor::EDITOR_OPEN;
use crate::{midi, probe_log, Drive, Gesture, ProbeParams, Source, WritePath};

const TICK: Duration = Duration::from_micros(1_000_000 / 30);
const TRIANGLE_PERIOD: Duration = Duration::from_secs(4);
const CC_RESEND: Duration = Duration::from_secs(1);
const CC_NUMBER: u8 = 20;
const CC_CHANNEL: u8 = 0; // channel 1

static SHARED: OnceLock<Arc<CaptureShared>> = OnceLock::new();
/// f32 bits of the newest value a main-queue write block should apply.
static LATEST_BITS: AtomicU32 = AtomicU32::new(0);
/// Kind of the most recently queued main-queue block; identical consecutive
/// writes coalesce onto it (see `submit_main`).
static PENDING_OP: AtomicU8 = AtomicU8::new(NONE);

const NONE: u8 = 0;
const OP_FULL: u8 = 1; // begin + set(latest) + end
const OP_SET: u8 = 2; // set(latest), gesture already open
const OP_BEGIN: u8 = 3;
const OP_END: u8 = 4;

static NOOPS: AtomicUsize = AtomicUsize::new(0);
static PANICS: AtomicUsize = AtomicUsize::new(0);
static LAST_NOOP_LOG: Mutex<Option<Instant>> = Mutex::new(None);

/// Rolling one-second window of completed writes.
struct WriteWindow {
    count: u32,
    min: f32,
    max: f32,
    thread: String,
}

static WINDOW: Mutex<WriteWindow> = Mutex::new(WriteWindow {
    count: 0,
    min: 0.0,
    max: 0.0,
    thread: String::new(),
});
static LAST_LOGGED: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static LAST_WRITE_LINE: Mutex<String> = Mutex::new(String::new());

/// Latest `write ...` log line for the editor.
pub fn last_write_line() -> String {
    LAST_WRITE_LINE.lock().clone()
}

fn noop(msg: &str) {
    NOOPS.fetch_add(1, Ordering::Relaxed);
    let mut last = LAST_NOOP_LOG.lock();
    if last.map(|t| t.elapsed().as_secs() > 30).unwrap_or(true) {
        probe_log::log(msg);
        *last = Some(Instant::now());
    }
}

fn record_write(value: f32) {
    let mut w = WINDOW.lock();
    w.count += 1;
    if w.count == 1 {
        w.min = value;
        w.max = value;
    } else {
        w.min = w.min.min(value);
        w.max = w.max.max(value);
    }
    w.thread = thread_name();
}

fn thread_name() -> String {
    if NSThread::isMainThread_class() {
        return "main".into();
    }
    let t = std::thread::current();
    if let Some(name) = t.name() {
        return name.to_string();
    }
    format!("tid={:?}", t.id())
}

fn triangle(elapsed: Duration) -> f32 {
    let phase = elapsed.as_secs_f64() % TRIANGLE_PERIOD.as_secs_f64();
    let half = TRIANGLE_PERIOD.as_secs_f64() / 2.0;
    if phase < half {
        (phase / half) as f32
    } else {
        (2.0 - phase / half) as f32
    }
}

fn source_value(params: &ProbeParams, start: Instant) -> f32 {
    let v = match params.source.value() {
        Source::Luminance => SHARED
            .get()
            .map(|s| s.luminance())
            .unwrap_or(0.0),
        Source::Triangle => triangle(start.elapsed()),
    };
    v.clamp(0.0, 1.0)
}

/// Run one begin/set/end or set/begin/end step with panic isolation.
/// `op` is `OP_FULL`, `OP_SET`, `OP_BEGIN` or `OP_END`.
fn run_op(
    op: u8,
    ctx: &Arc<dyn GuiContext>,
    params: &Arc<ProbeParams>,
    value: f32,
) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let setter = ParamSetter::new(ctx.as_ref());
        match op {
            OP_FULL => {
                setter.begin_set_parameter(&params.signal_1);
                setter.set_parameter(&params.signal_1, value);
                setter.end_set_parameter(&params.signal_1);
            }
            OP_SET => setter.set_parameter(&params.signal_1, value),
            OP_BEGIN => setter.begin_set_parameter(&params.signal_1),
            OP_END => setter.end_set_parameter(&params.signal_1),
            _ => {}
        }
    }));
    if let Err(e) = result {
        let n = PANICS.fetch_add(1, Ordering::Relaxed) + 1;
        probe_log::log(&format!("pump: param write panicked ({e:?}) — #{n}"));
    }
    if op == OP_FULL || op == OP_SET {
        record_write(value);
    }
}

/// Queue `op` on the main GCD queue. Value ops read the freshest value at
/// run time; identical consecutive ops coalesce onto the pending block so a
/// stalled main thread keeps at most a handful of queued blocks.
fn submit_main(
    op: u8,
    ctx: Arc<dyn GuiContext>,
    params: Arc<ProbeParams>,
    value: f32,
) {
    if op == OP_FULL || op == OP_SET {
        LATEST_BITS.store(value.to_bits(), Ordering::Relaxed);
    }
    if PENDING_OP.swap(op, Ordering::SeqCst) == op {
        return;
    }
    DispatchQueue::main().exec_async(move || {
        // Clear the marker; a different-kind block may already be queued
        // behind us and its kind lives in the captured `op`, not here.
        PENDING_OP.store(0, Ordering::SeqCst);
        let latest = f32::from_bits(LATEST_BITS.load(Ordering::Relaxed));
        run_op(op, &ctx, &params, latest);
    });
}

fn submit(
    path: WritePath,
    op: u8,
    ctx: &Arc<dyn GuiContext>,
    params: &Arc<ProbeParams>,
    value: f32,
) {
    match path {
        WritePath::Main => submit_main(op, ctx.clone(), params.clone(), value),
        WritePath::Background => run_op(op, ctx, params, value),
        WritePath::MidiCc => {}
    }
}

struct PumpState {
    start: Instant,
    /// Path whose held gesture is currently open, if any. Only ever a
    /// GuiContext path (Background/Main); path D has no gestures.
    held_open: Option<WritePath>,
    last_cc: Option<u8>,
    last_cc_sent: Option<Instant>,
}

impl PumpState {
    /// End an open held gesture on its own path's thread.
    fn close_held(&mut self) {
        let Some(path) = self.held_open.take() else {
            return;
        };
        if let (Some(params), Some(ctx)) = (crate::params(), crate::gui_context()) {
            submit(path, OP_END, &ctx, params, 0.0);
        }
    }

    fn tick(&mut self) {
        let Some(params) = crate::params() else {
            return;
        };
        let drive = params.drive.value();
        let gesture = params.gesture.value();
        let path = params.path.value();
        let want = match drive {
            Drive::Off => false,
            Drive::Always => true,
            Drive::EditorOnly => EDITOR_OPEN.load(Ordering::Acquire),
        };

        if self.held_open.is_some()
            && (!want || gesture != Gesture::Held || self.held_open != Some(path))
        {
            self.close_held();
        }

        if !want {
            return;
        }

        let value = source_value(params, self.start);
        match path {
            WritePath::MidiCc => {
                let v7 = (value * 127.0).round().clamp(0.0, 127.0) as u8;
                let resend = self
                    .last_cc_sent
                    .map(|t| t.elapsed() >= CC_RESEND)
                    .unwrap_or(true);
                if self.last_cc != Some(v7) || resend {
                    let status = midi::send_cc(CC_CHANNEL, CC_NUMBER, v7);
                    if status == 0 {
                        self.last_cc = Some(v7);
                        self.last_cc_sent = Some(Instant::now());
                        record_write(value);
                    } else {
                        noop(&format!("pump: MIDI CC send failed status={status}"));
                    }
                }
            }
            WritePath::Background | WritePath::Main => {
                let Some(ctx) = crate::gui_context() else {
                    noop("pump: no GuiContext captured yet (open the editor once)");
                    return;
                };
                match gesture {
                    Gesture::PerWrite => {
                        submit(path, OP_FULL, &ctx, params, value);
                    }
                    Gesture::Held => {
                        if self.held_open.is_none() {
                            self.held_open = Some(path);
                            submit(path, OP_BEGIN, &ctx, params, value);
                        }
                        submit(path, OP_SET, &ctx, params, value);
                    }
                }
            }
        }
    }
}

fn emit_write_log(elapsed: Duration) {
    let Some(params) = crate::params() else {
        return;
    };
    let (count, min, max, thread) = {
        let mut w = WINDOW.lock();
        let out = (w.count, w.min, w.max, std::mem::take(&mut w.thread));
        w.count = 0;
        out
    };
    let mut last = LAST_LOGGED.lock();
    if count > 0 {
        *last = (min, max);
    }
    let (min, max) = *last;
    drop(last);

    let api = crate::gui_context()
        .map(|ctx| format!("{:?}", ctx.plugin_api()))
        .unwrap_or_else(|| "n/a".into());
    let path = params.path.value();
    let line = format!(
        "write path={} api={} thread={} writes_per_s={} min={min:.3} max={max:.3} \
         noops={} panics={} editor_open={} drive={} source={} gesture={}",
        path.letter(),
        api,
        if thread.is_empty() { "none" } else { thread.as_str() },
        (count as f64 / elapsed.as_secs_f64()).round() as u32,
        NOOPS.load(Ordering::Relaxed),
        PANICS.load(Ordering::Relaxed),
        EDITOR_OPEN.load(Ordering::Acquire),
        drive_id(params.drive.value()),
        source_id(params.source.value()),
        gesture_id(params.gesture.value()),
    );
    *LAST_WRITE_LINE.lock() = line.clone();
    probe_log::log(&line);
}

fn drive_id(d: Drive) -> &'static str {
    match d {
        Drive::Off => "off",
        Drive::EditorOnly => "editor",
        Drive::Always => "always",
    }
}

fn source_id(s: Source) -> &'static str {
    match s {
        Source::Luminance => "lum",
        Source::Triangle => "tri",
    }
}

fn gesture_id(g: Gesture) -> &'static str {
    match g {
        Gesture::PerWrite => "per_write",
        Gesture::Held => "held",
    }
}

fn pump_loop() {
    let mut state = PumpState {
        start: Instant::now(),
        held_open: None,
        last_cc: None,
        last_cc_sent: None,
    };
    let mut next = Instant::now();
    let mut last_log = Instant::now();
    loop {
        next += TICK;
        let now = Instant::now();
        if next < now - Duration::from_secs(1) {
            // Far behind (e.g. after a suspend): resync instead of bursting.
            next = now;
        }
        std::thread::sleep(next.saturating_duration_since(now));

        state.tick();

        if last_log.elapsed() >= Duration::from_secs(1) {
            let elapsed = last_log.elapsed();
            if let Some(params) = crate::params() {
                if params.drive.value() != Drive::Off {
                    emit_write_log(elapsed);
                }
            }
            last_log = Instant::now();
        }
    }
}

/// Start the process-wide pump thread. Called once by the camera engine.
pub fn start(shared: Arc<CaptureShared>) {
    static START: OnceLock<()> = OnceLock::new();
    START.get_or_init(|| {
        if SHARED.set(shared).is_err() {
            return;
        }
        match std::thread::Builder::new()
            .name("synthexis-pump".into())
            .spawn(pump_loop)
        {
            Ok(_) => probe_log::log("pump thread started (30 Hz)"),
            Err(e) => probe_log::log(&format!("pump thread spawn failed: {e}")),
        }
    });
}
