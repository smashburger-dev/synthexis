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

use super::coreml::{Frame, PixelBuf};
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

/// Image the observation was made on (w, h) and where it sits in the
/// camera frame: frame = (x0, y0) + image * k.
#[derive(Clone, Copy)]
struct Place {
    w: f32,
    h: f32,
    x0: f32,
    y0: f32,
    k: f32,
}

impl Place {
    fn frame(w: f32, h: f32) -> Self {
        Self { w, h, x0: 0.0, y0: 0.0, k: 1.0 }
    }

    fn map(&self, x: f32, y: f32) -> [f32; 2] {
        [self.x0 + x * self.k, self.y0 + y * self.k]
    }
}

fn region(r: Option<Retained<VNFaceLandmarkRegion2D>>, at: Place) -> Option<Vec<[f32; 2]>> {
    let r = r?;
    unsafe {
        let n = r.pointCount();
        let pts = r.pointsInImageOfSize(CGSize { width: at.w as f64, height: at.h as f64 });
        let pts = std::slice::from_raw_parts(pts.as_ptr(), n);
        Some(pts.iter().map(|p| at.map(p.x as f32, at.h - p.y as f32)).collect())
    }
}

fn face(obs: &VNFaceObservation, at: Place) -> Option<(FaceFeatures, f32)> {
    unsafe {
        let bb = obs.boundingBox();
        let tl = at.map(bb.origin.x as f32 * at.w, (1.0 - (bb.origin.y + bb.size.height) as f32) * at.h);
        let fbox = [tl[0], tl[1], bb.size.width as f32 * at.w * at.k, bb.size.height as f32 * at.h * at.k];
        let lm = obs.landmarks()?;
        let eyes: Vec<Shape> = [region(lm.leftEye(), at), region(lm.rightEye(), at)]
            .into_iter()
            .flatten()
            .filter_map(|p| Shape::from_outline(&p))
            .collect();
        let mouth = region(lm.innerLips(), at).and_then(|p| Shape::from_outline(&p));
        let mut f = face_features(fbox, &eyes, mouth, CAL_LANDMARKS);
        if let Some(lift) = region(lm.outerLips(), at).and_then(|p| lip_corner_lift(&p)) {
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
/// Where a face crop for the landmarks came from: the crop square in frame
/// pixels and the seed box inside it (normalised, origin top-left).
#[derive(Clone, Copy)]
pub struct Seed {
    x0: f32,
    y0: f32,
    side: f32,
    rect: [f32; 4],
}

/// Side of the square face crop handed to Vision, and its padding around
/// the seg face box.
pub const LM_CROP: usize = 256;
const LM_PAD: f32 = 1.4;

/// Crops the face around `fbox` (frame px, x y w h) into `buf`
/// (`LM_CROP` square).
pub fn face_crop(src: &Bgra, fbox: [f32; 4], buf: &PixelBuf) -> Seed {
    let (x0, y0, side) = square(fbox[0], fbox[1], fbox[2], fbox[3], LM_PAD);
    buf.write(|d, ds| super::vimage::crop_square(src, x0, y0, side, d, ds, LM_CROP));
    Seed { x0, y0, side, rect: [(fbox[0] - x0) / side, (fbox[1] - y0) / side, fbox[2] / side, fbox[3] / side] }
}

/// Face landmarks only, on a face crop seeded with the seg face box, so
/// Vision skips its own face detection. The `hybrid` backend uses it for
/// the face because the face student's eye and mouth masks do not
/// separate open from closed.
pub struct FaceLandmarks {
    req: Retained<VNDetectFaceLandmarksRequest>,
    adapt: FaceAdapt,
    errors: u64,
}

impl FaceLandmarks {
    pub fn new() -> Self {
        Self { req: unsafe { VNDetectFaceLandmarksRequest::new() }, adapt: FaceAdapt::default(), errors: 0 }
    }

    pub fn run(&mut self, buf: &PixelBuf, seed: &Seed, t_ns: u64, hand_on_face: bool) -> Option<(FaceFeatures, f32)> {
        let n = LM_CROP as f32;
        let [fx, fy, fw, fh] = seed.rect;
        unsafe {
            let rect = CGRect {
                origin: CGPoint { x: fx as f64, y: (1.0 - fy - fh) as f64 },
                size: CGSize { width: fw as f64, height: fh as f64 },
            };
            let obs = VNFaceObservation::faceObservationWithRequestRevision_boundingBox_roll_yaw(
                VNDetectFaceRectanglesRequestRevision3,
                rect,
                None,
                None,
            );
            self.req.setInputFaceObservations(Some(&NSArray::from_retained_slice(&[obs])));
            let handler = VNImageRequestHandler::initWithCVPixelBuffer_options(
                VNImageRequestHandler::alloc(),
                buf.pixel_buffer(),
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
            let mut f = face(&obs, Place { w: n, h: n, x0: seed.x0, y0: seed.y0, k: seed.side / n })?;
            if let Some(lm) = obs.landmarks() {
                let crop = Place::frame(n, n);
                let mut eyes: Vec<Vec<[f32; 2]>> = [region(lm.leftEye(), crop), region(lm.rightEye(), crop)].into_iter().flatten().collect();
                // Image-left eye first: the person's right.
                eyes.sort_by(|a, b| a[0][0].total_cmp(&b[0][0]));
                if eyes.len() == 2 {
                    let px = buf.read(|data, stride, w, h| {
                        let img = Bgra { data, stride, w, h };
                        [eye_pixels(&img, &eyes[1]), eye_pixels(&img, &eyes[0])]
                    });
                    if let Some([Some(l), Some(r)]) = px {
                        f.0.eye_px = [l, r];
                    }
                    let centre = |p: &[[f32; 2]]| {
                        let n = p.len().max(1) as f32;
                        [p.iter().map(|q| q[0]).sum::<f32>() / n, p.iter().map(|q| q[1]).sum::<f32>() / n]
                    };
                    let (a, b) = (centre(&eyes[0]), centre(&eyes[1]));
                    let eye_d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt().max(1.0);
                    if let Some(outer) = region(lm.outerLips(), crop) {
                        let (lo, hi) = outer.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
                        f.0.mouth_px = [(hi - lo) / eye_d, lip_corner_lift(&outer).unwrap_or(0.0)];
                    }
                }
            }
            self.adapt.apply(&mut f.0, t_ns, hand_on_face);
            Some(f)
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
            p.face = face(&obs, Place::frame(w, h));
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
