//! Mapping assistant entry: one process-wide worker thread loads the frame
//! parser when the sentence field is used (warm(), about 0.3 s, well before
//! a sentence is typed) and parses submitted sentences; the chips of the
//! sentence field are enforced on the frame as pinned spans
//! (synthexis_parser::pin), the resolver binds the targets to the picked
//! plugin's parameters (synthexis_parser::resolve), and one line per route
//! ("left hand openness -> Topos / Drive") goes back to the field and to
//! the log; the field turns the routes into pending routes (sentence.rs).
//! Unused for IDLE, the parser (about 350 MB) is dropped; a sentence sent
//! while it loads waits for it.
//!
//! Model dir: $SYNTHEXIS_PARSER_DIR, else
//! ~/Library/Application Support/synthexis/parser, laid out as
//! tools/mapping/export_runtime.py writes it. Runs: $SYNTHEXIS_PARSER_RUNS,
//! default the v11 four-seed ensemble, Q8_0 encoders and the conv,pronoun
//! rules as its parser.json says (Noa's pick, 2026-11-02). Headless check:
//! SYNTHEXIS_PARSE="sentence" parses that sentence once loaded.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use synthexis_complete::Param;
use synthexis_parser::resolve::{self, resolve, Candidate};
use synthexis_parser::pin::Part;
use synthexis_parser::{pin_spans, Item, Options, Parser, Pin};

use crate::log::log;

const DEFAULT_RUNS: &str = "p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3";

/// Unused this long, the parser is dropped (probes/plugin-settings, 5).
const IDLE: Duration = Duration::from_secs(180);

/// What the resolver binds against: the picked plugin.
pub struct Scope {
    /// fl::Key id, as chip pins carry it.
    pub key: String,
    /// Plugin name for the result line.
    pub plugin: String,
    pub params: Arc<Vec<Param>>,
    /// alias key -> parameter name (alias.rs).
    pub aliases: HashMap<String, String>,
}

/// The assistant's answer to one sentence.
pub struct Reply {
    /// The frame's intent token without "I:" (map, unmap, clear, undo, ...).
    pub intent: String,
    /// Frame tokens outside the routes ("scope:all", a source a "clear"
    /// names), without the intent.
    pub rest: Vec<String>,
    pub routes: Vec<resolve::Route>,
    /// One line per route, or why there is none.
    pub lines: Vec<String>,
}

/// One submitted sentence.
pub struct Request {
    text: String,
    pins: Vec<Pin>,
    scope: Option<Scope>,
    reply: Sender<Reply>,
}

pub enum Job {
    /// The sentence field is in use: load the parser if it is not.
    Warm,
    Parse(Request),
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

/// Queues a typed sentence with its chips. The receiver gets the reply
/// once parsed; it disconnects when the parser is unavailable.
pub fn submit(text: &str, pins: Vec<Pin>, scope: Option<Scope>) -> Receiver<Reply> {
    let (reply, rx) = bounded(1);
    if start().send(Job::Parse(Request { text: text.to_string(), pins, scope, reply })).is_err() {
        log("assistant: worker gone, sentence dropped");
    }
    rx
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
            parse(p, &text, &[], None);
        }
    }
    work(rx, IDLE, first, load, parse);
}

/// The worker loop: loads the parser with the first job, parses, and drops
/// it after `idle` without a job; a failed load is tried again with the
/// next job, and that sentence's receiver disconnects. Generic over the
/// parser so the tests can use a stand-in.
fn work<P>(
    rx: &Receiver<Job>,
    idle: Duration,
    mut parser: Option<P>,
    load: impl Fn() -> Option<P>,
    mut parse: impl FnMut(&P, &str, &[Pin], Option<&Scope>) -> Reply,
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
        if let Job::Parse(r) = job {
            match &parser {
                Some(p) => {
                    let _ = r.reply.send(parse(p, &r.text, &r.pins, r.scope.as_ref()));
                }
                None => log(&format!("assistant: parser unavailable, \"{}\" not parsed", r.text)),
            }
        }
    }
}

/// Parses, pins, resolves; logs the frame and returns the reply (also the
/// tests' path to real parser output, assign.rs).
pub(crate) fn parse(parser: &Parser, text: &str, pins: &[Pin], scope: Option<&Scope>) -> Reply {
    let t0 = Instant::now();
    let p = match parser.parse(text) {
        Ok(p) => p,
        Err(e) => {
            log(&format!("assistant: parse failed for \"{text}\": {e:#}"));
            return Reply { intent: "none".into(), rest: Vec::new(), routes: Vec::new(), lines: vec![format!("parse failed: {e}")] };
        }
    };
    let parsed_ms = t0.elapsed().as_secs_f64() * 1e3;
    let frame = pin_spans(&p.items, text, &p.spans, pins);
    let cands: Vec<Candidate> = scope
        .map(|s| s.params.iter().map(|p| Candidate { index: p.index, name: &p.name, class: &p.class }).collect())
        .unwrap_or_default();
    let none = HashMap::new();
    let (key, plugin, aliases) = scope.map_or(("", "no plugin", &none), |s| (s.key.as_str(), s.plugin.as_str(), &s.aliases));
    let routes = resolve(&frame, text, &p.spans, key, &cands, aliases);
    let mut lines: Vec<String> = routes.iter().map(|r| r.describe(plugin)).collect();
    let intent = frame.frame().into_iter().find_map(|t| t.strip_prefix("I:").map(str::to_string)).unwrap_or_default();
    if lines.is_empty() {
        lines.push(format!("no route ({intent})"));
    }
    let pinned = if pins.is_empty() { String::new() } else { format!(" | pinned {} -> {}", pins.len(), frame.frame().join(" ")) };
    log(&format!(
        "assistant: {:.3} {parsed_ms:.0} ms \"{text}\" -> {}{pinned} | resolved on {plugin} ({} params): {}",
        f64::from(p.logp).exp(),
        p.frame().join(" "),
        cands.len(),
        lines.join("; ")
    ));
    let rest = frame
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Item(Item::Token(t)) if !t.starts_with("I:") => Some(t.clone()),
            Part::Item(Item::Span { text, .. }) => Some(format!("={text}")),
            _ => None,
        })
        .collect();
    Reply { intent, rest, routes, lines }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use parking_lot::Mutex;

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
            work(&rx, idle, None, load, |gen: &usize, text: &str, _: &[Pin], _: Option<&Scope>| {
                p.lock().push(format!("{gen}:{text}"));
                Reply { intent: String::new(), rest: Vec::new(), routes: Vec::new(), lines: Vec::new() }
            });
        });
        (tx, loads, parsed)
    }

    /// A sentence job whose reply nobody reads.
    fn sentence(text: &str) -> Job {
        let (reply, _) = bounded(1);
        Job::Parse(Request { text: text.into(), pins: Vec::new(), scope: None, reply })
    }

    fn wait_for(parsed: &Mutex<Vec<String>>, n: usize) -> Vec<String> {
        for _ in 0..200 {
            if parsed.lock().len() >= n {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        parsed.lock().clone()
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
        assert!(tx.send(Job::Warm).is_ok());
        assert!(tx.send(sentence("open the filter")).is_ok());
        assert_eq!(wait_for(&parsed, 1), ["1:open the filter"]);
        assert_eq!(loads.load(Ordering::SeqCst), 1, "warm and parse load once");
    }

    #[test]
    fn the_parser_drops_when_unused_and_loads_again() {
        let (tx, loads, parsed) = worker(Duration::from_millis(150));
        assert!(tx.send(sentence("a")).is_ok());
        wait_for(&parsed, 1);
        // warm jobs keep it loaded
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(80));
            assert!(tx.send(Job::Warm).is_ok());
        }
        assert!(tx.send(sentence("b")).is_ok());
        assert_eq!(wait_for(&parsed, 2), ["1:a", "1:b"], "still the first load");
        std::thread::sleep(Duration::from_millis(400));
        assert!(tx.send(sentence("c")).is_ok());
        assert_eq!(wait_for(&parsed, 3), ["1:a", "1:b", "2:c"], "dropped after idle, loaded again");
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }
}
