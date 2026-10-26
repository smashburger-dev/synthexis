//! Append-only log to ~/Library/Logs/synthexis-probe.log plus an in-memory
//! ring buffer of the last lines for the editor.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const MAX_LINES: usize = 200;

struct Log {
    lines: Vec<String>,
    file: Option<std::fs::File>,
}

fn log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join("Library/Logs/synthexis-probe.log")
}

fn global() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| {
        let path = log_path();
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Mutex::new(Log {
            lines: Vec::new(),
            file,
        })
    })
}

fn now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    format!("{secs:.3}")
}

pub fn log(msg: &str) {
    let line = format!("[{}] {}", now_string(), msg);
    if let Ok(mut g) = global().lock() {
        if let Some(f) = g.file.as_mut() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
        g.lines.push(line.clone());
        if g.lines.len() > MAX_LINES {
            let drop = g.lines.len() - MAX_LINES;
            g.lines.drain(..drop);
        }
    }
    eprintln!("synthexis-probe: {line}");
}

pub fn last_lines(n: usize) -> Vec<String> {
    global()
        .lock()
        .map(|g| g.lines.iter().rev().take(n).rev().cloned().collect())
        .unwrap_or_default()
}
