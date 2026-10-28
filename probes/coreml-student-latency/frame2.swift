import CoreML
import CoreVideo
import Foundation

// Full D10 perception-frame bench:
//   frame2 <seg.mlmodelc> <face.mlmodelc> <hand_b1.mlmodelc> <hand_b2.mlmodelc>
// cpuAndNeuralEngine, 50 warmup + 3000 frames. Per frame:
//   seg predict -> face predict -> hand a) 2x batch-1 AND b) 1x batch-2
//   -> CPU geometry: argmax over 7xHxW fp16 seg output + per-class
//      count/centroid/bbox.
// Reports per-stage and total median/p95/p99 + RSS delta.

func ms(_ d: Duration) -> Double {
    let c = d.components
    return Double(c.seconds) * 1000.0 + Double(c.attoseconds) / 1e15
}
func pctl(_ s: [Double], _ p: Double) -> Double {
    s[min(s.count - 1, Int(p * Double(s.count)))]
}
func footprint() -> Int64 {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size) / 4
    let r = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return r == KERN_SUCCESS ? Int64(info.phys_footprint) : -1
}
func makePB(_ w: Int, _ h: Int) -> CVPixelBuffer {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, w, h, kCVPixelFormatType_32BGRA,
        [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &pb)
    return pb!
}
func makeInput(_ model: MLModel) throws -> MLFeatureProvider {
    var dict = [String: MLFeatureValue]()
    for (name, desc) in model.modelDescription.inputDescriptionsByName {
        switch desc.type {
        case .image:
            dict[name] = MLFeatureValue(pixelBuffer: makePB(
                Int(desc.imageConstraint!.pixelsWide),
                Int(desc.imageConstraint!.pixelsHigh)))
        case .multiArray:
            let arr = try MLMultiArray(
                shape: desc.multiArrayConstraint!.shape, dataType: .float16)
            dict[name] = MLFeatureValue(multiArray: arr)
        default:
            fatalError("unsupported input \(name)")
        }
    }
    return try MLDictionaryFeatureProvider(dictionary: dict)
}

let a = CommandLine.arguments
let clock = ContinuousClock()
let cfg = MLModelConfiguration(); cfg.computeUnits = .cpuAndNeuralEngine
let rss0 = footprint()

let seg = try MLModel(contentsOf: URL(fileURLWithPath: a[1]), configuration: cfg)
let face = try MLModel(contentsOf: URL(fileURLWithPath: a[2]), configuration: cfg)
let handA = try MLModel(contentsOf: URL(fileURLWithPath: a[3]), configuration: cfg)
let handB = try MLModel(contentsOf: URL(fileURLWithPath: a[4]), configuration: cfg)
let segIn = try makeInput(seg), faceIn = try makeInput(face)
let handAIn = try makeInput(handA), handBIn = try makeInput(handB)
let segOutName = seg.modelDescription.outputDescriptionsByName.keys.first!
let rss1 = footprint()

var tSeg = [Double](), tFace = [Double](), tHandA = [Double]()
var tHandB = [Double](), tGeom = [Double](), tTotA = [Double](), tTotB = [Double]()

let frames = a.count > 5 ? Int(a[5])! : 3000
let warmup = 50
for i in 0..<(frames + warmup) {
    let tAll = clock.now
    var t = clock.now
    let segRes = try seg.prediction(from: segIn)
    let dSeg = ms(t.duration(to: clock.now))
    t = clock.now
    _ = try face.prediction(from: faceIn)
    let dFace = ms(t.duration(to: clock.now))
    t = clock.now
    _ = try handA.prediction(from: handAIn)
    _ = try handA.prediction(from: handAIn)
    let dHandA = ms(t.duration(to: clock.now))
    t = clock.now
    _ = try handB.prediction(from: handBIn)
    let dHandB = ms(t.duration(to: clock.now))
    t = clock.now
    // Geometry: argmax over classes + per-class count/centroid/bbox.
    let arr = segRes.featureValue(for: segOutName)!.multiArrayValue!
    let C = arr.shape[1].intValue, H = arr.shape[2].intValue, W = arr.shape[3].intValue
    let ptr = arr.dataPointer.assumingMemoryBound(to: Float16.self)
    let sC = arr.strides[1].intValue, sH = arr.strides[2].intValue, sW = arr.strides[3].intValue
    var count = [Int](repeating: 0, count: C)
    var sx = [Int](repeating: 0, count: C), sy = [Int](repeating: 0, count: C)
    var mn = [(Int, Int)](repeating: (1 << 30, 1 << 30), count: C)
    var mx = [(Int, Int)](repeating: (-1, -1), count: C)
    for y in 0..<H {
        for x in 0..<W {
            var best = 0
            var bv = Float16.leastNormalMagnitude
            for c in 0..<C {
                let v = ptr[c * sC + y * sH + x * sW]
                if v > bv { bv = v; best = c }
            }
            count[best] += 1; sx[best] += x; sy[best] += y
            mn[best].0 = min(mn[best].0, x); mn[best].1 = min(mn[best].1, y)
            mx[best].0 = max(mx[best].0, x); mx[best].1 = max(mx[best].1, y)
        }
    }
    _ = (count, sx, sy, mn, mx)
    let dGeom = ms(t.duration(to: clock.now))
    let dAll = ms(tAll.duration(to: clock.now))
    if i >= warmup {
        tSeg.append(dSeg); tFace.append(dFace)
        tHandA.append(dHandA); tHandB.append(dHandB); tGeom.append(dGeom)
        tTotA.append(dAll - dHandB)
        tTotB.append(dAll - dHandA)
    }
}
let rss2 = footprint()
func row(_ n: String, _ v: [Double]) {
    let s = v.sorted()
    print(String(format: "STAGE %-8s med=%.3f p95=%.3f p99=%.3f", n,
                 pctl(s, 0.5), pctl(s, 0.95), pctl(s, 0.99)))
}
print("MODEL seg=\(a[1].split(separator: "/").last!) frames=\(frames)")
row("seg", tSeg); row("face", tFace); row("hand_2xb1", tHandA)
row("hand_1xb2", tHandB); row("geom", tGeom)
row("total_a", tTotA); row("total_b", tTotB)
print(String(format: "RSS_DELTA_MB %.1f", Double(rss2 - rss0) / 1048576.0))
