import CoreML
import CoreVideo
import Foundation

// Generic per-model bench: reads the model's input descriptions and feeds
// BGRA CVPixelBuffers for image inputs, zero MLMultiArray fp16 for tensor
// inputs. bench2 <model.mlmodelc>
// cpuAndNeuralEngine + all: 50 warmup + 1000 iters x 3 repeats.
// Also reports MLComputePlan op counts per preferred device and load/RSS.

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
            let w = Int(desc.imageConstraint!.pixelsWide)
            let h = Int(desc.imageConstraint!.pixelsHigh)
            dict[name] = MLFeatureValue(pixelBuffer: makePB(w, h))
        case .multiArray:
            let shape = desc.multiArrayConstraint!.shape
            let arr = try MLMultiArray(shape: shape, dataType: .float16)
            dict[name] = MLFeatureValue(multiArray: arr)
        default:
            fatalError("unsupported input \(name) type \(desc.type.rawValue)")
        }
    }
    return try MLDictionaryFeatureProvider(dictionary: dict)
}

// MLComputePlan (macOS 14.4+): count ops per preferred compute device and
// list op types whose preferred device is not the ANE.
@available(macOS 14.4, *)
func allOps(_ block: MLModelStructure.Program.Block) -> [MLModelStructure.Program.Operation] {
    block.operations.flatMap { [$0] + $0.blocks.flatMap(allOps) }
}

@available(macOS 14.4, *)
func opCoverageAsync(_ modelURL: URL, _ units: MLComputeUnits, _ name: String) async {
    let cfg = MLModelConfiguration(); cfg.computeUnits = units
    do {
        let plan = try await MLComputePlan.load(contentsOf: modelURL, configuration: cfg)
        var counts: [String: Int] = [:]
        var nonANE: Set<String> = []
        if case .program(let program) = plan.modelStructure {
            for (_, f) in program.functions {
                for op in allOps(f.block) {
                    var dev = "unknown"
                    if let usage = plan.deviceUsage(for: op) {
                        switch usage.preferred {
                        case .neuralEngine: dev = "ane"
                        case .gpu: dev = "gpu"
                        case .cpu: dev = "cpu"
                        @unknown default: dev = "other"
                        }
                    }
                    counts[dev, default: 0] += 1
                    if dev != "ane" { nonANE.insert(op.operatorName) }
                }
            }
        }
        print("OPS units=\(name) per_device=\(counts) non_ane_ops=\(nonANE.sorted())")
    } catch {
        print("OPS units=\(name) error=\(error)")
    }
}

func opCoverage(_ modelURL: URL) {
    guard #available(macOS 14.4, *) else {
        print("OPS unavailable (macOS <14.4)")
        return
    }
    let sem = DispatchSemaphore(value: 0)
    Task {
        await opCoverageAsync(modelURL, .cpuAndNeuralEngine, "cpuAndNeuralEngine")
        await opCoverageAsync(modelURL, .all, "all")
        sem.signal()
    }
    sem.wait()
}

let modelURL = URL(fileURLWithPath: CommandLine.arguments[1])
let clock = ContinuousClock()
let warmup = 50, iters = 1000, reps = 3

opCoverage(modelURL)
let rss0 = footprint()

for units in [MLComputeUnits.cpuAndNeuralEngine, .all] {
    let label = units == .cpuAndNeuralEngine ? "cpuAndNeuralEngine" : "all"
    let cfg = MLModelConfiguration(); cfg.computeUnits = units
    let t0 = clock.now
    let model = try MLModel(contentsOf: modelURL, configuration: cfg)
    let loadMs = ms(t0.duration(to: clock.now))
    let rss1 = footprint()
    let input = try makeInput(model)
    for _ in 0..<warmup { _ = try model.prediction(from: input) }
    var meds: [Double] = []
    var all: [[Double]] = []
    for _ in 0..<reps {
        var times = [Double](); times.reserveCapacity(iters)
        for _ in 0..<iters {
            let t = clock.now
            _ = try model.prediction(from: input)
            times.append(ms(t.duration(to: clock.now)))
        }
        times.sort()
        all.append(times)
        meds.append(pctl(times, 0.5))
    }
    let flat = all.flatMap { $0 }.sorted()
    print(String(format:
        "RESULT model=%@ units=%@ load_ms=%.2f rss_delta_mb=%.1f med=%.3f p95=%.3f p99=%.3f rep_meds=%@ spread=%.3f",
        modelURL.lastPathComponent, label, loadMs, Double(rss1 - rss0) / 1048576.0,
        pctl(flat, 0.5), pctl(flat, 0.95), pctl(flat, 0.99),
        meds.map { String(format: "%.3f", $0) }.joined(separator: ","),
        meds.max()! - meds.min()!))
}
