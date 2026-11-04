//! Bakes a build identifier into the binary: the git short sha when the probe
//! is built inside the repo, otherwise the build unix time. Logged on plugin
//! construction so the probe log proves which build a host loaded.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let stamp = sha.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    });
    println!("cargo:rustc-env=FL_RESIZE_PROBE_BUILD={stamp}");
}
