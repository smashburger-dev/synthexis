import CoreML
import CoreVideo
import Foundation

// Latency bench: bench <model.mlmodelc> <width> <height>
// For each computeUnits: load+first-pred time, 50 warmup, 1000 timed preds.

func ms(_ d: Duration) -> Double {
    let c = d.components
    return Double(c.seconds) * 1000.0 + Double(c.attoseconds) / 1e15
}

func percentile(_ s: [Double], _ p: Double) -> Double {
    s[min(s.count - 1, Int(p * Double(s.count)))]
}

func makePixelBuffer(_ w: Int, _ h: Int) -> CVPixelBuffer {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(
        kCFAllocatorDefault, w, h, kCVPixelFormatType_32BGRA,
        [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &pb)
    return pb!
}

let args = CommandLine.arguments
let modelURL = URL(fileURLWithPath: args[1])
let width = Int(args[2])!
let height = Int(args[3])!
let warmup = 50
let iters = 1000
let clock = ContinuousClock()

let unitSets: [(String, MLComputeUnits)] = [
    ("cpuOnly", .cpuOnly),
    ("cpuAndGPU", .cpuAndGPU),
    ("cpuAndNeuralEngine", .cpuAndNeuralEngine),
    ("all", .all),
]

let pixelBuffer = makePixelBuffer(width, height)

for (label, units) in unitSets {
    let config = MLModelConfiguration()
    config.computeUnits = units

    let t0 = clock.now
    let model = try MLModel(contentsOf: modelURL, configuration: config)
    let loadMs = ms(t0.duration(to: clock.now))

    guard let inputName = model.modelDescription.inputDescriptionsByName.keys.first
    else { fatalError("no inputs") }

    let fv = MLFeatureValue(pixelBuffer: pixelBuffer)
    let provider = try MLDictionaryFeatureProvider(dictionary: [inputName: fv])

    let t1 = clock.now
    _ = try model.prediction(from: provider)
    let firstMs = ms(t1.duration(to: clock.now))

    for _ in 0..<warmup {
        _ = try model.prediction(from: provider)
    }

    var times = [Double]()
    times.reserveCapacity(iters)
    for _ in 0..<iters {
        let t = clock.now
        _ = try model.prediction(from: provider)
        times.append(ms(t.duration(to: clock.now)))
    }
    times.sort()
    print(
        String(
            format:
                "RESULT model=%@ units=%@ load_ms=%.2f first_pred_ms=%.2f median_ms=%.3f p95_ms=%.3f p99_ms=%.3f",
            modelURL.lastPathComponent, label, loadMs, firstMs,
            percentile(times, 0.5), percentile(times, 0.95), percentile(times, 0.99)))
}
