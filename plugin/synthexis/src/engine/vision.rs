//! Apple Vision at runtime: the fallback backend while the students miss
//! the quality gate (probes/licence-check: runtime use and eval are fine,
//! only training on Vision output is not). Hand pose (2 hands x 21 joints)
//! and face landmarks in one handler call, fed into the same geometry as
//! the students. Probe C: hand 18.4 / 27.8 ms, face landmarks 14.5 / 24.6
//! ms (median / p95, 1280x720).
#![allow(unsafe_code)]

use std::time::Instant;

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::*;

use super::coreml::Frame;
use super::geom::*;
use super::perception::{Backend, Percept};
use crate::log::log;

const MIN_JOINT_CONF: f32 = 0.3;
/// Lip corner lift for smile 0 and 1 (lip_corner_lift; calibrated on S4).
const SMILE_LIFT: (f32, f32) = (-0.01, 0.03);

pub struct VisionBackend {
    hands: Retained<VNDetectHumanHandPoseRequest>,
    faces: Retained<VNDetectFaceLandmarksRequest>,
    tracker: HandTracker,
    errors: u64,
}

impl VisionBackend {
    pub fn new() -> Self {
        unsafe {
            let hands = VNDetectHumanHandPoseRequest::new();
            hands.setMaximumHandCount(2);
            Self { hands, faces: VNDetectFaceLandmarksRequest::new(), tracker: HandTracker::default(), errors: 0 }
        }
    }
}

fn joint_names() -> [&'static VNHumanHandPoseObservationJointName; 21] {
    unsafe {
        [
            VNHumanHandPoseObservationJointNameWrist,
            VNHumanHandPoseObservationJointNameThumbCMC,
            VNHumanHandPoseObservationJointNameThumbMP,
            VNHumanHandPoseObservationJointNameThumbIP,
            VNHumanHandPoseObservationJointNameThumbTip,
            VNHumanHandPoseObservationJointNameIndexMCP,
            VNHumanHandPoseObservationJointNameIndexPIP,
            VNHumanHandPoseObservationJointNameIndexDIP,
            VNHumanHandPoseObservationJointNameIndexTip,
            VNHumanHandPoseObservationJointNameMiddleMCP,
            VNHumanHandPoseObservationJointNameMiddlePIP,
            VNHumanHandPoseObservationJointNameMiddleDIP,
            VNHumanHandPoseObservationJointNameMiddleTip,
            VNHumanHandPoseObservationJointNameRingMCP,
            VNHumanHandPoseObservationJointNameRingPIP,
            VNHumanHandPoseObservationJointNameRingDIP,
            VNHumanHandPoseObservationJointNameRingTip,
            VNHumanHandPoseObservationJointNameLittleMCP,
            VNHumanHandPoseObservationJointNameLittlePIP,
            VNHumanHandPoseObservationJointNameLittleDIP,
            VNHumanHandPoseObservationJointNameLittleTip,
        ]
    }
}

/// 21 joints in frame pixels (y down) and their mean confidence.
fn hand_points(obs: &VNHumanHandPoseObservation, w: f32, h: f32) -> Option<([[f32; 2]; 21], f32)> {
    let mut kp = [[0f32; 2]; 21];
    let mut conf_sum = 0.0;
    let mut good = 0;
    for (j, name) in joint_names().iter().enumerate() {
        let p = unsafe { obs.recognizedPointForJointName_error(name) }.ok()?;
        let c = unsafe { p.confidence() };
        let loc = unsafe { p.location() };
        kp[j] = [loc.x as f32 * w, (1.0 - loc.y as f32) * h];
        conf_sum += c;
        if c >= MIN_JOINT_CONF {
            good += 1;
        }
    }
    (good >= 15).then_some((kp, conf_sum / 21.0))
}

fn region(r: Option<Retained<VNFaceLandmarkRegion2D>>, w: f32, h: f32) -> Option<Vec<[f32; 2]>> {
    let r = r?;
    unsafe {
        let n = r.pointCount();
        let pts = r.pointsInImageOfSize(CGSize { width: w as f64, height: h as f64 });
        let pts = std::slice::from_raw_parts(pts.as_ptr(), n);
        Some(pts.iter().map(|p| [p.x as f32, h - p.y as f32]).collect())
    }
}

fn face(obs: &VNFaceObservation, w: f32, h: f32) -> Option<(FaceFeatures, f32)> {
    unsafe {
        let bb = obs.boundingBox();
        let fbox = [
            bb.origin.x as f32 * w,
            (1.0 - (bb.origin.y + bb.size.height) as f32) * h,
            bb.size.width as f32 * w,
            bb.size.height as f32 * h,
        ];
        let lm = obs.landmarks()?;
        let eyes: Vec<Shape> = [region(lm.leftEye(), w, h), region(lm.rightEye(), w, h)]
            .into_iter()
            .flatten()
            .filter_map(|p| Shape::from_outline(&p))
            .collect();
        let mouth = region(lm.innerLips(), w, h).and_then(|p| Shape::from_outline(&p));
        let mut f = face_features(fbox, &eyes, mouth, CAL_LANDMARKS);
        if let Some(lift) = region(lm.outerLips(), w, h).and_then(|p| lip_corner_lift(&p)) {
            f.raw[3] = lift;
            f.smile = (lift - SMILE_LIFT.0) / (SMILE_LIFT.1 - SMILE_LIFT.0);
            f.smile = f.smile.clamp(0.0, 1.0);
        }
        // Vision's own angles (radians) where it has them.
        let quarter = std::f32::consts::FRAC_PI_4;
        if let Some(y) = obs.yaw() {
            f.yaw = 0.5 + 0.5 * (y.floatValue() / quarter).clamp(-1.0, 1.0);
        }
        if let Some(r) = obs.roll() {
            f.roll = 0.5 + 0.5 * (r.floatValue() / quarter).clamp(-1.0, 1.0);
        }
        if let Some(p) = obs.pitch() {
            f.pitch = 0.5 - 0.5 * (p.floatValue() / quarter).clamp(-1.0, 1.0);
        }
        Some((f, lm.confidence()))
    }
}

/// Face landmarks only, on the seg student's face box: skips Vision's own
/// face detection. The `hybrid` backend uses it for the face because the
/// face student's eye and mouth masks do not separate open from closed.
pub struct FaceLandmarks {
    req: Retained<VNDetectFaceLandmarksRequest>,
    errors: u64,
}

impl FaceLandmarks {
    pub fn new() -> Self {
        Self { req: unsafe { VNDetectFaceLandmarksRequest::new() }, errors: 0 }
    }

    /// `bb`: face box in normalised image coordinates, origin top-left
    /// (x, y, w, h).
    pub fn run(&mut self, frame: &Frame, bb: [f32; 4]) -> Option<(FaceFeatures, f32)> {
        let (w, h) = frame.read(|_, _, w, h| (w as f32, h as f32))?;
        unsafe {
            let rect = CGRect {
                origin: CGPoint { x: bb[0] as f64, y: (1.0 - bb[1] - bb[3]) as f64 },
                size: CGSize { width: bb[2] as f64, height: bb[3] as f64 },
            };
            let seed = VNFaceObservation::faceObservationWithRequestRevision_boundingBox_roll_yaw(
                VNDetectFaceRectanglesRequestRevision3,
                rect,
                None,
                None,
            );
            self.req.setInputFaceObservations(Some(&NSArray::from_retained_slice(&[seed])));
            let handler = VNImageRequestHandler::initWithCVPixelBuffer_options(
                VNImageRequestHandler::alloc(),
                frame.pixel_buffer(),
                &NSDictionary::new(),
            );
            let reqs: [&VNRequest; 1] = [&self.req];
            if let Err(e) = handler.performRequests_error(&NSArray::from_slice(&reqs)) {
                self.errors += 1;
                if self.errors.is_power_of_two() {
                    log(&format!("perception: face landmarks failed ({}x): {}", self.errors, e.localizedDescription()));
                }
                return None;
            }
            let obs = self.req.results()?.firstObject()?;
            face(&obs, w, h)
        }
    }
}

impl Backend for VisionBackend {
    fn name(&self) -> &'static str {
        "vision"
    }

    fn process(&mut self, frame: &Frame, t_ns: u64, stages: &mut Vec<(&'static str, f64)>) -> Option<Percept> {
        let (w, h) = frame.read(|_, _, w, h| (w as f32, h as f32))?;
        let t0 = Instant::now();
        let handler = unsafe {
            VNImageRequestHandler::initWithCVPixelBuffer_options(
                VNImageRequestHandler::alloc(),
                frame.pixel_buffer(),
                &NSDictionary::new(),
            )
        };
        let reqs: [&VNRequest; 2] = [&self.hands, &self.faces];
        if let Err(e) = handler.performRequests_error(&NSArray::from_slice(&reqs)) {
            self.errors += 1;
            if self.errors.is_power_of_two() {
                log(&format!("perception: vision failed ({}x): {}", self.errors, e.localizedDescription()));
            }
            return None;
        }
        stages.push(("vision", t0.elapsed().as_secs_f64() * 1e3));
        let t1 = Instant::now();
        let mut p = Percept::default();
        let faces = unsafe { self.faces.results() };
        if let Some(obs) = faces.as_ref().and_then(|a| a.firstObject()) {
            p.face = face(&obs, w, h);
        }
        let face_cx = faces
            .as_ref()
            .and_then(|a| a.firstObject())
            .map(|o| unsafe { (o.boundingBox().origin.x + o.boundingBox().size.width / 2.0) as f32 * w });
        let hands: Vec<([[f32; 2]; 21], f32)> = unsafe { self.hands.results() }
            .map(|a| a.iter().filter_map(|o| hand_points(&o, w, h)).take(2).collect())
            .unwrap_or_default();
        let palms: Vec<[f32; 2]> = hands
            .iter()
            .map(|(kp, _)| [(kp[0][0] + kp[9][0]) / 2.0, (kp[0][1] + kp[9][1]) / 2.0])
            .collect();
        let sides = self.tracker.assign(&palms, face_cx, w, t_ns);
        for ((kp, conf), side) in hands.iter().zip(sides) {
            let mut f = hand_features(kp, side, w, h, None);
            f.speed = self.tracker.update(side, f.palm, h, t_ns);
            p.hands[side as usize] = Some((f, *conf));
        }
        stages.push(("geometry", t1.elapsed().as_secs_f64() * 1e3));
        Some(p)
    }
}
