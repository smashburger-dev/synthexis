import CoreML
import CoreVideo
import Darwin
import Foundation

// Memory + per-frame pipeline probe: mem <seg.mlmodelc> <segSize> <hand.mlmodelc> <handSize>
// Per frame: one seg prediction + two hand-crop predictions, as the plugin would run them.
// Reports process physical footprint (what Activity Monitor shows as "Memory") at each stage.

func footprintMB() -> Double {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(
        MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576.0 : -1
}

func ms(_ d: Duration) -> Double {
    let c = d.components
    return Double(c.seconds) * 1000.0 + Double(c.attoseconds) / 1e15
}

func pixelBuffer(_ s: Int) -> CVPixelBuffer {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(
        kCFAllocatorDefault, s, s, kCVPixelFormatType_32BGRA,
        [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &pb)
    return pb!
}

func provider(_ model: MLModel, _ pb: CVPixelBuffer) throws -> MLDictionaryFeatureProvider {
    let name = model.modelDescription.inputDescriptionsByName.keys.first!
    return try MLDictionaryFeatureProvider(dictionary: [name: MLFeatureValue(pixelBuffer: pb)])
}

let a = CommandLine.arguments
let segURL = URL(fileURLWithPath: a[1])
let segSize = Int(a[2])!
let handURL = URL(fileURLWithPath: a[3])
let handSize = Int(a[4])!
let clock = ContinuousClock()

let base = footprintMB()
print(String(format: "MEM baseline_mb=%.1f", base))

let config = MLModelConfiguration()
config.computeUnits = .cpuAndNeuralEngine
let seg = try MLModel(contentsOf: segURL, configuration: config)
let hand = try MLModel(contentsOf: handURL, configuration: config)
let afterLoad = footprintMB()
print(String(format: "MEM after_load_mb=%.1f delta_mb=%.1f", afterLoad, afterLoad - base))

let segIn = try provider(seg, pixelBuffer(segSize))
let handInL = try provider(hand, pixelBuffer(handSize))
let handInR = try provider(hand, pixelBuffer(handSize))

func frame() throws {
    _ = try seg.prediction(from: segIn)
    _ = try hand.prediction(from: handInL)
    _ = try hand.prediction(from: handInR)
}

try frame()
let afterFirst = footprintMB()
print(String(format: "MEM after_first_frame_mb=%.1f delta_mb=%.1f", afterFirst, afterFirst - base))

for _ in 0..<50 { try frame() }

var times = [Double]()
var peak = footprintMB()
for i in 0..<3000 {
    let t = clock.now
    try frame()
    times.append(ms(t.duration(to: clock.now)))
    if i % 50 == 0 { peak = max(peak, footprintMB()) }
}
times.sort()
let med = times[times.count / 2]
let p95 = times[Int(0.95 * Double(times.count))]
let p99 = times[Int(0.99 * Double(times.count))]
print(String(
    format: "PIPELINE seg=%@ hand=%@ frames=3000 median_ms=%.3f p95_ms=%.3f p99_ms=%.3f max_fps_at_p95=%.0f",
    segURL.lastPathComponent, handURL.lastPathComponent, med, p95, p99, 1000.0 / p95))
print(String(format: "MEM steady_peak_mb=%.1f delta_mb=%.1f", peak, peak - base))
