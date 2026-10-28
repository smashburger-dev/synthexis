// CamLat — camera capture latency bench for hill-climb H1.
//
// Records per-frame {seq, pts_host_s, cb_host_s, luma, dropped} as JSONL and
// (optionally) flashes the whole screen black/white, timestamping each toggle
// with CAMetalDrawable.presentedTime (host-time seconds) as ground truth.
// No camera images are written to disk — only numbers.

import AVFoundation
import AppKit
import CoreMedia
import CoreVideo
import Foundation
import Metal
import ObjectiveC
import QuartzCore

// MARK: - Args

struct Args {
    var label = "run"
    var seconds: Double = 40
    var device = "builtin"
    var mode = "preset720"
    var fps = "auto"
    var pix = "bgra"
    var qos = "default"
    var injectMs: Double = 0
    var injectMode = "busy" // busy | sleep | busy-after
    var spinnerPct = 0
    var holdMs: Double = 0
    var flash = false
    var out = ""

    static func parse() -> Args {
        var a = Args()
        var it = CommandLine.arguments.dropFirst()
        while let k = it.popFirst() {
            let v = it.popFirst() ?? ""
            switch k {
            case "--label": a.label = v
            case "--seconds": a.seconds = Double(v) ?? a.seconds
            case "--device": a.device = v
            case "--mode": a.mode = v
            case "--fps": a.fps = v
            case "--pix": a.pix = v
            case "--qos": a.qos = v
            case "--inject-ms": a.injectMs = Double(v) ?? 0
            case "--inject-mode": a.injectMode = v
            case "--spinner-pct": a.spinnerPct = Int(v) ?? 0
            case "--hold-ms": a.holdMs = Double(v) ?? 0
            case "--flash": a.flash = (v == "1")
            case "--out": a.out = v
            default: break
            }
        }
        return a
    }
}

// MARK: - Shared state

let args = Args.parse()
let hostClock = CMClockGetHostTimeClock()
var machTicksPerSec: Double = {
    var info = mach_timebase_info_data_t()
    mach_timebase_info(&info)
    return 1e9 * Double(info.denom) / Double(info.numer)
}()

func hostNow() -> Double { CMClockGetTime(hostClock).seconds }

func busyWait(ms: Double) {
    guard ms > 0 else { return }
    let deadline = mach_absolute_time() + UInt64(ms * 1e-3 * machTicksPerSec)
    while mach_absolute_time() < deadline {}
}

/// JSONSerialization fails on NaN/Inf; clamp doubles before serializing.
func jsonSafe(_ v: Any) -> Any {
    if let d = v as? Double, !d.isFinite { return NSNull() }
    if let a = v as? [Any] { return a.map(jsonSafe) }
    if let d = v as? [String: Any] {
        return d.mapValues(jsonSafe)
    }
    return v
}

func loadavg() -> String {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/sbin/sysctl")
    p.arguments = ["-n", "vm.loadavg"]
    let pipe = Pipe()
    p.standardOutput = pipe
    do {
        try p.run()
        p.waitUntilExit()
        let s = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? "?"
        return s.trimmingCharacters(in: .whitespacesAndNewlines)
    } catch { return "?" }
}

// JSONL writer, serialized.
final class JsonlWriter {
    let fh: FileHandle
    let lock = NSLock()
    init(path: String) throws {
        FileManager.default.createFile(atPath: path, contents: nil)
        fh = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
    }
    func write(_ dict: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: dict),
              var line = String(data: data, encoding: .utf8) else { return }
        line.append("\n")
        lock.lock()
        fh.write(line.data(using: .utf8)!)
        lock.unlock()
    }
}

var writer: JsonlWriter!
var syncClock: CMClock?
var frameSeq = 0
var droppedTotal = 0
var previewBuf = [UInt8](repeating: 0, count: 320 * 180 * 4)
let holdQueue = DispatchQueue(label: "camlat.hold")
var spinnerStop = false

// MARK: - Frame processing

// Full-frame mean luma, subsampled every 8th pixel/row. For 420v reads the
// Y plane; for BGRA Rec.601.
func meanLumaBGRA(_ base: UnsafePointer<UInt8>, stride: Int, w: Int, h: Int) -> Double {
    var sum = 0.0, n = 0
    var y = 0
    while y < h {
        let row = base.advanced(by: y * stride)
        var x = 0
        while x < w {
            let px = row.advanced(by: x * 4)
            sum += 0.114 * Double(px[0]) + 0.587 * Double(px[1]) + 0.299 * Double(px[2])
            n += 1
            x += 8
        }
        y += 8
    }
    return n == 0 ? 0 : sum / Double(n)
}

func meanLumaY(_ base: UnsafePointer<UInt8>, stride: Int, w: Int, h: Int) -> Double {
    var sum = 0.0, n = 0
    var y = 0
    while y < h {
        let row = base.advanced(by: y * stride)
        var x = 0
        while x < w {
            sum += Double(row[x])
            n += 1
            x += 8
        }
        y += 8
    }
    return n == 0 ? 0 : sum / Double(n)
}

// Plugin parity: mean of the central quarter + a 320x180 nearest downscale
// kept in previewBuf (never written to disk).
func pluginWorkBGRA(_ base: UnsafePointer<UInt8>, stride: Int, w: Int, h: Int) {
    var sum = 0, n = 0
    var y = h * 3 / 8
    while y < h * 5 / 8 {
        let row = base.advanced(by: y * stride)
        var x = w * 3 / 8
        while x < w * 5 / 8 {
            let px = row.advanced(by: x * 4)
            sum += (Int(px[2]) * 3 + Int(px[1]) * 6 + Int(px[0])) / 10
            n += 1
            x += 2
        }
        y += 2
    }
    _ = n == 0 ? 0 : sum / n
    for dy in 0..<180 {
        let sy = dy * h / 180
        let row = base.advanced(by: sy * stride)
        for dx in 0..<320 {
            let sx = dx * w / 320
            let px = row.advanced(by: sx * 4)
            let i = (dy * 320 + dx) * 4
            previewBuf[i] = px[2]
            previewBuf[i + 1] = px[1]
            previewBuf[i + 2] = px[0]
            previewBuf[i + 3] = 255
        }
    }
}

func pluginWorkY(_ base: UnsafePointer<UInt8>, stride: Int, w: Int, h: Int) {
    var sum = 0, n = 0
    var y = h * 3 / 8
    while y < h * 5 / 8 {
        let row = base.advanced(by: y * stride)
        var x = w * 3 / 8
        while x < w * 5 / 8 {
            sum += Int(row[x])
            n += 1
            x += 2
        }
        y += 2
    }
    _ = n == 0 ? 0 : sum / n
    for dy in 0..<180 {
        let sy = dy * h / 180
        let row = base.advanced(by: sy * stride)
        for dx in 0..<320 {
            let sx = dx * w / 320
            previewBuf[dy * 320 + dx] = row[sx]
        }
    }
}

// MARK: - Capture delegate

final class CaptureDelegate: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    func captureOutput(_ output: AVCaptureOutput,
                       didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        // Injection: 'busy' burns CPU before the timestamp, 'sleep' yields
        // the same wall time, 'busy-after' timestamps first then burns — so
        // the measured latency excludes the injected time while the callback
        // still occupies the queue for it.
        switch args.injectMode {
        case "busy": busyWait(ms: args.injectMs)
        case "sleep": Thread.sleep(forTimeInterval: args.injectMs / 1000.0)
        default: break
        }
        let cb = hostNow()
        if args.injectMode == "busy-after" { busyWait(ms: args.injectMs) }
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        let ptsHost = syncClock.map {
            CMSyncConvertTime(pts, from: $0, to: hostClock).seconds
        } ?? pts.seconds
        var luma = 0.0
        if let pb = CMSampleBufferGetImageBuffer(sampleBuffer) {
            CVPixelBufferLockBaseAddress(pb, .readOnly)
            let fmt = CVPixelBufferGetPixelFormatType(pb)
            let planeCount = CVPixelBufferGetPlaneCount(pb)
            if planeCount >= 1 {
                let w = CVPixelBufferGetWidthOfPlane(pb, 0)
                let h = CVPixelBufferGetHeightOfPlane(pb, 0)
                let stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0)
                if let base = CVPixelBufferGetBaseAddressOfPlane(pb, 0) {
                    let b = base.assumingMemoryBound(to: UInt8.self)
                    luma = meanLumaY(b, stride: stride, w: w, h: h)
                    pluginWorkY(b, stride: stride, w: w, h: h)
                }
            } else {
                let w = CVPixelBufferGetWidth(pb)
                let h = CVPixelBufferGetHeight(pb)
                let stride = CVPixelBufferGetBytesPerRow(pb)
                if let base = CVPixelBufferGetBaseAddress(pb) {
                    let b = base.assumingMemoryBound(to: UInt8.self)
                    if fmt == kCVPixelFormatType_32BGRA {
                        luma = meanLumaBGRA(b, stride: stride, w: w, h: h)
                        pluginWorkBGRA(b, stride: stride, w: w, h: h)
                    } else {
                        luma = meanLumaY(b, stride: stride, w: w, h: h)
                    }
                }
            }
            CVPixelBufferUnlockBaseAddress(pb, .readOnly)
        }
        // Buffer-holding test: retain the sample buffer for hold-ms on
        // another queue; the callback itself returns immediately.
        if args.holdMs > 0 {
            holdQueue.asyncAfter(deadline: .now() + args.holdMs / 1000.0) {
                _ = sampleBuffer
            }
        }
        frameSeq += 1
        writer.write([
            "t": "f", "seq": frameSeq, "pts_host_s": ptsHost, "cb_host_s": cb,
            "luma": luma, "dropped": droppedTotal,
        ])
    }

    func captureOutput(_ output: AVCaptureOutput,
                       didDrop sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        droppedTotal += 1
    }
}

// MARK: - Flash windows

final class Flasher {
    var windows: [NSWindow] = []
    var layers: [CAMetalLayer] = []
    var queue: MTLCommandQueue!
    var edgeLock = NSLock()
    var edges: [[String: Any]] = []
    var toggleIndex = 0
    var pending: [Int: [Double]] = [:]   // toggle index -> presentedTimes per screen
    var stop = false
    var warmDark = NSColor.black
    var thread: Thread?

    func setup() {
        guard let device = MTLCreateSystemDefaultDevice() else {
            print("flash: no Metal device, flashing disabled")
            return
        }
        queue = device.makeCommandQueue()
        for screen in NSScreen.screens {
            let win = NSWindow(contentRect: screen.frame,
                               styleMask: [.borderless],
                               backing: .buffered,
                               defer: false)
            win.level = NSWindow.Level(rawValue: Int(CGShieldingWindowLevel()) + 1)
            win.isOpaque = true
            win.hasShadow = false
            win.ignoresMouseEvents = true
            let view = NSView(frame: screen.frame)
            let layer = CAMetalLayer()
            layer.device = device
            layer.pixelFormat = .bgra8Unorm
            layer.framebufferOnly = true
            layer.drawableSize = CGSize(width: screen.frame.width * screen.backingScaleFactor,
                                        height: screen.frame.height * screen.backingScaleFactor)
            layer.contentsScale = screen.backingScaleFactor
            view.layer = layer
            view.wantsLayer = true
            win.contentView = view
            win.orderFrontRegardless()
            windows.append(win)
            layers.append(layer)
        }
        present(color: (0, 0, 0), index: -1, rising: false)
        thread = Thread { self.loop() }
        thread?.start()
    }

    func present(color: (Double, Double, Double), index: Int, rising: Bool) {
        for layer in layers {
            guard let drawable = layer.nextDrawable() else { continue }
            let pass = MTLRenderPassDescriptor()
            pass.colorAttachments[0].texture = drawable.texture
            pass.colorAttachments[0].loadAction = .clear
            pass.colorAttachments[0].storeAction = .store
            pass.colorAttachments[0].clearColor =
                MTLClearColor(red: color.0, green: color.1, blue: color.2, alpha: 1)
            guard let cb = queue.makeCommandBuffer(),
                  let enc = cb.makeRenderCommandEncoder(descriptor: pass) else { continue }
            enc.endEncoding()
            drawable.addPresentedHandler { [weak self] d in
                guard let self = self, index >= 0, d.presentedTime > 0 else { return }
                self.edgeLock.lock()
                self.pending[index, default: []].append(d.presentedTime)
                self.edgeLock.unlock()
            }
            cb.present(drawable)
            cb.commit()
        }
        if index >= 0 {
            edgeLock.lock()
            pending[index] = pending[index] ?? []
            edgeLock.unlock()
        }
    }

    func loop() {
        // 3 s dark warm-up, then alternating flashes with 350-700 ms dwell.
        Thread.sleep(forTimeInterval: 3.0)
        var white = true
        while !stop {
            toggleIndex += 1
            let idx = toggleIndex
            present(color: white ? (1, 1, 1) : (0, 0, 0), index: idx, rising: white)
            // Give presented handlers a moment before sealing the edge.
            Thread.sleep(forTimeInterval: 0.05)
            edgeLock.lock()
            let times = pending.removeValue(forKey: idx) ?? []
            edgeLock.unlock()
            let t_e = times.min() ?? 0.0
            edgeLock.lock()
            edges.append(["t": "e", "t_e": t_e, "dir": white ? "r" : "f"])
            edgeLock.unlock()
            Thread.sleep(forTimeInterval: 0.30 + Double.random(in: 0..<0.35))
            white.toggle()
        }
    }

    func shutdown() {
        stop = true
        thread?.cancel()
        // Let the last presented handlers arrive.
        Thread.sleep(forTimeInterval: 0.5)
        edgeLock.lock()
        for e in edges { writer.write(e) }
        edgeLock.unlock()
        // NSWindow close must happen on the main thread; finish() runs on a
        // background queue and NSWindow._close traps off-main.
        DispatchQueue.main.sync {
            for w in self.windows { w.close() }
            self.windows.removeAll()
            self.layers.removeAll()
        }
    }
}

// MARK: - App

final class App: NSObject, NSApplicationDelegate {
    let flasher = Flasher()
    var session: AVCaptureSession?
    var device: AVCaptureDevice?
    var output: AVCaptureVideoDataOutput?
    var captureDelegate: CaptureDelegate?
    var info: [String: Any] = [:]
    var ts: Int = 0
    var loadStart = "?"
    var startTime = Date()

    func run() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.delegate = self

        ts = Int(Date().timeIntervalSince1970)
        try? FileManager.default.createDirectory(atPath: args.out, withIntermediateDirectories: true)
        let jsonlPath = "\(args.out)/\(ts)-\(args.label).jsonl"
        do { writer = try JsonlWriter(path: jsonlPath) } catch {
            print("cannot open \(jsonlPath): \(error)"); exit(2)
        }

        // Permission.
        var status = AVCaptureDevice.authorizationStatus(for: .video)
        if status == .notDetermined {
            AVCaptureDevice.requestAccess(for: .video) { _ in }
            let deadline = Date().addingTimeInterval(120)
            while status == .notDetermined && Date() < deadline {
                Thread.sleep(forTimeInterval: 0.2)
                status = AVCaptureDevice.authorizationStatus(for: .video)
            }
        }
        guard status == .authorized else {
            print("camera authorization = \(status.rawValue), cannot run")
            exit(3)
        }

        // Device selection — never defaultDevice.
        let session = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInWideAngleCamera, .continuityCamera, .external],
            mediaType: .video,
            position: .unspecified)
        let pick: AVCaptureDevice?
        if args.device == "iphone" {
            pick = session.devices.first { $0.localizedName.contains("iPhone") }
                ?? session.devices.first { $0.deviceType == .continuityCamera }
        } else {
            pick = session.devices.first { $0.deviceType == .builtInWideAngleCamera }
        }
        guard let device = pick else {
            print("no matching capture device for --device \(args.device)")
            exit(4)
        }
        self.device = device
        print("device: \(device.localizedName) id=\(device.uniqueID)")

        do { try device.lockForConfiguration() } catch {
            print("lockForConfiguration failed: \(error)"); exit(5)
        }

        if args.mode != "preset720" {
            // format:WxH — match dims, mediaSubType 420v, landscape.
            let dims = args.mode.dropFirst("format:".count).split(separator: "x")
            let w = Int(dims[0])!, h = Int(dims[1])!
            let wantFps = args.fps == "auto" ? 0 : (Int(args.fps) ?? 0)
            guard let fmt = device.formats.first(where: {
                let d = CMVideoFormatDescriptionGetDimensions($0.formatDescription)
                let sub = CMFormatDescriptionGetMediaSubType($0.formatDescription)
                guard Int(d.width) == w && Int(d.height) == h
                    && sub == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange else { return false }
                if wantFps > 0 {
                    return $0.videoSupportedFrameRateRanges.contains {
                        Double(wantFps) >= $0.minFrameRate && Double(wantFps) <= $0.maxFrameRate
                    }
                }
                return true
            }) else {
                print("no \(w)x\(h) 420v format supporting fps=\(wantFps) on device"); exit(6)
            }
            device.activeFormat = fmt
        }
        if args.fps != "auto" {
            let f = Int(args.fps)!
            let ok = device.activeFormat.videoSupportedFrameRateRanges.contains {
                f >= Int($0.minFrameRate.rounded()) && f <= Int($0.maxFrameRate.rounded())
            }
            guard ok else { print("fps \(f) unsupported by activeFormat"); exit(7) }
            let dur = CMTime(value: 1, timescale: CMTimeScale(f))
            device.activeVideoMinFrameDuration = dur
            device.activeVideoMaxFrameDuration = dur
        }
        device.unlockForConfiguration()

        let input: AVCaptureDeviceInput
        do { input = try AVCaptureDeviceInput(device: device) } catch {
            print("input failed: \(error)"); exit(8)
        }
        let out = AVCaptureVideoDataOutput()
        if args.pix == "420v" {
            out.videoSettings = [
                kCVPixelBufferPixelFormatTypeKey as String:
                    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            ]
        } else {
            out.videoSettings = [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA
            ]
        }
        let capQueue: DispatchQueue = args.qos == "ui"
            ? DispatchQueue(label: "camlat.capture", qos: .userInteractive)
            : DispatchQueue(label: "camlat.capture")
        let delegate = CaptureDelegate()
        self.captureDelegate = delegate
        out.setSampleBufferDelegate(delegate, queue: capQueue)

        let s = AVCaptureSession()
        if args.mode == "preset720" && s.canSetSessionPreset(.hd1280x720) {
            s.sessionPreset = .hd1280x720
        }
        if s.canAddInput(input) { s.addInput(input) } else { print("cannot add input"); exit(9) }
        if s.canAddOutput(out) { s.addOutput(out) } else { print("cannot add output"); exit(10) }
        s.startRunning()
        self.session = s
        self.output = out

        // Startup info.
        syncClock = s.synchronizationClock
        let syncIsHost = syncClock != nil &&
            CFEqual(syncClock, CMClockGetHostTimeClock())
        let fmt = device.activeFormat
        let dims = CMVideoFormatDescriptionGetDimensions(fmt.formatDescription)
        let sub = CMFormatDescriptionGetMediaSubType(fmt.formatDescription)
        let ranges = fmt.videoSupportedFrameRateRanges.map {
            String(format: "%.2f-%.2f", $0.minFrameRate, $0.maxFrameRate)
        }
        info = [
            "label": args.label,
            "args": CommandLine.arguments.dropFirst().joined(separator: " "),
            "device": device.localizedName,
            "device_id": device.uniqueID,
            "format_w": Int(dims.width), "format_h": Int(dims.height),
            "fourcc": String(format: "%08x", sub),
            "fps_ranges": ranges,
            "min_dur_s": device.activeVideoMinFrameDuration.seconds,
            "max_dur_s": device.activeVideoMaxFrameDuration.seconds,
            "sync_is_host": syncIsHost,
            "discards_late": out.alwaysDiscardsLateVideoFrames,
            "fx_portrait_en": AVCaptureDevice.isPortraitEffectEnabled,
            "fx_studio_en": AVCaptureDevice.isStudioLightEnabled,
            "fx_center_en": AVCaptureDevice.isCenterStageEnabled,
            "fx_reactions_en": AVCaptureDevice.reactionEffectsEnabled,
            "fx_react_gestures_en": AVCaptureDevice.reactionEffectGesturesEnabled,
            "fx_bgrepl_en": AVCaptureDevice.isBackgroundReplacementEnabled,
            "fx_portrait_active": device.isPortraitEffectActive,
            "fx_studio_active": device.isStudioLightActive,
            "fx_center_active": device.isCenterStageActive,
            "fx_bgrepl_active": device.isBackgroundReplacementActive,
            "macos": ProcessInfo.processInfo.operatingSystemVersionString,
            "inject_ms": args.injectMs,
        ]
        loadStart = loadavg()
        info["load_start"] = loadStart
        print("startup: \(info)")

        if args.flash { flasher.setup() }

        // Process-wide spinner: busy-spins spinner-pct % of each 10 ms slice
        // on its own thread (not the capture queue).
        if args.spinnerPct > 0 {
            let pct = Double(args.spinnerPct) / 100.0
            let sliceNs = 10_000_000.0
            let sliceTicks = sliceNs * 1e-9 * machTicksPerSec
            Thread.detachNewThread {
                while !spinnerStop {
                    let t0 = mach_absolute_time()
                    while Double(mach_absolute_time() - t0) < sliceTicks * pct {}
                    Thread.sleep(forTimeInterval: (1 - pct) * 0.010)
                }
            }
        }

        // End after --seconds.
        DispatchQueue.global().asyncAfter(deadline: .now() + args.seconds) {
            self.finish()
        }
        app.run()
    }

    func finish() {
        if args.flash { flasher.shutdown() }
        spinnerStop = true
        session?.stopRunning()
        info["load_end"] = loadavg()
        info["frames"] = frameSeq
        info["dropped"] = droppedTotal
        let infoPath = "\(args.out)/\(ts)-\(args.label).info.json"
        if let d = try? JSONSerialization.data(
            withJSONObject: jsonSafe(info), options: .prettyPrinted) {
            try? d.write(to: URL(fileURLWithPath: infoPath))
        }
        print("done frames=\(frameSeq) dropped=\(droppedTotal) -> \(args.out)/\(ts)-\(args.label).jsonl")
        exit(0)
    }
}

@main
enum CamLatMain {
    static func main() {
        App().run()
    }
}
