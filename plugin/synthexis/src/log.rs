//! Line logger to ~/Library/Logs/synthexis.log plus a small ring buffer
//! for the editor.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

fn path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    format!("{home}/Library/Logs/synthexis.log")
}

pub fn log(msg: &str) {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let line = format!("[{ts:.3}] {msg}");
    {
        let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
        lines.push_back(line.clone());
        while lines.len() > 50 {
            lines.pop_front();
        }
    }
    let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if file.is_none() {
        *file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path())
            .ok();
    }
    if let Some(f) = file.as_mut() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
    NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
}

pub fn last_lines(n: usize) -> Vec<String> {
    let lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    lines.iter().rev().take(n).rev().cloned().collect()
}
