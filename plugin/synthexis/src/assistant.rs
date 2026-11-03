//! Mapping assistant entry, minimal: one process-wide worker thread loads
//! the frame parser when the sentence field is used (warm(), about 0.3 s,
//! well before a sentence is typed) and parses submitted sentences; the
//! chips of the sentence field are enforced on the frame as pinned spans
//! (synthexis_parser::pin) and the frames go to the log. Unused for IDLE,
//! the parser (about 350 MB) is dropped; a sentence sent while it loads
//! waits for it. No resolver or route changes yet
//! (docs/parser-runtime-plan.md).
//!
//! Model dir: $SYNTHEXIS_PARSER_DIR, else
//! ~/Library/Application Support/synthexis/parser, laid out as
//! tools/mapping/export_runtime.py writes it. Runs: $SYNTHEXIS_PARSER_RUNS,
//! default the v11 four-seed ensemble, Q8_0 encoders and the conv,pronoun
//! rules as its parser.json says (Noa's pick, 2026-11-02). Headless check:
//! SYNTHEXIS_PARSE="sentence" parses that sentence once loaded.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use synthexis_parser::{pin, Options, Parser, Pin};

use crate::log::log;

const DEFAULT_RUNS: &str = "p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3";

/// Unused this long, the parser is dropped (probes/plugin-settings, 5).
const IDLE: Duration = Duration::from_secs(180);

pub enum Job {
    /// The sentence field is in use: load the parser if it is not.
    Warm,
    Parse(String, Vec<Pin>),
}

static WORKER: OnceLock<Sender<Job>> = OnceLock::new();

/// Starts the worker once per process; it loads nothing until a job comes.
/// Never call from the audio thread (the first call spawns a thread).
pub fn start() -> &'static Sender<Job> {
    WORKER.get_or_init(|| {
        let (tx, rx) = unbounded();
        if let Err(e) = std::thread::Builder::new()
            .name("synthexis-assistant".into())
            .spawn(move || run(&rx))
        {
            log(&format!("assistant: worker not started: {e}"));
        }
        tx
    })
}

/// The sentence field is in use: have the parser ready (and keep it) for
/// the sentence that follows.
pub fn warm() {
    let _ = start().send(Job::Warm);
}

/// Queues a typed sentence with its chips; the frame shows up in the log.
pub fn submit(text: &str, pins: Vec<Pin>) {
    if start().send(Job::Parse(text.to_string(), pins)).is_err() {
        log("assistant: worker gone, sentence dropped");
    }
}

fn model_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_PARSER_DIR") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/parser")
}

/// The parser the worker uses (also the load-time probe's parser part).
pub fn load() -> Option<Parser> {
    let dir = model_dir();
    let runs = std::env::var("SYNTHEXIS_PARSER_RUNS").unwrap_or_else(|_| DEFAULT_RUNS.into());
    let names: Vec<&str> = runs.split(',').map(str::trim).collect();
    let t0 = Instant::now();
    match Parser::load(&dir, &names, &Options::default()) {
        Ok(p) => {
            log(&format!("assistant: parser {runs} loaded in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3));
            Some(p)
        }
        Err(e) => {
            log(&format!("assistant: parser {runs} not loaded from {}: {e:#}", dir.display()));
            None
        }
    }
}

fn run(rx: &Receiver<Job>) {
    let mut first = None;
    if let Ok(text) = std::env::var("SYNTHEXIS_PARSE") {
        first = load();
        if let Some(p) = &first {
            parse(p, &text, &[]);
        }
    }
    work(rx, IDLE, first, load, parse);
}

/// The worker loop: loads the parser with the first job, parses, and drops
/// it after `idle` without a job; a failed load is tried again with the
/// next job. Generic over the parser so the tests can use a stand-in.
fn work<P>(
    rx: &Receiver<Job>,
    idle: Duration,
    mut parser: Option<P>,
    load: impl Fn() -> Option<P>,
    mut parse: impl FnMut(&P, &str, &[Pin]),
) {
    loop {
        let job = if parser.is_some() {
            match rx.recv_timeout(idle) {
                Ok(job) => job,
                Err(RecvTimeoutError::Timeout) => {
                    parser = None;
                    log(&format!("assistant: parser dropped, unused for {} s", idle.as_secs()));
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(job) => job,
                Err(_) => return,
            }
        };
        if parser.is_none() {
            parser = load();
        }
        if let Job::Parse(text, pins) = job {
            match &parser {
                Some(p) => parse(p, &text, &pins),
                None => log(&format!("assistant: parser unavailable, \"{text}\" not parsed")),
            }
        }
    }
}

fn parse(parser: &Parser, text: &str, pins: &[Pin]) {
    let t0 = Instant::now();
    match parser.parse(text) {
        Ok(p) if pins.is_empty() => log(&format!(
            "assistant: {:.3} {:.0} ms \"{text}\" -> {}",
            f64::from(p.logp).exp(),
            t0.elapsed().as_secs_f64() * 1e3,
            p.frame().join(" ")
        )),
        Ok(p) => log(&format!(
            "assistant: {:.3} {:.0} ms \"{text}\" -> {} | pinned {} -> {}",
            f64::from(p.logp).exp(),
            t0.elapsed().as_secs_f64() * 1e3,
            p.frame().join(" "),
            pins.len(),
            pin(&p.items, pins).frame().join(" ")
        )),
        Err(e) => log(&format!("assistant: parse failed for \"{text}\": {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A stand-in parser that counts its loads and records what it parsed.
    fn worker(idle: Duration) -> (Sender<Job>, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let (tx, rx) = unbounded();
        let (loads, parsed) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
        let (l, p) = (loads.clone(), parsed.clone());
        std::thread::spawn(move || {
            let load = || {
                std::thread::sleep(Duration::from_millis(50));
                Some(l.fetch_add(1, Ordering::SeqCst) + 1)
            };
            work(&rx, idle, None, load, |gen: &usize, text: &str, _: &[Pin]| p.lock().unwrap().push(format!("{gen}:{text}")));
        });
        (tx, loads, parsed)
    }

    fn wait_for(parsed: &Mutex<Vec<String>>, n: usize) -> Vec<String> {
        for _ in 0..200 {
            if parsed.lock().unwrap().len() >= n {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        parsed.lock().unwrap().clone()
    }

    #[test]
    fn nothing_loads_before_the_field_is_used() {
        let (_tx, loads, _) = worker(Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(loads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_sentence_sent_while_the_parser_loads_is_parsed() {
        let (tx, loads, parsed) = worker(Duration::from_secs(60));
        tx.send(Job::Warm).unwrap();
        tx.send(Job::Parse("open the filter".into(), Vec::new())).unwrap();
        assert_eq!(wait_for(&parsed, 1), ["1:open the filter"]);
        assert_eq!(loads.load(Ordering::SeqCst), 1, "warm and parse load once");
    }

    #[test]
    fn the_parser_drops_when_unused_and_loads_again() {
        let (tx, loads, parsed) = worker(Duration::from_millis(150));
        tx.send(Job::Parse("a".into(), Vec::new())).unwrap();
        wait_for(&parsed, 1);
        // warm jobs keep it loaded
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(80));
            tx.send(Job::Warm).unwrap();
        }
        tx.send(Job::Parse("b".into(), Vec::new())).unwrap();
        assert_eq!(wait_for(&parsed, 2), ["1:a", "1:b"], "still the first load");
        std::thread::sleep(Duration::from_millis(400));
        tx.send(Job::Parse("c".into(), Vec::new())).unwrap();
        assert_eq!(wait_for(&parsed, 3), ["1:a", "1:b", "2:c"], "dropped after idle, loaded again");
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }
}
