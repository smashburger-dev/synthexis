//! Mapping assistant entry: one process-wide worker thread loads the frame
//! parser in the background and parses submitted sentences; the chips of
//! the sentence field are enforced on the frame as pinned spans
//! (synthexis_parser::pin), the resolver binds the targets to the picked
//! plugin's parameters (synthexis_parser::resolve), and one line per route
//! ("left hand openness -> Topos / Drive") goes back to the field and to
//! the log. No route changes yet (docs/parser-runtime-plan.md).
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
use std::time::Instant;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use synthexis_complete::Param;
use synthexis_parser::resolve::{self, resolve, Candidate};
use synthexis_parser::{pin, Options, Parser, Pin};

use crate::log::log;

const DEFAULT_RUNS: &str = "p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3";

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
    pub routes: Vec<resolve::Route>,
    /// One line per route, or why there is none.
    pub lines: Vec<String>,
}

struct Job {
    text: String,
    pins: Vec<Pin>,
    scope: Option<Scope>,
    reply: Sender<Reply>,
}

static WORKER: OnceLock<Sender<Job>> = OnceLock::new();

/// Starts the worker once per process; loading begins immediately on it.
/// Never call from the audio thread (the first call spawns a thread).
pub fn start() {
    worker();
}

fn worker() -> &'static Sender<Job> {
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

/// Queues a typed sentence with its chips. The receiver gets the reply
/// once parsed; it disconnects when the parser is unavailable.
pub fn submit(text: &str, pins: Vec<Pin>, scope: Option<Scope>) -> Receiver<Reply> {
    let (reply, rx) = bounded(1);
    if worker().send(Job { text: text.to_string(), pins, scope, reply }).is_err() {
        log("assistant: parser unavailable, sentence dropped");
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

fn run(rx: &Receiver<Job>) {
    let dir = model_dir();
    let runs = std::env::var("SYNTHEXIS_PARSER_RUNS").unwrap_or_else(|_| DEFAULT_RUNS.into());
    let names: Vec<&str> = runs.split(',').map(str::trim).collect();
    let t0 = Instant::now();
    let parser = match Parser::load(&dir, &names, &Options::default()) {
        Ok(p) => p,
        Err(e) => {
            log(&format!("assistant: parser {runs} not loaded from {}: {e:#}", dir.display()));
            return; // drops rx, so submit() reports the parser as unavailable
        }
    };
    log(&format!(
        "assistant: parser {runs} loaded in {:.0} ms",
        t0.elapsed().as_secs_f64() * 1e3
    ));
    if let Ok(text) = std::env::var("SYNTHEXIS_PARSE") {
        parse(&parser, &text, &[], None);
    }
    for job in rx {
        let _ = job.reply.send(parse(&parser, &job.text, &job.pins, job.scope.as_ref()));
    }
}

/// Parses, pins, resolves; logs the frame and returns the reply.
fn parse(parser: &Parser, text: &str, pins: &[Pin], scope: Option<&Scope>) -> Reply {
    let t0 = Instant::now();
    let p = match parser.parse(text) {
        Ok(p) => p,
        Err(e) => {
            log(&format!("assistant: parse failed for \"{text}\": {e:#}"));
            return Reply { intent: "none".into(), routes: Vec::new(), lines: vec![format!("parse failed: {e}")] };
        }
    };
    let parsed_ms = t0.elapsed().as_secs_f64() * 1e3;
    let frame = pin(&p.items, pins);
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
    Reply { intent, routes, lines }
}
