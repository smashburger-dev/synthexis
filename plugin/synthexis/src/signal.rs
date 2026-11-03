//! Signal sample frame exchanged between the engine and route writers.
//!
//! Perception ids are `cam0.` + the ontology source id
//! (`tools/mapping/ontology.yaml`, the `S:` names the parser emits), with
//! the side (`L`/`R`, the person's own side) after the first segment for
//! sided sources: `S:hand.openness side:left` -> `cam0.hand.L.openness`.

/// One delivered value from a signal source.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Normalized value 0..1.
    pub value: f32,
    /// Confidence 0..1 (occlusion handling uses c_min).
    pub confidence: f32,
    /// Capture time on the host clock, nanoseconds (CMClock host time).
    pub t_capture_ns: u64,
    /// False while the source is occluded/not producing.
    pub present: bool,
}

pub const SIG_BRIGHTNESS: &str = "cam0.frame.brightness";
pub const SIG_MOTION: &str = "cam0.frame.motion";
pub const SIG_TRIANGLE: &str = "test.triangle";

macro_rules! sided {
    ($feats:ident, $left:ident, $right:ident, $entity:literal, [$($f:literal),* $(,)?]) => {
        /// Feature names, in the order the feature arrays use.
        pub const $feats: &[&str] = &[$($f),*];
        pub const $left: &[&str] = &[$(concat!("cam0.", $entity, ".L.", $f)),*];
        pub const $right: &[&str] = &[$(concat!("cam0.", $entity, ".R.", $f)),*];
    };
}

// Order matters: `engine::geom::HandFeatures::values` fills this order.
sided!(HAND_FEATS, HAND_L, HAND_R, "hand", [
    "openness", "fist",
    "finger.thumb", "finger.index", "finger.middle", "finger.ring", "finger.pinky",
    "spread", "x", "y", "depth", "pinch", "speed", "roll",
    "finger.thumb.tilt", "finger.index.tilt", "finger.middle.tilt",
    "finger.ring.tilt", "finger.pinky.tilt",
    "finger_count",
]);

pub const SIG_HANDS_DISTANCE: &str = "cam0.hands.distance";

/// The person's centre on x (face centre, person's view, 0 = their left).
/// Internal: no ontology source and not in `perception_ids`, so no
/// sentence can route it; route.rs splits two hands' x at it.
pub const SIG_BODY_X: &str = "cam0.body.x";

// Order matters: `engine::geom::FaceFeatures::values` fills this order.
pub const FACE_IDS: &[&str] = &[
    "cam0.mouth.open",
    "cam0.face.smile",
    "cam0.eye.L.open",
    "cam0.eye.R.open",
    "cam0.eye.L.closed",
    "cam0.eye.R.closed",
    "cam0.head.yaw",
    "cam0.head.pitch",
    "cam0.head.roll",
];

// Largest tracked object; the name binding (`object:<name>`) is the
// resolver's job, the engine has one object slot today.
pub const OBJECT_IDS: &[&str] = &[
    "cam0.object.x",
    "cam0.object.y",
    "cam0.object.size",
    "cam0.object.angle",
    "cam0.object.speed",
];

/// Every perception id the engine can emit (capabilities list).
pub fn perception_ids() -> impl Iterator<Item = &'static str> {
    HAND_L
        .iter()
        .chain(HAND_R)
        .chain(std::iter::once(&SIG_HANDS_DISTANCE))
        .chain(FACE_IDS)
        .chain(OBJECT_IDS)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source ids and their `side` flag from the ontology's sources block.
    fn ontology_sources() -> Vec<(String, bool)> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/mapping/ontology.yaml");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut out = Vec::new();
        let mut in_sources = false;
        for line in text.lines() {
            if !line.starts_with(' ') && !line.starts_with('#') && !line.is_empty() {
                in_sources = line.starts_with("sources:");
            }
            let Some(rest) = line.trim_start().strip_prefix("- {id: ") else { continue };
            if !in_sources {
                continue;
            }
            let id: String = rest.chars().take_while(|c| *c != ',' && *c != '}').collect();
            out.push((id.trim().to_string(), line.contains("side: true")));
        }
        out
    }

    #[test]
    fn perception_ids_match_ontology_sources() {
        let sources = ontology_sources();
        assert!(sources.len() > 30, "ontology not read ({} sources)", sources.len());
        for id in perception_ids() {
            let rest = id.strip_prefix("cam0.").unwrap_or("");
            let mut parts: Vec<&str> = rest.split('.').collect();
            let sided = parts.len() > 2 && (parts[1] == "L" || parts[1] == "R");
            if sided {
                parts.remove(1);
            }
            let source = parts.join(".");
            let hit = sources.iter().find(|(s, _)| *s == source);
            assert!(hit.is_some(), "{id}: {source} is not an ontology source");
            assert_eq!(hit.map(|h| h.1), Some(sided), "{id}: side flag differs");
        }
    }
}
