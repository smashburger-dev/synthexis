//! Offline perception bench: decodes a recording (AVAssetReader, BGRA) and
//! runs every frame through a backend on this thread, back to back. Prints
//! stage times (median / p95 over all frames) and writes one JSON line per
//! frame with the feature values, for checking them against the
//! recording's segment labels (probes/plugin-perception/eval.py).
//!
//!   SYNTHEXIS_BENCH=<cam0.mov> [SYNTHEXIS_PERCEPTION=students|vision]
//!   [SYNTHEXIS_BENCH_OUT=<frames.jsonl>] [SYNTHEXIS_BENCH_START=<s>]
//!   [SYNTHEXIS_BENCH_SECONDS=<s>] [SYNTHEXIS_BENCH_SHOTS=<shots.jsonl>]
//!
//! With SYNTHEXIS_BENCH_SHOTS it also runs the Pong pointing shot
//! (pong::shot::Point) per hand and writes its events, one JSON line each
//! (probes/pong-shot/score.py): `gate` gets the debounced mask the MIDI
//! triggers see (only frames with a settled shape, as MASKS), `plain` the
//! mask without band and dwell; `mask` lines on every change, `shot` lines
//! with tip and direction, `noshot` lines for a turn into a point that did
//! not fire and why (lost, refractory, short index, unusable hand). Both
//! carry the frame's shot features (palms), hand speed and trigger values.
#![allow(unsafe_code)]

use std::io::Write;
use std::time::Instant;

use objc2::runtime::AnyObject;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeVideo, AVURLAsset};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use super::coreml::Frame;
use super::geom::{trigger_fingers, HandFeatures};
use super::perception::{make_backend, model_dir, Stats};
use crate::pong::shot::{self, Point};

/// The two Pong shot variants of one hand: debounced and plain mask.
#[derive(Default)]
struct Shots {
    point: [Point; 2],
    last: [Option<u8>; 2],
    fed_ns: [Option<u64>; 2],
    shot_ns: [Option<u64>; 2],
    fired: [u64; 2],
}

/// The frame's numbers behind a shot decision, as JSON fields.
fn shot_feat(f: &HandFeatures, vals: &[f32; 5]) -> String {
    let geo = shot::features(&f.joints).map_or("null".into(), |x| {
        format!("{{\"proj\":{:.3},\"straight\":{:.3},\"reach\":{:.3},\"others\":{:.3},\"gap\":{:.3}}}", x.proj, x.straight, x.reach, x.others, x.gap)
    });
    format!(",\"feat\":{geo},\"speed\":{:.3},\"vals\":{}", f.speed, json_floats(vals))
}

const SHOT_SRC: [&str; 2] = ["gate", "plain"];

impl Shots {
    /// One frame of the hand; its events as JSON lines.
    fn frame(&mut self, f: &HandFeatures, thr: [f32; 5], side: &str, t: f64, t_ns: u64) -> String {
        let vals = trigger_fingers(f);
        let plain = vals.iter().zip(&thr).enumerate().filter(|(_, (v, th))| v > th).fold(0u8, |m, (i, _)| m | (1 << (4 - i)));
        let masks = [f.shape_settled.then(|| crate::route::shape_mask(f.shape)), Some(plain)];
        let mut out = String::new();
        for (k, mask) in masks.into_iter().enumerate() {
            let Some(m) = mask else { continue };
            let head = format!("{{\"t\":{t:.3},\"side\":\"{side}\",\"src\":\"{}\"", SHOT_SRC[k]);
            let turned = shot::pointing(m) && self.last[k].is_some_and(|p| !shot::pointing(p));
            if self.last[k] != Some(m) {
                out += &format!("{head},\"ev\":\"mask\",\"mask\":\"{m:05b}\"}}\n");
            }
            self.last[k] = Some(m);
            let lost = self.fed_ns[k].is_some_and(|p| t_ns.saturating_sub(p) > shot::LOST_NS);
            let refractory = self.shot_ns[k].is_some_and(|p| t_ns.saturating_sub(p) < shot::REFRACTORY_NS);
            self.fed_ns[k] = Some(t_ns);
            match self.point[k].update(m, &f.joints, shot::Hand { vals: Some(vals), speed: Some(f.speed) }, t_ns) {
                Some(s) => {
                    self.fired[k] += 1;
                    self.shot_ns[k] = Some(t_ns);
                    let (at, dir) = (json_floats(&s.at), json_floats(&s.dir));
                    out += &format!("{head},\"ev\":\"shot\",\"at\":{at},\"dir\":{dir}{}}}\n", shot_feat(f, &vals));
                }
                None if turned => {
                    let why = match shot::features(&f.joints) {
                        _ if lost => "lost",
                        _ if refractory => "refractory",
                        Some(_) => "short",
                        None => "unusable",
                    };
                    out += &format!("{head},\"ev\":\"noshot\",\"why\":\"{why}\"{}}}\n", shot_feat(f, &vals));
                }
                None => {}
            }
        }
        out
    }
}

fn json_floats(v: &[f32]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.3}")).collect();
    format!("[{}]", parts.join(","))
}

pub fn run(path: &str) -> i32 {
    let choice = std::env::var("SYNTHEXIS_PERCEPTION").unwrap_or_else(|_| "students".into());
    let max_s: f64 = std::env::var("SYNTHEXIS_BENCH_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(f64::MAX);
    let start_s: f64 = std::env::var("SYNTHEXIS_BENCH_START").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let mut out = std::env::var("SYNTHEXIS_BENCH_OUT").ok().and_then(|p| std::fs::File::create(p).ok());
    let mut shots_out = std::env::var("SYNTHEXIS_BENCH_SHOTS").ok().and_then(|p| std::fs::File::create(p).ok());
    let mut shots: [Shots; 2] = Default::default();
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
        // a pool per frame, as the perception thread has (perception::run)
        let (mut p, later, early) = objc2::rc::autoreleasepool(|_| {
            let p = backend.process(&frame, t_ns, &mut stages);
            let early = start.elapsed();
            (p, backend.face_later(&mut stages), early)
        });
        stats.push("early", early);
        if let Some(face) = later {
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
                if let Some(w) = shots_out.as_mut() {
                    let thr = super::FINGER_THR.lock()[s];
                    let _ = w.write_all(shots[s].frame(f, thr, name, t, t_ns).as_bytes());
                }
                line += &format!(",\"hand_{name}\":{},\"hand_{name}_conf\":{c:.2},\"hand_{name}_ext\":{}", json_floats(&f.values()), json_floats(&f.ext));
            }
        }
        if let Some((f, c)) = &p.face {
            seen_face += 1;
            line += &format!(
                ",\"face\":{},\"face_raw\":{},\"eye_px\":{},\"head_pts\":{},\"head_fit\":{:.4},\"face_conf\":{c:.2},\"face_seen\":[{},{},{},{}]",
                json_floats(&f.values()),
                json_floats(&f.raw),
                json_floats(&[&f.eye_px[0][..], &f.eye_px[1][..], &f.mouth_px[..]].concat()),
                json_floats(f.head_pts.as_flattened()),
                f.head_fit,
                f.seen_eye[0] as u8,
                f.seen_eye[1] as u8,
                f.seen_mouth as u8,
                f.seen_head as u8
            );
        }
        if let Some((x, _)) = p.centre_x {
            line += &format!(",\"centre_x\":{x:.3}");
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
    if shots_out.is_some() {
        let n = |k: usize| shots[0].fired[k] + shots[1].fired[k];
        println!("shots gate={} plain={}", n(0), n(1));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pong::shot::synth::{hand, place, THUMB_OUT};

    fn frame(mask: u8, settled: bool) -> HandFeatures {
        let bits: [f32; 5] = std::array::from_fn(|i| f32::from(mask >> (4 - i) & 1));
        HandFeatures {
            ext: bits,
            finger: bits,
            shape: crate::route::shape_value(mask),
            shape_settled: settled,
            joints: place(hand(0.0, THUMB_OUT, 0.0), 0.0, 0.2, [0.5, 0.5]),
            ..Default::default()
        }
    }

    fn events(lines: &str, ev: &str, src: &str) -> usize {
        lines.lines().filter(|l| l.contains(&format!("\"ev\":\"{ev}\"")) && l.contains(&format!("\"src\":\"{src}\""))).count()
    }

    #[test]
    fn a_fist_turning_into_a_point_writes_one_shot_per_mask() {
        let mut s = Shots::default();
        let lines: String = [0b00000, 0b00000].into_iter().chain([0b01000; 14])
            .enumerate()
            .map(|(i, m)| s.frame(&frame(m, true), [0.5; 5], "R", i as f64 / 30.0, i as u64 * 33_333_333))
            .collect();
        assert_eq!((events(&lines, "shot", "gate"), events(&lines, "shot", "plain")), (1, 1));
        assert_eq!(events(&lines, "mask", "gate"), 2);
        assert!(lines.contains("\"mask\":\"01000\""));
    }

    #[test]
    fn an_unsettled_shape_feeds_only_the_plain_mask() {
        let mut s = Shots::default();
        let lines: String = [(0b00000, true)].into_iter().chain([(0b01000, false); 14])
            .enumerate()
            .map(|(i, (m, settled))| s.frame(&frame(m, settled), [0.5; 5], "L", i as f64 / 30.0, i as u64 * 33_333_333))
            .collect();
        assert_eq!((events(&lines, "shot", "gate"), events(&lines, "shot", "plain")), (0, 1));
    }
}
