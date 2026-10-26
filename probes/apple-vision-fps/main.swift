import AVFoundation
import CoreImage
import CoreVideo
import Foundation
import Vision

// Apple Vision per-frame latency probe on test_clip.mp4.
// Reads via AVAssetReader (32BGRA), scales to 1280x720 with CoreImage,
// times each Vision request separately plus a combined hand+face+body call.
// Stateless detectors -> plain VNImageRequestHandler per frame; request
// objects are created once and reused across frames.

func ms(_ d: Duration) -> Double {
    let c = d.components
    return Double(c.seconds) * 1000.0 + Double(c.attoseconds) / 1e15
}

func percentile(_ s: [Double], _ p: Double) -> Double {
    s[min(s.count - 1, Int(p * Double(s.count)))]
}

let clipPath = CommandLine.arguments[1]
let maxFrames = CommandLine.arguments.count > 2 ? Int(CommandLine.arguments[2])! : 300
let outW = 1280
let outH = 720

let asset = AVAsset(url: URL(fileURLWithPath: clipPath))
guard let track = asset.tracks(withMediaType: .video).first else {
    fatalError("no video track")
}
let reader = try AVAssetReader(asset: asset)
let output = AVAssetReaderTrackOutput(
    track: track,
    outputSettings: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA
    ])
reader.add(output)
reader.startReading()

let ciContext = CIContext()
var dstPB: CVPixelBuffer?
CVPixelBufferCreate(
    kCFAllocatorDefault, outW, outH, kCVPixelFormatType_32BGRA,
    [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &dstPB)
let dst = dstPB!

let handReq = VNDetectHumanHandPoseRequest()
handReq.maximumHandCount = 2
let faceReq = VNDetectFaceLandmarksRequest()
let bodyReq = VNDetectHumanBodyPoseRequest()
let segFast = VNGeneratePersonSegmentationRequest()
segFast.qualityLevel = .fast
let segBal = VNGeneratePersonSegmentationRequest()
segBal.qualityLevel = .balanced
// Second set of request objects for the combined call: reusing the ones
// already performed on this frame would return cached results.
let handReq2 = VNDetectHumanHandPoseRequest()
handReq2.maximumHandCount = 2
let faceReq2 = VNDetectFaceLandmarksRequest()
let bodyReq2 = VNDetectHumanBodyPoseRequest()
let comboReqs: [VNRequest] = [handReq2, faceReq2, bodyReq2]

var times: [String: [Double]] = [
    "hand": [], "face": [], "body": [], "seg_fast": [], "seg_bal": [], "combo": [],
]
var nFrames = 0
var nHand = 0
var nFace = 0
var nBody = 0
let clock = ContinuousClock()

while nFrames < maxFrames,
    let sample = output.copyNextSampleBuffer(),
    let pb = CMSampleBufferGetImageBuffer(sample)
{
    let src = CIImage(cvPixelBuffer: pb)
    let scaled = src.transformed(
        by: CGAffineTransform(
            scaleX: CGFloat(outW) / src.extent.width,
            y: CGFloat(outH) / src.extent.height))
    ciContext.render(scaled, to: dst)

    let handler = VNImageRequestHandler(cvPixelBuffer: dst, orientation: .up)

    var t = clock.now
    try handler.perform([handReq])
    times["hand"]!.append(ms(t.duration(to: clock.now)))

    t = clock.now
    try handler.perform([faceReq])
    times["face"]!.append(ms(t.duration(to: clock.now)))

    t = clock.now
    try handler.perform([bodyReq])
    times["body"]!.append(ms(t.duration(to: clock.now)))

    t = clock.now
    try handler.perform([segFast])
    times["seg_fast"]!.append(ms(t.duration(to: clock.now)))

    t = clock.now
    try handler.perform([segBal])
    times["seg_bal"]!.append(ms(t.duration(to: clock.now)))

    // Fresh handler for the combined call: VNImageRequestHandler caches
    // results per request type, so reusing `handler` would not re-run work.
    let comboHandler = VNImageRequestHandler(cvPixelBuffer: dst, orientation: .up)
    t = clock.now
    try comboHandler.perform(comboReqs)
    times["combo"]!.append(ms(t.duration(to: clock.now)))

    if (handReq.results?.count ?? 0) >= 1 { nHand += 1 }
    if (faceReq.results?.count ?? 0) >= 1 { nFace += 1 }
    if (bodyReq.results?.count ?? 0) >= 1 { nBody += 1 }
    nFrames += 1
}

print("frames \(nFrames)")
for key in ["hand", "face", "body", "seg_fast", "seg_bal", "combo"] {
    let s = times[key]!.sorted()
    print(
        String(
            format: "RESULT %@ median_ms=%.3f p95_ms=%.3f",
            key, percentile(s, 0.5), percentile(s, 0.95)))
}
print(
    String(
        format: "SANITY frac_hand=%.3f frac_face=%.3f frac_body=%.3f",
        Double(nHand) / Double(nFrames), Double(nFace) / Double(nFrames),
        Double(nBody) / Double(nFrames)))
