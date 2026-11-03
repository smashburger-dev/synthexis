// au_states: dump the text of every state of the indexed and boolean
// parameters of one AU component, plus a dense text scan of selected
// continuous parameters (hidden steps) and the factory preset names.
//
//   au_states --dump <type> <subtype> <manufacturer> --out <path>
//       [--oop] [--dense <addresses.txt>]
//
// Instantiation follows tools/mapping/harvest/au_harvest.swift.

import AVFoundation
import AudioToolbox
import Foundation

let INSTANTIATE_DEADLINE_S: TimeInterval = 50
let MAX_STATES = 4096
let DENSE_N = 512

func fourCC(_ s: String) -> UInt32? {
    if s.hasPrefix("0x") || s.hasPrefix("0X") {
        return UInt32(s.dropFirst(2), radix: 16)
    }
    let b = Array(s.utf8)
    guard b.count == 4 else { return nil }
    return b.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
}

func t48(_ s: String?) -> String {
    guard let s else { return "" }
    return s.count > 48 ? String(s.prefix(48)) : s
}

func diag(_ s: String) {
    FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
}

func write(_ obj: [String: Any], to path: String) {
    guard let data = try? JSONSerialization.data(withJSONObject: obj) else {
        diag("JSONSerialization failed")
        return
    }
    FileManager.default.createFile(atPath: path, contents: data)
}

func text(_ p: AUParameter, _ v: Float) -> String {
    var x = v
    return t48(p.string(fromValue: &x))
}

/// Every integer value from min to max: [normalized, text, value_string].
func steppedStates(_ p: AUParameter) -> [String: Any] {
    let lo = Int(p.minValue.rounded()), hi = Int(p.maxValue.rounded())
    let n = hi - lo + 1
    var d: [String: Any] = ["source": "steps", "steps": max(n - 1, 0)]
    guard n >= 1, n <= MAX_STATES else {
        d["truncated"] = true
        d["states"] = [[Any]]()
        return d
    }
    let vs = p.valueStrings ?? []
    var states: [[Any]] = []
    for k in 0..<n {
        let v = Float(lo + k)
        let t = n > 1 ? Double(k) / Double(n - 1) : 0
        let s = text(p, v)
        let w = k < vs.count ? t48(vs[k]) : ""
        states.append([t, s.isEmpty ? w : s, w])
    }
    d["states"] = states
    d["n_value_strings"] = vs.count
    return d
}

/// Text at DENSE_N + 1 points, run-length coded as [t_first, t_last, text].
func denseRuns(_ p: AUParameter) -> [String: Any] {
    var runs: [[Any]] = []
    var prev = "", t0 = 0.0, tl = 0.0
    for i in 0...DENSE_N {
        let t = Double(i) / Double(DENSE_N)
        let v = p.minValue + Float(t) * (p.maxValue - p.minValue)
        let s = text(p, v)
        if i == 0 { prev = s; t0 = t; tl = t; continue }
        if s != prev {
            runs.append([t0, tl, prev])
            prev = s; t0 = t
        }
        tl = t
    }
    runs.append([t0, tl, prev])
    return ["source": "dense", "dense_n": DENSE_N, "runs": runs]
}

func walk(_ g: AUParameterGroup, idx: inout Int, dense: Set<UInt64>,
          out: inout [[String: Any]]) {
    for node in g.children {
        if let p = node as? AUParameter {
            let stepped = p.unit == .indexed || p.unit == .boolean
                || p.valueStrings != nil
            if stepped || dense.contains(p.address) {
                var d: [String: Any] = [
                    "index": idx, "address": p.address,
                    "name": t48(p.displayName),
                    "unit": p.unit == .indexed ? "indexed"
                        : p.unit == .boolean ? "boolean" : "other",
                    "min": Double(p.minValue), "max": Double(p.maxValue),
                    "default": Double(p.value),
                ]
                d.merge(stepped ? steppedStates(p) : denseRuns(p)) { _, b in b }
                out.append(d)
            }
            idx += 1
        } else if let sub = node as? AUParameterGroup {
            walk(sub, idx: &idx, dense: dense, out: &out)
        }
    }
}

func cmdDump(_ args: [String], out: String, oop: Bool,
             dense: Set<UInt64>) -> Int32 {
    guard let ti = fourCC(args[0]), let si = fourCC(args[1]),
          let mi = fourCC(args[2]) else {
        diag("bad 4cc argument")
        return 2
    }
    let desc = AudioComponentDescription(
        componentType: ti, componentSubType: si, componentManufacturer: mi,
        componentFlags: 0, componentFlagsMask: 0)
    let opts: AudioComponentInstantiationOptions = oop ? [.loadOutOfProcess] : []
    var auBox: AUAudioUnit?
    var errBox: Error?
    var done = false
    AUAudioUnit.instantiate(with: desc, options: opts) { au, err in
        auBox = au
        errBox = err
        done = true
    }
    let deadline = Date().addingTimeInterval(INSTANTIATE_DEADLINE_S)
    while !done && Date() < deadline {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
    guard done, let au = auBox else {
        let e = done ? (errBox?.localizedDescription ?? "instantiate failed")
            : "instantiate timed out (\(Int(INSTANTIATE_DEADLINE_S))s)"
        write(["error": e, "params": [[String: Any]]()], to: out)
        return done ? 1 : 3
    }
    var tree = au.parameterTree
    if tree == nil || tree!.allParameters.isEmpty {
        try? au.allocateRenderResources()
        tree = au.parameterTree
    }
    var params: [[String: Any]] = []
    if let root = tree {
        var idx = 0
        walk(root, idx: &idx, dense: dense, out: &params)
    }
    let presets = (au.factoryPresets ?? []).map { t48($0.name) }
    write(["params": params, "factory_presets": presets], to: out)
    return 0
}

let argv = CommandLine.arguments
func opt(_ name: String) -> String? {
    guard let i = argv.firstIndex(of: name), i + 1 < argv.count else { return nil }
    return argv[i + 1]
}
guard argv.count >= 5, argv[1] == "--dump", let out = opt("--out") else {
    diag("usage: au_states --dump <type> <subtype> <manufacturer> --out <path> [--oop] [--dense <file>]")
    exit(2)
}
var dense = Set<UInt64>()
if let f = opt("--dense"), let s = try? String(contentsOfFile: f, encoding: .utf8) {
    dense = Set(s.split(whereSeparator: \.isWhitespace).compactMap { UInt64($0) })
}
exit(cmdDump(Array(argv[2...4]), out: out, oop: argv.contains("--oop"), dense: dense))
