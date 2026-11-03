//! Offline perception bench: decodes a recording (AVAssetReader, BGRA) and
//! runs every frame through a backend on this thread, back to back. Prints
//! stage times (median / p95 over all frames) and writes one JSON line per
//! frame with the feature values, for checking them against the
//! recording's segment labels (probes/plugin-perception/eval.py).
//!
//!   SYNTHEXIS_BENCH=<cam0.mov> [SYNTHEXIS_PERCEPTION=students|vision]
//!   [SYNTHEXIS_BENCH_OUT=<frames.jsonl>] [SYNTHEXIS_BENCH_START=<s>]
//!   [SYNTHEXIS_BENCH_SECONDS=<s>]
#![allow(unsafe_code)]

use std::io::Write;
use std::time::Instant;

use objc2::runtime::AnyObject;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeVideo, AVURLAsset};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use super::coreml::Frame;
use super::perception::{make_backend, model_dir, Stats};

fn json_floats(v: &[f32]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.3}")).collect();
    format!("[{}]", parts.join(","))
}

pub fn run(path: &str) -> i32 {
    let choice = std::env::var("SYNTHEXIS_PERCEPTION").unwrap_or_else(|_| "students".into());
    let max_s: f64 = std::env::var("SYNTHEXIS_BENCH_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(f64::MAX);
    let start_s: f64 = std::env::var("SYNTHEXIS_BENCH_START").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let mut out = std::env::var("SYNTHEXIS_BENCH_OUT").ok().and_then(|p| std::fs::File::create(p).ok());
    let Some(mut backend) = make_backend(&model_dir(), &choice) else {
        eprintln!("bench: no backend {choice}");
        return 2;
    };
    let reader = unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&NSURL::fileURLWithPath(&NSString::from_str(path)), None);
        let Some(media) = AVMediaTypeVideo else { return 2 };
        #[allow(deprecated)]
        let tracks = asset.tracksWithMediaType(media);
        let Some(track) = tracks.firstObject() else {
            eprintln!("bench: no video track in {path}");
            return 2;
        };
        let key: &NSString = {
            use objc2_core_video::kCVPixelBufferPixelFormatTypeKey;
            &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString)
        };
        let bgra = NSNumber::new_u32(0x4247_5241);
        let settings = NSDictionary::from_slices(&[key], &[&*bgra as &AnyObject]);
        let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&settings));
        output.setAlwaysCopiesSampleData(false);
        let reader = match AVAssetReader::assetReaderWithAsset_error(&asset) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("bench: reader: {}", e.localizedDescription());
                return 2;
            }
        };
        reader.addOutput(&output);
        if !reader.startReading() {
            eprintln!("bench: startReading failed");
            return 2;
        }
        (reader, output)
    };
    let (_reader, output) = reader;
    let mut stats = Stats::default();
    stats.window = usize::MAX;
    let (mut frames, mut seen_hands, mut seen_face) = (0u64, [0u64; 2], 0u64);
    let wall = Instant::now();
    while let Some(sb) = unsafe { output.copyNextSampleBuffer() } {
        let t = unsafe { sb.presentation_time_stamp().seconds() };
        if t > max_s {
            break;
        }
        if t < start_s {
            continue;
        }
        let Some(image) = (unsafe { sb.image_buffer() }) else { continue };
        let frame = Frame::new(image);
        let t_ns = (t.max(0.0) * 1e9) as u64;
        let start = Instant::now();
        let mut stages = Vec::new();
        let mut p = backend.process(&frame, t_ns, &mut stages);
        stats.push("early", start.elapsed());
        if let Some(face) = backend.face_later(&mut stages) {
            if let Some(p) = p.as_mut() {
                p.face = face;
            }
        }
        stats.push("frame", start.elapsed());
        for (n, ms) in stages {
            stats.push_ms(n, ms);
        }
        frames += 1;
        let Some(p) = p else { continue };
        let mut line = format!("{{\"t\":{t:.3}");
        for (s, name) in ["L", "R"].iter().enumerate() {
            if let Some((f, c)) = &p.hands[s] {
                seen_hands[s] += 1;
                line += &format!(",\"hand_{name}\":{},\"hand_{name}_conf\":{c:.2}", json_floats(&f.values()));
            }
        }
        if let Some((f, c)) = &p.face {
            seen_face += 1;
            line += &format!(
                ",\"face\":{},\"face_raw\":{},\"eye_px\":{},\"face_conf\":{c:.2},\"face_seen\":[{},{},{}]",
                json_floats(&f.values()),
                json_floats(&f.raw),
                json_floats(&[&f.eye_px[0][..], &f.eye_px[1][..], &f.mouth_px[..]].concat()),
                f.seen_eye[0] as u8,
                f.seen_eye[1] as u8,
                f.seen_mouth as u8
            );
        }
        if let Some(raw) = &p.debug {
            line += &format!(",\"raw\":{raw}");
        }
        if let Some((v, c)) = &p.object {
            line += &format!(",\"object\":{},\"object_conf\":{c:.2}", json_floats(v));
        }
        line += "}\n";
        if let Some(f) = out.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
    println!(
        "bench {choice} {path}: frames={frames} wall_s={:.1} hand_L={:.3} hand_R={:.3} face={:.3}",
        wall.elapsed().as_secs_f64(),
        seen_hands[0] as f64 / frames.max(1) as f64,
        seen_hands[1] as f64 / frames.max(1) as f64,
        seen_face as f64 / frames.max(1) as f64
    );
    println!("ms med/p95 {}", stats.summary());
    0
}
