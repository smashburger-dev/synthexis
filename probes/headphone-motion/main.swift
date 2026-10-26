import CoreMotion
import Foundation

// headphone-motion probe (P4): CMHeadphoneMotionManager yaw/pitch/roll for
// 30 s, connect/disconnect events, measured update rate.
// Log goes to stdout and raw_motion.log next to the app binary.

let runSeconds: Double = 30

final class Logger {
    let handle: FileHandle?
    init() {
        let url = URL(fileURLWithPath: CommandLine.arguments[0])
            .deletingLastPathComponent()
            .appendingPathComponent("raw_motion.log")
        FileManager.default.createFile(atPath: url.path, contents: nil)
        handle = try? FileHandle(forWritingTo: url)
        print("log file: \(url.path)")
    }
    func log(_ s: String) {
        let line = String(format: "[%@] %@",
                          ISO8601DateFormatter().string(from: Date()), s)
        print(line)
        if let h = handle { h.write((line + "\n").data(using: .utf8)!) }
    }
}

let log = Logger()

final class Delegate: NSObject, CMHeadphoneMotionManagerDelegate {
    func headphoneMotionManagerDidConnect(_ manager: CMHeadphoneMotionManager) {
        log.log("HEADPHONE connected")
    }
    func headphoneMotionManagerDidDisconnect(_ manager: CMHeadphoneMotionManager) {
        log.log("HEADPHONE disconnected")
    }
}

let manager = CMHeadphoneMotionManager()
let delegate = Delegate()
manager.delegate = delegate

let auth: String
if #available(macOS 14, *) {
    switch CMHeadphoneMotionManager.authorizationStatus() {
    case .authorized: auth = "authorized"
    case .denied: auth = "denied"
    case .restricted: auth = "restricted"
    case .notDetermined: auth = "notDetermined"
    @unknown default: auth = "unknown"
    }
} else { auth = "unavailable-pre-macOS14" }
log.log("authorizationStatus = \(auth)")
log.log("isDeviceMotionAvailable = \(manager.isDeviceMotionAvailable)")

if !manager.isDeviceMotionAvailable {
    log.log("no headphone motion device available - connect Powerbeats Pro 2 as audio output, wear them, rerun")
    exit(0)
}

var samples = 0
var firstTs: Double? = nil
var lastTs: Double = 0
let start = Date()

log.log("collecting \(runSeconds)s of yaw/pitch/roll ...")
manager.startDeviceMotionUpdates(to: .main) { motion, error in
    guard let m = motion else {
        log.log("update error: \(error?.localizedDescription ?? "nil")")
        return
    }
    samples += 1
    if firstTs == nil { firstTs = m.timestamp }
    lastTs = m.timestamp
    let r = m.attitude
    log.log(String(format: "t=%.3f yaw=%.1f pitch=%.1f roll=%.1f",
                   m.timestamp, r.yaw * 180 / .pi, r.pitch * 180 / .pi,
                   r.roll * 180 / .pi))
}

RunLoop.main.run(until: Date().addingTimeInterval(runSeconds))
manager.stopDeviceMotionUpdates()

let span = lastTs - (firstTs ?? lastTs)
let rate = span > 0 ? Double(samples - 1) / span : 0
log.log("DONE samples=\(samples) span_s=\(String(format: "%.1f", span)) measured_hz=\(String(format: "%.1f", rate))")
