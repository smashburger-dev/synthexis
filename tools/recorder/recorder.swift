import AVFoundation
import Cocoa
import SwiftUI

// synthexis recorder: plays a recording protocol (segments with German
// instructions), records each selected camera to .mov, writes
// segments.json + session.json. New work for the Meta hackathon.

let APP_VERSION = "0.2.0"

let rlogFile: FileHandle? = {
    let p = NSHomeDirectory() + "/Library/Logs/synthexis-recorder.log"
    if !FileManager.default.fileExists(atPath: p) {
        FileManager.default.createFile(atPath: p, contents: nil)
    }
    let h = try? FileHandle(forWritingTo: URL(fileURLWithPath: p))
    h?.seekToEndOfFile()
    return h
}()

func rlog(_ s: String) {
    let line = String(format: "[%.3f] %@", Date().timeIntervalSince1970, s)
    print(line)
    if let d = (line + "\n").data(using: .utf8) { rlogFile?.write(d) }
}

// MARK: - Protocol model

struct RampLabel: Codable {
    let signal: String
    let shape: String
    let period_s: Double
    let start: String
}

struct Segment: Codable {
    let id: String
    let block: String
    let duration_s: Double
    let instruction_de: String
    let margin_s: [Double]
    let labels: [String: LabelValue]

    var ramp: RampLabel? {
        if case .ramp(let r) = labels["ramp"] { return r }
        return nil
    }
}

enum LabelValue: Codable {
    case string(String), strings([String]), ramp(RampLabel), other
    init(from d: Decoder) throws {
        let c = try d.singleValueContainer()
        if let s = try? c.decode(String.self) { self = .string(s); return }
        if let a = try? c.decode([String].self) { self = .strings(a); return }
        if let r = try? c.decode(RampLabel.self) { self = .ramp(r); return }
        self = .other
    }
    func encode(to e: Encoder) throws {
        var c = e.singleValueContainer()
        switch self {
        case .string(let s): try c.encode(s)
        case .strings(let a): try c.encode(a)
        case .ramp(let r): try c.encode(r)
        case .other: try c.encode("other")
        }
    }
    // segments.json representation; used by writeFiles and the --selftest
    // round-trip so both paths can never diverge again.
    var jsonValue: Any {
        switch self {
        case .string(let s): return s
        case .strings(let a): return a
        case .ramp(let rp): return ["signal": rp.signal, "shape": rp.shape,
                                    "period_s": rp.period_s, "start": rp.start]
        case .other: return "other"
        }
    }
}

struct RecProtocol: Codable {
    let name: String
    let gap_s: Double
    let segments: [Segment]
}

struct ProtocolFile: Codable {
    let protocols: [String: RecProtocol]
}

// MARK: - Camera capture

final class CameraRec: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let device: AVCaptureDevice
    let session = AVCaptureSession()
    let videoOut = AVCaptureVideoDataOutput()
    var configuredFps = 0
    var fpsNote = ""
    var writer: AVAssetWriter?
    var vIn: AVAssetWriterInput?
    var sessionStarted = false
    var firstPTS: CMTime?   // first appended frame; segment times are relative to this
    var framesWritten = 0
    var onError: ((String) -> Void)?

    init(device: AVCaptureDevice) { self.device = device }

    // Created once. A new preview layer per SwiftUI render re-wired the session
    // on every countdown tick, which stopped the movie output (-11806) and
    // starved the data output (3 frames per run).
    lazy var previewLayer: AVCaptureVideoPreviewLayer = {
        let l = AVCaptureVideoPreviewLayer(session: session)
        l.videoGravity = .resizeAspect
        return l
    }()
    var framesSkipped = 0
    var lastStatsLog = Date()

    func configure(log: (String) -> Void) throws {
        session.beginConfiguration()
        if session.canSetSessionPreset(.hd1280x720) { session.sessionPreset = .hd1280x720 }
        let input = try AVCaptureDeviceInput(device: device)
        if session.canAddInput(input) { session.addInput(input) }
        videoOut.alwaysDiscardsLateVideoFrames = false
        videoOut.setSampleBufferDelegate(self, queue: DispatchQueue(label: "rec." + device.localizedName))
        if session.canAddOutput(videoOut) { session.addOutput(videoOut) }
        videoOut.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
        let target = CMTime(value: 1, timescale: 30)
        var locked = false
        for r in device.activeFormat.videoSupportedFrameRateRanges {
            if target.seconds >= r.minFrameDuration.seconds - 1e-6,
               target.seconds <= r.maxFrameDuration.seconds + 1e-6 { locked = true }
        }
        try device.lockForConfiguration()
        if locked {
            device.activeVideoMinFrameDuration = target
            device.activeVideoMaxFrameDuration = target
            configuredFps = 30
        } else {
            fpsNote = "30 fps lock not supported by active format"
            log(fpsNote)
        }
        device.unlockForConfiguration()
        session.commitConfiguration()
        session.startRunning()
        rlog("cam \(device.localizedName) configured: isRunning=\(session.isRunning) fpsLock=\(configuredFps)")
    }

    func startRecording(to url: URL) {
        do {
            let w = try AVAssetWriter(url: url, fileType: .mov)
            let settings: [String: Any] = [
                AVVideoCodecKey: AVVideoCodecType.h264,
                AVVideoWidthKey: 1280, AVVideoHeightKey: 720]
            let vi = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
            vi.expectsMediaDataInRealTime = true
            vi.mediaTimeScale = 30
            if w.canAdd(vi) { w.add(vi) }
            writer = w; vIn = vi
            onError?("writer ready -> \(url.lastPathComponent)")
        } catch { onError?("writer init failed: \(error)") }
    }

    func stopRecording() {
        if let v = vIn { v.markAsFinished() }
        writer?.finishWriting {
            if let w = self.writer { rlog("writer done status=\(w.status.rawValue) err=\(String(describing: w.error))") }
            rlog("cam \(self.device.localizedName): final written=\(self.framesWritten) skipped=\(self.framesSkipped)")
            self.session.stopRunning()   // release the camera once the file is closed
        }
    }

    // segment times = PTS relative to first appended frame (frame-accurate)
    var lastPTS = CMTime.zero
    var curRecordedSeconds: Double {
        guard let f = firstPTS, lastPTS.isNumeric else { return 0 }
        return max(0, lastPTS.seconds - f.seconds)
    }

    func captureOutput(_ o: AVCaptureOutput, didOutput sb: CMSampleBuffer,
                       from c: AVCaptureConnection) {
        guard let w = writer, let vi = vIn else { return }
        let pts = CMSampleBufferGetPresentationTimeStamp(sb)
        if !sessionStarted {
            if w.startWriting() != true { onError?("startWriting failed: \(String(describing: w.error))"); return }
            w.startSession(atSourceTime: pts)
            sessionStarted = true
            firstPTS = pts
        }
        if w.status == .writing, vi.isReadyForMoreMediaData, vi.append(sb) {
            lastPTS = pts
            framesWritten += 1
        } else {
            framesSkipped += 1
        }
        if Date().timeIntervalSince(lastStatsLog) > 5 {
            lastStatsLog = Date()
            rlog("cam \(device.localizedName): written=\(framesWritten) skipped=\(framesSkipped) writer=\(w.status.rawValue)")
        }
    }

    func captureOutput(_ o: AVCaptureOutput, didDrop sb: CMSampleBuffer,
                       from c: AVCaptureConnection) {
        onError?("frame dropped")
    }
}

// MARK: - App state

enum Phase { case start, gap, segment, done }

struct SegRecord {
    var id = "", block = "", labels = [String: LabelValue]()
    var margin_s = [Double]()
    var bad = false
    var start_s = [Double](), end_s = [Double]()
    var hostStart = 0.0
}

final class Model: ObservableObject {
    @Published var protocolNames = ["full", "short"]
    @Published var protoSel = "full"
    @Published var sessionId = "S1"
    @Published var notes = ""
    @Published var devices: [AVCaptureDevice] = []
    @Published var cam0 = 0
    @Published var cam1 = -1  // -1 = none
    @Published var phase: Phase = .start
    @Published var segIdx = 0
    @Published var countdown = 0.0
    @Published var paused = false
    @Published var badFlagged = false
    @Published var authNote = ""
    @Published var endText = ""
    @Published var logLines: [String] = []

    var proto: RecProtocol?
    var protos: [String: RecProtocol] = [:]
    var cams: [CameraRec] = []
    var records: [SegRecord] = []
    var segEnd = 0.0
    var segStartHost = Date()
    var timer: Timer?
    var outDir: URL?

    var current: Segment? { proto?.segments[safe: segIdx] }
    var nextSeg: Segment? {
        if phase == .gap { return proto?.segments[safe: segIdx] }
        return proto?.segments[safe: segIdx + 1]
    }

    func log(_ s: String) {
        rlog(s)
        print("[recorder] \(s)")
        DispatchQueue.main.async { self.logLines.append(s) }
    }

    /// The app never touches the repo at run time: the repo lives on the Desktop,
    /// which macOS guards per app (TCC) and resets on every re-sign. The protocol
    /// ships inside the bundle and recordings go to ~/Movies (not TCC-guarded);
    /// data/recordings in the repo is a symlink to that folder (see build.sh).
    static let recordingsRoot = URL(fileURLWithPath: NSHomeDirectory())
        .appendingPathComponent("Movies/synthexis/recordings")

    func loadProtocol() {
        let args = CommandLine.arguments
        var url = Bundle.main.url(forResource: "recording-protocol", withExtension: "json")
        if let i = args.firstIndex(of: "--protocol-file"), i + 1 < args.count {
            url = URL(fileURLWithPath: args[i + 1])
        }
        if let i = args.firstIndex(of: "--session"), i + 1 < args.count { sessionId = args[i + 1] }
        guard let url, let data = FileManager.default.contents(atPath: url.path) else {
            log("Protokoll nicht lesbar: \(url?.path ?? "fehlt im App-Bundle, build.sh neu laufen lassen")")
            return
        }
        let dec = JSONDecoder()
        do {
            if let file = try? dec.decode(ProtocolFile.self, from: data) {
                protos = file.protocols
            } else {
                let p = try dec.decode(RecProtocol.self, from: data)
                protos = [p.name: p]
                protoSel = p.name
            }
        } catch {
            log("Protokoll ungültig (\(url.lastPathComponent)): \(error)")
            return
        }
        protocolNames = protos.keys.sorted()
        if protos[protoSel] == nil { protoSel = protocolNames[0] }
        proto = protos[protoSel]
        rlog("protocol loaded from \(url.path): " +
             protocolNames.map { "\($0)=\(protos[$0]!.segments.count)" }.joined(separator: " "))
    }

    func reloadProto() {
        proto = protos[protoSel]
    }

    func discoverCameras() {
        let ds = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInWideAngleCamera, .external, .continuityCamera],
            mediaType: .video, position: .unspecified)
        devices = ds.devices
        cam0 = devices.isEmpty ? -1 : 0
    }

    func startAll() {
        reloadProto()
        guard proto != nil else { authNote = "Kein Protokoll geladen (siehe Zeile darunter)"; return }
        guard cam0 >= 0, cam0 < devices.count else { authNote = "Keine Kamera gefunden"; return }
        AVCaptureDevice.requestAccess(for: .video) { granted in
            DispatchQueue.main.async {
                if !granted {
                    self.authNote = "Kamera verweigert: Systemeinstellungen → Datenschutz & Sicherheit → Kamera → synthexis recorder einschalten"
                    return
                }
                self.beginRecording()
            }
        }
    }

    func beginRecording() {
        var camsToUse: [CameraRec] = []
        for (i, d) in devices.enumerated() where i == cam0 || i == cam1 {
            let c = CameraRec(device: d)
            do { try c.configure(log: self.log) }
            catch { log("camera \(d.localizedName) configure failed: \(error)"); continue }
            c.onError = self.log
            camsToUse.append(c)
        }
        guard !camsToUse.isEmpty else { authNote = "no usable camera"; return }
        cams = camsToUse

        let date = DateFormatter()
        date.dateFormat = "yyyy-MM-dd"
        let base = Model.recordingsRoot
        let stem = "\(date.string(from: Date()))-\(sessionId)"
        var dir = base.appendingPathComponent(stem)
        var n = 1
        while FileManager.default.fileExists(atPath: dir.path) {
            n += 1
            dir = base.appendingPathComponent("\(stem)-\(n)")
        }
        do { try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true) }
        catch { log("cannot create \(dir.path): \(error)"); authNote = "Ordner nicht anlegbar"; return }
        outDir = dir
        // give sessions a moment to produce frames before startRecording,
        // else the file output may have no live connection and record 0 s
        self.log("outdir exists=\(FileManager.default.fileExists(atPath: dir.path))")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            for (i, c) in self.cams.enumerated() {
                let u = dir.appendingPathComponent("cam\(i).mov")
                self.log("startRecording cam\(i) -> \(u.path) sessionRunning=\(c.session.isRunning)")
                c.startRecording(to: u)
            }
            self.segIdx = 0
            self.records = []
            self.startGapThenSegment()
        }
        log("recording to \(dir.path)")
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.033, repeats: true) { _ in
            self.tick()
        }
    }

    func startGapThenSegment() {
        phase = .gap
        segEnd = (proto?.gap_s ?? 1.0)
        segStartHost = Date()
    }

    func startSegment() {
        phase = .segment
        badFlagged = false
        let s = current!
        var rec = SegRecord()
        rec.id = s.id; rec.block = s.block; rec.labels = s.labels
        rec.margin_s = s.margin_s
        rec.hostStart = Date().timeIntervalSince1970
        rec.start_s = cams.map { $0.curRecordedSeconds }
        records.append(rec)
        segEnd = s.duration_s
        segStartHost = Date()
    }

    func tick() {
        guard !paused, phase == .gap || phase == .segment else { return }
        let elapsed = Date().timeIntervalSince(segStartHost)
        countdown = max(0, segEnd - elapsed)
        if elapsed >= segEnd {
            if phase == .gap { startSegment() }
            else { endSegment() }
        }
    }

    func endSegment() {
        var rec = records.removeLast()
        rec.end_s = cams.map { $0.curRecordedSeconds }
        rec.bad = badFlagged
        records.append(rec)
        segIdx += 1
        if let p = proto, segIdx < p.segments.count { startGapThenSegment() }
        else { finish() }
    }

    func togglePause() {
        paused.toggle()
        if !paused { segStartHost = Date() - (segEnd - countdown) }
    }

    func markBad() { if phase == .segment { badFlagged = true } }

    func finish() {
        timer?.invalidate()
        for c in cams { c.stopRecording() }
        writeFiles()
        phase = .done
    }

    func writeFiles() {
        guard let dir = outDir, let p = proto else { return }
        // wait briefly for the last frames to flush
        Thread.sleep(forTimeInterval: 0.5)
        var segs = [[String: Any]](); records.forEach { r in
            var labels = [String: Any]()
            for (k, v) in r.labels {
                labels[k] = v.jsonValue
            }
            var d: [String: Any] = ["id": r.id, "block": r.block, "labels": labels,
                                    "margin_s": r.margin_s, "bad": r.bad,
                                    "host_start": r.hostStart]
            for (i, v) in r.start_s.enumerated() { d["start_s_cam\(i)"] = v }
            for (i, v) in r.end_s.enumerated() { d["end_s_cam\(i)"] = v }
            segs.append(d)
        }
        writeJSON(segs, to: dir.appendingPathComponent("segments.json"))

        var camInfo: [[String: Any]] = []
        var sizes: [String] = []
        var endLines: [String] = []
        for (i, c) in cams.enumerated() {
            let u = dir.appendingPathComponent("cam\(i).mov")
            var measured = 0.0, dur = 0.0, frames = 0
            let a = AVAsset(url: u)
            if let t = a.tracks(withMediaType: .video).first {
                measured = Double(t.nominalFrameRate)
                dur = a.duration.seconds
            }
            frames = c.framesWritten
            if measured == 0 && dur > 0 { measured = Double(frames) / dur }
            let size = (try? FileManager.default.attributesOfItem(atPath: u.path))?[.size] as? Int ?? 0
            sizes.append("\(size/1_048_576) MiB")
            camInfo.append(["name": c.device.localizedName, "uniqueID": c.device.uniqueID,
                            "model": c.device.modelID, "resolution": "1280x720",
                            "configured_fps": c.configuredFps,
                            "fps_note": c.fpsNote, "measured_fps": measured,
                            "duration_s": dur, "size_bytes": size])
            endLines.append(String(format: "cam%d %@: %.1f fps, %@, %d frames", i,
                                   c.device.localizedName, measured, sizes[i], frames))
        }
        let session: [String: Any] = [
            "session": sessionId, "protocol": p.name, "app_version": APP_VERSION,
            "notes": notes, "start_time": ISO8601DateFormatter().string(from: Date()),
            "cameras": camInfo]
        writeJSON(session, to: dir.appendingPathComponent("session.json"))
        let nBad = records.filter { $0.bad }.count
        endText = "\(records.count) Segmente, \(nBad) als schlecht markiert\n" +
                  endLines.joined(separator: "\n") + "\n\(dir.path)"
    }

    func writeJSON(_ obj: Any, to url: URL) {
        if let d = try? JSONSerialization.data(withJSONObject: obj,
                                               options: [.prettyPrinted, .sortedKeys]) {
            try? d.write(to: url)
        }
    }
}

extension Array { subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil } }

// MARK: - Preview

struct Preview: NSViewRepresentable {
    let layer: AVCaptureVideoPreviewLayer
    func makeNSView(context: Context) -> NSView {
        let v = NSView()
        v.wantsLayer = true
        v.layer = layer
        layer.frame = CGRect(x: 0, y: 0, width: 640, height: 360)
        // mirror the preview only (recording stays unmirrored)
        layer.connection?.automaticallyAdjustsVideoMirroring = false
        layer.connection?.isVideoMirrored = true
        return v
    }
    func updateNSView(_ v: NSView, context: Context) {}
}

// MARK: - UI

struct ContentView: View {
    @ObservedObject var m: Model
    var body: some View {
        Group {
            switch m.phase {
            case .start: startView
            case .done: doneView
            default: recView
            }
        }
        .frame(minWidth: 800, minHeight: 600)
    }

    var startView: some View {
        Form {
            Text("synthexis recorder").font(.largeTitle)
            Picker("Protokoll", selection: $m.protoSel) {
                ForEach(m.protocolNames, id: \.self) { Text($0) }
            }
            TextField("Session (S1–S4 oder frei)", text: $m.sessionId)
            Picker("Kamera 0", selection: $m.cam0) {
                ForEach(Array(m.devices.enumerated()), id: \.offset) {
                    Text($0.element.localizedName).tag($0.offset)
                }
            }
            Picker("Kamera 1 (optional)", selection: $m.cam1) {
                Text("keine").tag(-1)
                ForEach(Array(m.devices.enumerated()), id: \.offset) {
                    Text($0.element.localizedName).tag($0.offset)
                }
            }
            TextField("Licht / Notizen", text: $m.notes)
            if let p = m.proto {
                Text("\(p.segments.count) Segmente, gap \(p.gap_s)s").foregroundColor(.secondary)
            }
            Button("Start") { m.startAll() }.keyboardShortcut(.defaultAction)
            Text(m.authNote).foregroundColor(.red)
            ForEach(m.logLines.suffix(8), id: \.self) { Text($0).font(.caption) }
        }.padding(30)
    }

    var recView: some View {
        VStack(spacing: 8) {
            HStack {
                if let c = m.cams.first { Preview(layer: c.previewLayer)
                    .frame(width: 320, height: 180) }
                VStack(alignment: .leading) {
                    if m.phase == .segment, let s = m.current {
                        Text("Block \(s.block)  •  \(m.segIdx+1)/\(m.proto!.segments.count)")
                            .font(.headline)
                        Text(s.instruction_de)
                            .font(.system(size: 52, weight: .bold))
                        if s.ramp != nil { RampPulse(period: s.ramp!.period_s,
                                                   startClosed: s.ramp!.start == "closed",
                                                   start: m.segStartHost) }
                    } else {
                        Text("Gleich:").font(.title)
                        if let n = m.nextSeg {
                            Text(n.instruction_de).font(.system(size: 52, weight: .bold))
                        }
                    }
                    if m.paused { Text("PAUSE").font(.title).foregroundColor(.yellow) }
                    if m.badFlagged { Text("marked bad").foregroundColor(.red) }
                }.padding()
            }
            if m.phase == .segment {
                ProgressView(value: m.current!.duration_s - m.countdown,
                             total: m.current!.duration_s)
                    .progressViewStyle(.linear).padding(.horizontal)
                Text(String(format: "%.1f s", m.countdown)).font(.title2)
                if let n = m.nextSeg {
                    Text("Nächstes: \(n.instruction_de)").font(.body)
                        .foregroundColor(.secondary).lineLimit(2)
                }
            }
            Text("B = schlecht markieren   Leertaste = Pause   Esc = Stopp")
                .font(.caption).foregroundColor(.secondary)
        }
    }

    var doneView: some View {
        VStack(spacing: 12) {
            Text("Aufnahme fertig").font(.largeTitle)
            Text(m.endText).multilineTextAlignment(.center)
            Button("Beenden") { NSApp.terminate(nil) }
        }.padding(30)
    }
}

struct RampPulse: View {
    let period: Double
    let startClosed: Bool
    let start: Date   // segment start: the label's triangle starts "closed" here
    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.05)) { ctx in
            let ph = max(0, ctx.date.timeIntervalSince(start)).truncatingRemainder(
                dividingBy: period) / period
            let tri = ph < 0.5 ? ph * 2 : 2 - ph * 2   // 0..1..0 triangle
            HStack {
                Circle()
                    .fill(Color.green.opacity(0.25 + 0.75 * tri))
                    .frame(width: 40, height: 40)
                    .overlay(Text("△").font(.title))
                Text(String(format: "Rampen-Takt %.1fs  (oben=%@)", period,
                            startClosed ? "offen" : "zu"))
                    .font(.caption)
            }
        }
    }
}

@main
enum Launcher {
    static func main() {
        guard CommandLine.arguments.contains("--selftest") else { RecorderApp.main(); return }
        // Headless check run by build.sh: protocol loads from the bundle and the
        // recordings folder is writable. Exit code 0 = ready to record.
        let m = Model()
        m.loadProtocol()
        let fm = FileManager.default
        let probe = Model.recordingsRoot.appendingPathComponent(".selftest")
        let writable = (try? fm.createDirectory(at: Model.recordingsRoot,
                                                withIntermediateDirectories: true)) != nil
            && fm.createFile(atPath: probe.path, contents: Data())
        try? fm.removeItem(at: probe)
        let summary = m.protocolNames.map { "\($0)=\(m.protos[$0]!.segments.count)" }
        print("selftest protocols: \(summary.joined(separator: " ")) recordings: " +
              "\(Model.recordingsRoot.path) writable=\(writable)")
        // Round-trip one block-E segment's labels through the writeFiles
        // serialization: objects must come back as a JSON array, not the
        // "other" string that broke the label pipeline on S1.
        var objectsOK = false
        if let e = m.protos["full"]?.segments.first(where: { $0.block == "E" }) {
            var labels = [String: Any]()
            for (k, v) in e.labels { labels[k] = v.jsonValue }
            objectsOK = labels["objects"] is [String]
            print("selftest E-segment \(e.id): labels.objects -> " +
                  "\(String(describing: labels["objects"])) isArray=\(objectsOK)")
        } else {
            print("selftest E-segment: none found in protocol 'full'")
        }
        exit(m.protos["full"] != nil && m.protos["short"] != nil && writable && objectsOK ? 0 : 1)
    }
}

struct RecorderApp: App {
    @StateObject private var m = Model()
    var body: some Scene {
        WindowGroup("synthexis recorder") {
            ContentView(m: m).onAppear { setup() }
        }
        .windowStyle(.hiddenTitleBar)
    }
    func setup() {
        m.loadProtocol(); m.discoverCameras()
        if CommandLine.arguments.contains("--auto") { m.startAll() }
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            // Shortcuts only while recording, so text fields keep space and "b".
            guard self.m.phase == .gap || self.m.phase == .segment else { return e }
            if e.keyCode == 53 { self.m.finish(); return nil }        // esc
            if e.charactersIgnoringModifiers == " " { self.m.togglePause(); return nil }
            if e.charactersIgnoringModifiers?.lowercased() == "b" { self.m.markBad(); return nil }
            return e
        }
    }
}
