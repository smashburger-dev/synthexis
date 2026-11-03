//! Resolver eval (probes/resolver/RESULT.md): parse each row, resolve its
//! targets against the parameters of the row's plugin only (the picked
//! plugin), write what got bound.
//!
//!   cargo run -p synthexis-parser --release --example resolve_eval -- \
//!       --dir data/mapping/runtime --plugins plugins.json --rows rows.jsonl --out out.jsonl
//!
//! plugins.json: {plugin_id: [[index, name, class], ...]}; rows.jsonl:
//! {id, text, plugin_id}. Out: {id, frame, spans, routes, describe}.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use synthexis_parser::resolve::{resolve, Candidate};
use synthexis_parser::{pin, Options, Parser};

fn main() -> Result<()> {
    let (mut dir, mut plugins, mut rows, mut out) = (PathBuf::new(), PathBuf::new(), PathBuf::new(), PathBuf::new());
    let mut runs = String::from("p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let v = args.next().ok_or_else(|| anyhow!("{a} needs a value"))?;
        match a.as_str() {
            "--dir" => dir = v.into(),
            "--plugins" => plugins = v.into(),
            "--rows" => rows = v.into(),
            "--out" => out = v.into(),
            "--runs" => runs = v,
            _ => return Err(anyhow!("unknown flag {a}")),
        }
    }
    let table: HashMap<String, Vec<(u32, String, String)>> = serde_json::from_str(&std::fs::read_to_string(&plugins)?)?;
    let names: Vec<&str> = runs.split(',').collect();
    let parser = Parser::load(&dir, &names, &Options::default())?;
    let mut w = std::io::BufWriter::new(std::fs::File::create(&out)?);
    let none = HashMap::new();
    for line in std::io::BufReader::new(std::fs::File::open(&rows)?).lines() {
        let row: Value = serde_json::from_str(&line?)?;
        let text = row["text"].as_str().unwrap_or_default();
        let pid = row["plugin_id"].as_str().unwrap_or_default();
        let params = table.get(pid).map(Vec::as_slice).unwrap_or_default();
        let cands: Vec<Candidate> =
            params.iter().map(|(index, name, class)| Candidate { index: *index, name, class }).collect();
        let p = parser.parse(text)?;
        let frame = pin(&p.items, &[]);
        let routes = resolve(&frame, text, &p.spans, pid, &cands, &none);
        let label = pid.rsplit('/').next().unwrap_or(pid);
        let rj: Vec<Value> = routes
            .iter()
            .map(|r| {
                let ts: Vec<Value> = r
                    .targets
                    .iter()
                    .map(|t| {
                        let b = t.bound.as_ref();
                        json!({"token": t.token, "text": t.text, "name": b.map(|b| b.name.clone()),
                               "index": b.map(|b| b.index), "via": b.map(|b| format!("{:?}", b.via))})
                    })
                    .collect();
                json!({"source": r.source, "side": r.side, "inv": r.inv, "targets": ts})
            })
            .collect();
        let spans: Vec<Value> = p.spans.iter().map(|s| json!([s.bytes.0, s.bytes.1, s.tag])).collect();
        let describe: Vec<String> = routes.iter().map(|r| r.describe(label)).collect();
        writeln!(w, "{}", json!({"id": row["id"], "frame": p.frame(), "spans": spans, "routes": rj, "describe": describe}))?;
    }
    Ok(())
}
