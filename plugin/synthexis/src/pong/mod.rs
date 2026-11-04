//! Pong mode (spec preset.pong): pointing shoots a
//! projectile from the fingertip; it bounces in the camera frame and every
//! wall hit fires that side's action (a MIDI part once, or a parameter).
//!
//! One Pong per Writer. Its thread holds only a Weak<Pong> and ends when
//! the instance goes. The setup sits in a leaf lock (cloned out, never held
//! across another call); `on`, `paused`, `live` and `suspended` are
//! atomics, so needs() and process() take no lock.

pub mod arena;
pub mod run;
pub mod setup;
pub mod shot;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use crate::midi::now_ns;
use crate::writer::Writer;
use arena::to_preview;
use run::{Input, Out, Run};
use setup::Setup;

/// The thread's poll while Pong is on (the engine statics) and off.
const ON_WAIT: Duration = Duration::from_millis(2);
const OFF_WAIT: Duration = Duration::from_millis(250);

/// Where parameter walls write: FL through the bridge script; tests record.
pub trait Sink: Send + Sync {
    fn param(&self, key: &crate::fl::Key, index: u32, v: f32);
}

pub struct FlSink;

impl Sink for FlSink {
    fn param(&self, key: &crate::fl::Key, index: u32, v: f32) {
        crate::fl::bridge().write(key, index, v);
    }
}

/// What the window draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// Projectiles in preview coordinates with their shot's strength.
    pub shots: Vec<([f32; 2], f32)>,
    /// Per side (Side order) seconds since its last hit fired.
    pub since_hit: [Option<f32>; 4],
    pub fired: u64,
    pub hits: u64,
    /// Hits dropped by a side's 30 ms window.
    pub dropped_window: u64,
    /// Hit notes over the player's budget, all instances.
    pub dropped_budget: u64,
    pub paused: [bool; 2],
}

pub struct Pong {
    setup: Mutex<Arc<Setup>>,
    on: AtomicBool,
    paused: [AtomicBool; 2],
    /// Live as the thread last read it.
    live: AtomicBool,
    /// Between deactivate and the next process(): no shots.
    suspended: AtomicBool,
    run: Mutex<Run>,
    writer: OnceLock<Weak<Writer>>,
    sink: Arc<dyn Sink>,
    wake: Sender<()>,
    /// The thread's end, until spawn takes it.
    wake_rx: Mutex<Option<Receiver<()>>>,
}

impl Pong {
    /// A Pong without its thread (spawn starts it; tests tick by hand).
    pub fn new(sink: Arc<dyn Sink>) -> Arc<Pong> {
        let (wake, rx) = bounded(1);
        Arc::new(Pong {
            setup: Mutex::new(Arc::new(Setup::default())),
            on: AtomicBool::new(false),
            paused: [AtomicBool::new(false), AtomicBool::new(false)],
            live: AtomicBool::new(true),
            suspended: AtomicBool::new(false),
            run: Mutex::new(Run::default()),
            writer: OnceLock::new(),
            sink,
            wake,
            wake_rx: Mutex::new(Some(rx)),
        })
    }

    /// The writer whose player, macros and store the walls use.
    pub fn attach(&self, writer: &Arc<Writer>) {
        let _ = self.writer.set(Arc::downgrade(writer));
    }

    fn writer(&self) -> Option<Arc<Writer>> {
        self.writer.get().and_then(Weak::upgrade)
    }

    /// Starts the "synthexis-pong" thread once.
    pub fn spawn(this: &Arc<Pong>) -> Option<JoinHandle<()>> {
        let rx = this.wake_rx.lock().take()?;
        let weak = Arc::downgrade(this);
        std::thread::Builder::new()
            .name("synthexis-pong".into())
            .spawn(move || thread(weak, rx))
            .map_err(|e| crate::log::log(&format!("pong: thread not started: {e}")))
            .ok()
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    /// Hands [L, R] whose finger parts are paused.
    pub fn paused(&self) -> [bool; 2] {
        [0, 1].map(|h| self.paused[h].load(Ordering::Acquire))
    }

    /// deactivate: no shots; the arena clears and running ramps write lo
    /// now, as the instance may go before another tick.
    pub fn suspend(&self) {
        self.suspended.store(true, Ordering::Release);
        if let Some(w) = self.writer() {
            self.run.lock().stop(now_ns(), &Out { writer: &w, sink: &*self.sink });
        }
    }

    /// process() is called again. An atomic store, safe on the audio thread.
    pub fn resume(&self) {
        if self.suspended.load(Ordering::Relaxed) {
            self.suspended.store(false, Ordering::Release);
        }
    }

    fn running(&self, s: &Setup) -> bool {
        s.on && self.live.load(Ordering::Acquire) && !self.suspended.load(Ordering::Acquire)
    }

    pub fn setup(&self) -> Arc<Setup> {
        self.setup.lock().clone()
    }

    /// The setup from the window, a preset or the project: clamped,
    /// stored, written to params `pong`. Pong off clears; a side with
    /// another action ends its ramp; the shooting hands' fingers pause or
    /// play again; the store's walls follow the setup's.
    pub fn set_setup(&self, s: Setup) {
        let s = Arc::new(s.clamped());
        let old = std::mem::replace(&mut *self.setup.lock(), s.clone());
        self.on.store(s.on, Ordering::Release);
        let paused = [0, 1].map(|h| s.on && s.pause_fingers && s.hands.has(h));
        for (a, p) in self.paused.iter().zip(paused) {
            a.store(p, Ordering::Release);
        }
        if let Some(w) = self.writer() {
            *w.params.pong.write() = s.save();
            let now = now_ns();
            if old.on && !s.on {
                self.clear_with(&w, now);
            } else {
                let mut run = self.run.lock();
                let out = Out { writer: &w, sink: &*self.sink };
                for i in (0..4).filter(|&i| old.walls[i] != s.walls[i]) {
                    run.end_side(i, now, &out);
                }
                if !old.on && s.on {
                    run.restart();
                }
            }
            w.set_paused(paused);
            let store = w.player.store();
            if store.walls != s.wall_parts() {
                w.update_midi(store);
            }
        }
        let _ = self.wake.try_send(());
    }

    /// Projectiles gone, hit notes ended, running ramps at lo (Pong off,
    /// a preset load).
    pub fn clear(&self) {
        match self.writer() {
            Some(w) => self.clear_with(&w, now_ns()),
            None => self.run.lock().arena.clear(),
        }
    }

    fn clear_with(&self, w: &Writer, now: u64) {
        {
            self.run.lock().stop(now, &Out { writer: w, sink: &*self.sink });
        }
        w.player.end_hits();
    }

    /// `n` shots from the frame centre (the window's Test shot and Burst),
    /// while Pong runs.
    pub fn test_shots(&self, n: usize) {
        let s = self.setup();
        if self.running(&s) {
            self.run.lock().test_shots(n, &s);
            let _ = self.wake.try_send(());
        }
    }

    pub fn view(&self) -> View {
        let now = now_ns();
        let run = self.run.lock();
        View {
            shots: run.arena.shots().iter().map(|p| (to_preview(p.pos), p.strength)).collect(),
            since_hit: run.since_hit(now),
            fired: run.fired,
            hits: run.hits,
            dropped_window: run.dropped_window,
            dropped_budget: crate::midi::hits_dropped(),
            paused: self.paused(),
        }
    }

    /// One tick at `now` with `input` (the thread's, or a test's).
    pub fn tick(&self, now: u64, input: &Input) {
        let Some(w) = self.writer() else { return };
        self.live.store(input.live, Ordering::Release);
        let s = self.setup();
        // read under the run lock: a suspend's stop cannot come in between
        let mut run = self.run.lock();
        let running = self.running(&s);
        run.tick(now, input, &s, running, &Out { writer: &w, sink: &*self.sink });
    }

    /// The thread's tick: engine statics and Live now.
    fn poll(&self) {
        let Some(live) = self.writer().map(|w| w.params.live.value()) else { return };
        self.tick(now_ns(), &Input::read(live));
    }
}

/// Polls while Pong is on; ends once the Pong is gone.
fn thread(weak: Weak<Pong>, rx: Receiver<()>) {
    loop {
        let wait = {
            let Some(p) = weak.upgrade() else { return };
            if p.is_on() {
                p.poll();
                ON_WAIT
            } else {
                OFF_WAIT
            }
        };
        if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(wait) {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
