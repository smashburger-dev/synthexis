//! Parse sentences with the runtime parser and time it.
//!
//!   synthexis-parse --dir data/mapping/runtime --runs p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3 \
//!       --file texts.txt [--jsonl out.jsonl] [--f16 | --q8 | --f32] [--rules conv,pronoun]
//!       [--threads 1] ["sentence" ...]
//!
//! Prints load time, resident memory after load, and per-sentence latency
//! (median, p95). --jsonl writes {text, conf, logp, frame, ms, spans} per
//! line, the format of tools/mapping/predict.py --jsonl plus logp, ms and
//! the BIO head's spans as [start byte, end byte, tag].

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{anyhow, Result};
use synthexis_parser::{Options, Parser};

fn rss_mb() -> Option<f64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kb: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb / 1024.0)
}

fn main() -> Result<()> {
    let mut dir = PathBuf::from("data/mapping/runtime");
    let mut runs = String::from("p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3");
    let mut texts: Vec<String> = Vec::new();
    let mut jsonl: Option<PathBuf> = None;
    let mut opt = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| anyhow!("{a} needs a value"));
        match a.as_str() {
            "--dir" => dir = PathBuf::from(val()?),
            "--runs" => runs = val()?,
            "--file" => {
                let body = std::fs::read_to_string(val()?)?;
                texts.extend(body.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from));
            }
            "--jsonl" => jsonl = Some(PathBuf::from(val()?)),
            "--threads" => opt.threads = val()?.parse()?,
            "--f32" => opt.f32_weights = true,
            "--q8" => opt.q8 = Some(true),
            "--f16" => opt.q8 = Some(false),
            // "" switches the runs' own rules off
            "--rules" => opt.rules = Some(val()?.split(',').filter(|r| !r.is_empty()).map(String::from).collect()),
            _ => texts.push(a),
        }
    }
    let names: Vec<&str> = runs.split(',').collect();
    let rss0 = rss_mb().unwrap_or(0.0);
    let t0 = Instant::now();
    let parser = Parser::load(&dir, &names, &opt)?;
    let load_ms = t0.elapsed().as_secs_f64() * 1e3;
    let rss1 = rss_mb().unwrap_or(0.0);
    eprintln!(
        "loaded {runs} ({}, {} thread) in {load_ms:.0} ms, rss {rss1:.0} MB (+{:.0})",
        match opt.q8 {
            Some(true) => "q8_0",
            Some(false) => "f16",
            None => "weights as parser.json says",
        },
        opt.threads,
        rss1 - rss0
    );
    let mut out = match &jsonl {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    let mut ms = Vec::with_capacity(texts.len());
    for t in &texts {
        let t1 = Instant::now();
        let p = parser.parse(t)?;
        let dt = t1.elapsed().as_secs_f64() * 1e3;
        ms.push(dt);
        let frame = p.frame();
        let conf = (f64::from(p.logp).exp() * 1000.0).round() / 1000.0;
        match out.as_mut() {
            Some(f) => {
                let spans: Vec<_> = p.spans.iter().map(|s| serde_json::json!([s.bytes.0, s.bytes.1, s.tag])).collect();
                let row = serde_json::json!({"text": t, "conf": conf, "logp": p.logp, "frame": frame, "ms": dt, "enc_ms": p.enc_ms, "dec_ms": p.dec_ms, "spans": spans});
                writeln!(f, "{row}")?;
            }
            None => {
                let spans: Vec<String> = p.spans.iter().map(|s| format!("{}={}", s.tag, &t[s.bytes.0..s.bytes.1])).collect();
                println!("{conf:.3}  {t}\n       {}\n       {}", frame.join(" "), spans.join(" | "));
            }
        }
    }
    if !ms.is_empty() {
        ms.sort_by(f64::total_cmp);
        let med = ms[ms.len() / 2];
        let p95 = ms[(ms.len() * 95 / 100).min(ms.len() - 1)];
        eprintln!(
            "{} sentences: median {med:.1} ms, p95 {p95:.1} ms, max {:.1} ms, rss {:.0} MB",
            ms.len(),
            ms[ms.len() - 1],
            rss_mb().unwrap_or(0.0)
        );
    }
    Ok(())
}
