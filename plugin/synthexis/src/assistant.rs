//! Mapping assistant entry, minimal: one process-wide worker thread loads
//! the frame parser in the background and parses submitted sentences; the
//! frames go to the log. No text field, resolver or route changes yet
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
use std::time::Instant;

use crossbeam_channel::{unbounded, Receiver, Sender};
use synthexis_parser::{Options, Parser};

use crate::log::log;

const DEFAULT_RUNS: &str = "p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3";

static WORKER: OnceLock<Sender<String>> = OnceLock::new();

/// Starts the worker once per process; loading begins immediately on it.
/// Never call from the audio thread (the first call spawns a thread).
pub fn start() -> &'static Sender<String> {
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

/// Queues a typed sentence; the frame shows up in the log.
#[allow(dead_code)] // the editor's text field will call this
pub fn submit(text: &str) {
    if start().send(text.to_string()).is_err() {
        log("assistant: parser unavailable, sentence dropped");
    }
}

fn model_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_PARSER_DIR") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/parser")
}

fn run(rx: &Receiver<String>) {
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
        parse(&parser, &text);
    }
    for text in rx {
        parse(&parser, &text);
    }
}

fn parse(parser: &Parser, text: &str) {
    let t0 = Instant::now();
    match parser.parse(text) {
        Ok(p) => log(&format!(
            "assistant: {:.3} {:.0} ms \"{text}\" -> {}",
            f64::from(p.logp).exp(),
            t0.elapsed().as_secs_f64() * 1e3,
            p.frame().join(" ")
        )),
        Err(e) => log(&format!("assistant: parse failed for \"{text}\": {e:#}")),
    }
}
