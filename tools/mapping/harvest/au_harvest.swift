// au_harvest: list installed AU components and dump their parameter trees.
//
//   au_harvest --list
//   au_harvest --dump <type> <subtype> <manufacturer> [--oop]
//
// Built by build.sh to data/mapping/bin/au_harvest. One JSON line per
// component for --list; one JSON document for --dump. Diagnostics go to
// stderr so stdout stays machine-readable.

import AVFoundation
import AudioToolbox
import Foundation

let LIST_TYPES: Set<UInt32> = Set(
    ["aumu", "aufx", "aumf", "augn", "aumi"].compactMap(fourCC))
let INSTANTIATE_DEADLINE_S: TimeInterval = 50

func fourCC(_ s: String) -> UInt32? {
    if s.hasPrefix("0x") || s.hasPrefix("0X") {
        return UInt32(s.dropFirst(2), radix: 16)
    }
    let b = Array(s.utf8)
    guard b.count == 4 else { return nil }
    return b.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
}

func ccString(_ v: UInt32) -> String {
    let b: [UInt8] = [
        UInt8((v >> 24) & 0xff), UInt8((v >> 16) & 0xff),
        UInt8((v >> 8) & 0xff), UInt8(v & 0xff),
    ]
    if b.allSatisfy({ $0 >= 0x20 && $0 <= 0x7e }) {
        return String(bytes: b, encoding: .utf8) ?? String(format: "0x%08X", v)
    }
    return String(format: "0x%08X", v)
}

func t48(_ s: String?) -> String? {
    guard let s else { return nil }
    return s.count > 48 ? String(s.prefix(48)) : s
}

// Some plugins print diagnostics to stdout while being instantiated,
// which would corrupt the JSON document on stdout. --dump therefore
// takes --out <path> and writes the document to that file instead.
var OUT_PATH: String? = nil

func printJSON(_ obj: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: obj),
          let str = String(data: data, encoding: .utf8) else {
        FileHandle.standardError.write("JSONSerialization failed\n".data(using: .utf8)!)
        return
    }
    if let out = OUT_PATH {
        do {
            try str.write(toFile: out, atomically: true, encoding: .utf8)
        } catch {
            diag("write \(out): \(error.localizedDescription)")
        }
    } else {
        print(str)
    }
}

func diag(_ s: String) {
    FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
}

func unitWord(_ u: AudioUnitParameterUnit) -> String {
    switch u {
    case .generic: return "generic"
    case .indexed: return "indexed"
    case .boolean: return "boolean"
    case .percent: return "percent"
    case .seconds: return "seconds"
    case .sampleFrames: return "sample_frames"
    case .phase: return "phase"
    case .rate: return "rate"
    case .hertz: return "hertz"
    case .cents: return "cents"
    case .relativeSemiTones: return "relative_semitones"
    case .midiNoteNumber: return "midi_note"
    case .midiController: return "midi_controller"
    case .decibels: return "decibels"
    case .linearGain: return "linear_gain"
    case .degrees: return "degrees"
    case .equalPowerCrossfade: return "equal_power_crossfade"
    case .mixerFaderCurve1: return "mixer_fader_curve"
    case .pan: return "pan"
    case .meters: return "meters"
    case .absoluteCents: return "absolute_cents"
    case .octaves: return "octaves"
    case .BPM: return "bpm"
    case .beats: return "beats"
    case .milliseconds: return "milliseconds"
    case .ratio: return "ratio"
    case .customUnit: return "custom"
    case .midi2Controller: return "midi2_controller"
    default: return "generic"
    }
}

// AudioUnitParameterOptions bit layout (AudioUnitProperties.h)
func flagWords(_ f: UInt32) -> [String] {
    var out: [String] = []
    if f & (1 << 30) != 0 { out.append("readable") }
    if f & (1 << 31) != 0 { out.append("writable") }
    switch (f >> 16) & 7 {
    case 1: out.append("display_sqrt")
    case 2: out.append("display_squared")
    case 3: out.append("display_cubed")
    case 4: out.append("display_cube_root")
    case 5: out.append("display_exp")
    default: break
    }
    if f & (1 << 22) != 0 { out.append("display_log") }
    if f & (1 << 21) != 0 { out.append("values_have_strings") }
    if f & (1 << 23) != 0 { out.append("is_high_resolution") }
    if f & (1 << 28) != 0 { out.append("is_global_meta") }
    if f & (1 << 29) != 0 { out.append("meta") }
    if f & (1 << 25) != 0 { out.append("can_ramp") }
    if f & (1 << 26) != 0 { out.append("expert_mode") }
    if f & (1 << 13) != 0 { out.append("omit_from_presets") }
    if f & (1 << 15) != 0 { out.append("meter_read_only") }
    if f & (1 << 24) != 0 { out.append("non_real_time") }
    return out
}

func componentDict(_ c: AVAudioUnitComponent) -> [String: Any] {
    let d = c.audioComponentDescription
    return [
        "type": ccString(d.componentType),
        "subtype": ccString(d.componentSubType),
        "manufacturer": ccString(d.componentManufacturer),
        "name": c.name,
        "manufacturer_name": c.manufacturerName,
        "type_name": c.typeName,
        "version": c.version,
        "tags": c.allTagNames,
    ]
}

func allComponents() -> [AVAudioUnitComponent] {
    AVAudioUnitComponentManager.shared().components { _, _ in true }
}

func cmdList() -> Int {
    for c in allComponents() {
        let d = c.audioComponentDescription
        guard LIST_TYPES.contains(d.componentType) else { continue }
        if c.name.localizedCaseInsensitiveContains("synthexis")
            || c.manufacturerName.localizedCaseInsensitiveContains("synthexis") {
            continue
        }
        printJSON(componentDict(c))
    }
    return 0
}

func paramDict(_ p: AUParameter, index: Int, path: [String]) -> [String: Any] {
    let cur = p.value
    var d: [String: Any] = [
        "index": index,
        "address": p.address,
        "identifier": t48(p.identifier) ?? "",
        "name": t48(p.displayName) ?? "",
        "group": path.map { t48($0) ?? "" },
        "unit": unitWord(p.unit),
        "unit_name": t48(p.unitName) ?? NSNull(),
        "min": Double(p.minValue),
        "max": Double(p.maxValue),
        "default": Double(cur),
        "flags": flagWords(p.flags.rawValue),
    ]
    if let vs = p.valueStrings, !vs.isEmpty {
        d["value_strings"] = vs.prefix(64).map { t48($0) ?? "" }
    } else {
        var samples: [[Any]] = []
        for i in 0...10 {
            let t = Decimal(i) / Decimal(10)
            var v = p.minValue + Float(truncating: t as NSNumber)
                * (p.maxValue - p.minValue)
            let s = p.string(fromValue: &v)
            samples.append([NSDecimalNumber(decimal: t), t48(s) ?? NSNull()])
        }
        d["samples"] = samples
    }
    var curCopy = cur
    let ds = p.string(fromValue: &curCopy)
    d["default_string"] = t48(ds) ?? NSNull()
    return d
}

func walk(_ g: AUParameterGroup, path: [String], idx: inout Int, out: inout [[String: Any]]) {
    for node in g.children {
        if let p = node as? AUParameter {
            out.append(paramDict(p, index: idx, path: path))
            idx += 1
        } else if let sub = node as? AUParameterGroup {
            walk(sub, path: path + [t48(sub.displayName) ?? ""], idx: &idx, out: &out)
        }
    }
}

func countParams(_ g: AUParameterGroup) -> Int {
    var n = 0
    for node in g.children {
        if node is AUParameter { n += 1 }
        else if let sub = node as? AUParameterGroup { n += countParams(sub) }
    }
    return n
}

func cmdDump(typeStr: String, subStr: String, mfgStr: String, oop: Bool) -> Int {
    guard let ti = fourCC(typeStr), let si = fourCC(subStr), let mi = fourCC(mfgStr) else {
        diag("bad 4cc argument")
        return 2
    }
    var base: [String: Any] = [
        "type": ccString(ti), "subtype": ccString(si), "manufacturer": ccString(mi),
    ]
    if let comp = allComponents().first(where: {
        let d = $0.audioComponentDescription
        return d.componentType == ti && d.componentSubType == si
            && d.componentManufacturer == mi
    }) {
        base.merge(componentDict(comp)) { _, new in new }
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
    guard done else {
        base["error"] = "instantiate timed out (\(Int(INSTANTIATE_DEADLINE_S))s)"
        base["params"] = [[String: Any]]()
        printJSON(base)
        return 3
    }
    guard let au = auBox else {
        base["error"] = errBox?.localizedDescription ?? "instantiate failed"
        base["params"] = [[String: Any]]()
        printJSON(base)
        return 1
    }

    var tree = au.parameterTree
    if tree == nil || countParams(tree!) == 0 {
        try? au.allocateRenderResources()
        tree = au.parameterTree
    }

    var params: [[String: Any]] = []
    if let root = tree {
        var idx = 0
        walk(root, path: [t48(root.displayName) ?? ""], idx: &idx, out: &params)
    }
    base["params"] = params
    printJSON(base)
    return 0
}

let args = CommandLine.arguments
if let i = args.firstIndex(of: "--out"), i + 1 < args.count {
    OUT_PATH = args[i + 1]
}
var code = 0
if args.count >= 2 && args[1] == "--list" {
    code = cmdList()
} else if args.count >= 5 && args[1] == "--dump" {
    let oop = args.contains("--oop")
    code = cmdDump(typeStr: args[2], subStr: args[3], mfgStr: args[4], oop: oop)
} else {
    diag("usage: au_harvest --list | au_harvest --dump <type> <subtype> <manufacturer> [--oop] [--out <path>]")
    code = 2
}
exit(Int32(code))
