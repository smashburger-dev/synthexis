import CoreMIDI
import Foundation

// synthexis FL bridge daemon: a CoreMIDI virtual source "synthexis bridge"
// (commands -> FL) and a virtual destination "synthexis bridge return"
// (replies <- FL), fronted by a unix socket at
// /tmp/synthexis-flbridge.sock speaking JSON lines.
// Wire framing both ways: F0 7D <ascii JSON> F7. Replies matched by "id",
// timeout -> {"id":n,"error":"timeout"}.
// Log: ~/Library/Logs/synthexis-flbridge.log
//
// Target-picker variant of probes/fl-camera-plugin/bridge/flbridged.swift:
// same port names and unique IDs (FL binds it like the original), payload
// limit FLB_MAX bytes (default 1 MiB, the original has 4096), timeout
// FLB_TIMEOUT seconds (default 5, original 2), request frames of any size
// (the original's single MIDIPacketList held about 256 bytes), log lines
// cut to 300 characters.

let SOCK = "/tmp/synthexis-flbridge.sock"
let LOG = NSHomeDirectory() + "/Library/Logs/synthexis-flbridge.log"
let MAGIC: UInt8 = 0x7D
let ENV = ProcessInfo.processInfo.environment
let MAX_PAYLOAD = Int(ENV["FLB_MAX"] ?? "") ?? (1 << 20)
let TIMEOUT = Double(ENV["FLB_TIMEOUT"] ?? "") ?? 5.0
let DEBUG = ENV["FLB_DEBUG"] != nil  // log every inbound packet's length

func clip(_ d: Data) -> String {
    let s = String(data: d, encoding: .utf8) ?? ""
    return s.count > 300 ? String(s.prefix(300)) + "...(\(d.count) bytes)" : s
}

let logFH: FileHandle = {
    FileManager.default.createFile(atPath: LOG, contents: nil)
    return try! FileHandle(forWritingTo: URL(fileURLWithPath: LOG))
}()
func log(_ s: String) {
    let line = "[\(Date().timeIntervalSince1970)] \(s)\n"
    logFH.write(line.data(using: .utf8)!)
}

// --- SysEx codec (mirrors bridge/sysexcodec.py) ---
func encodeSysex(_ jsonData: Data) throws -> [UInt8] {
    guard jsonData.count <= MAX_PAYLOAD else {
        throw NSError(domain: "flb", code: 1,
                      userInfo: [NSLocalizedDescriptionKey: "payload too large"])
    }
    for b in jsonData where b > 0x7F {
        throw NSError(domain: "flb", code: 2,
                      userInfo: [NSLocalizedDescriptionKey: "non-7-bit byte"])
    }
    return [0xF0, MAGIC] + [UInt8](jsonData) + [0xF7]
}

// MIDI state
var client = MIDIClientRef()
var source = MIDIEndpointRef()
var dest = MIDIEndpointRef()

var replyBuf: [UInt8] = []     // partial inbound sysex accumulation
var pendingLock = NSCondition()
var pending: [Int: Data] = [:] // id -> reply JSON bytes

var nextId = 1

func sendPackets(_ bytes: [UInt8]) {
    // One packet holding the whole frame, in a buffer sized for it.
    let size = bytes.count + 1024
    let raw = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: 8)
    defer { raw.deallocate() }
    let ptr = raw.bindMemory(to: MIDIPacketList.self, capacity: 1)
    let pkt = MIDIPacketListInit(ptr)
    _ = MIDIPacketListAdd(ptr, size, pkt, 0, bytes.count, bytes)
    MIDIReceived(source, ptr)
}

// Iterate a MIDIPacketList's byte payloads.
func forEachPacket(_ list: UnsafePointer<MIDIPacketList>,
                   _ f: (UnsafePointer<UInt8>, Int) -> Void) {
    // Walk the list in place: copying a MIDIPacket by value truncates its
    // payload to the 256-byte tuple, which cut long SysEx replies.
    let dataOffset = MemoryLayout<MIDIPacket>.offset(of: \MIDIPacket.data) ?? 10
    for pkt in list.unsafeSequence() {
        let len = Int(pkt.pointee.length)
        let bytes = UnsafeRawPointer(pkt).advanced(by: dataOffset)
            .assumingMemoryBound(to: UInt8.self)
        f(bytes, len)
    }
}

func feedInbound(_ bytes: UnsafePointer<UInt8>, _ count: Int) {
    // Plain [UInt8] with 0-based indices; Data slices keep their old indices
    // after removeFirst, which trapped on multi-packet replies.
    replyBuf.append(contentsOf: UnsafeBufferPointer(start: bytes, count: count))
    while true {
        guard let start = replyBuf.firstIndex(of: 0xF0) else {
            replyBuf.removeAll(keepingCapacity: true); return
        }
        if start > 0 { replyBuf.removeFirst(start) }
        if replyBuf.count < 2 { return }
        if replyBuf[1] != MAGIC { replyBuf.removeFirst(); continue }
        guard let end = replyBuf.firstIndex(of: 0xF7) else {
            if replyBuf.count > MAX_PAYLOAD + 4 { replyBuf.removeAll() }
            return
        }
        let payload = Data(replyBuf[2..<end])
        replyBuf.removeFirst(end + 1)
        if let obj = try? JSONSerialization.jsonObject(with: payload)
            as? [String: Any], let rid = obj["id"] as? Int {
            pendingLock.lock()
            pending[rid] = payload
            pendingLock.signal()
            pendingLock.unlock()
        }
    }
}

func handleRequest(_ line: Data) -> Data {
    guard var obj = try? JSONSerialization.jsonObject(with: line)
            as? [String: Any] else {
        return Data("{\"error\":\"bad json\"}\n".utf8)
    }
    pendingLock.lock()
    let rid = obj["id"] as? Int ?? nextId
    nextId = max(nextId, rid + 1)
    obj["id"] = rid
    pendingLock.unlock()
    guard let req = try? JSONSerialization.data(withJSONObject: obj),
          let frame = try? encodeSysex(req) else {
        return Data("{\"id\":\(rid),\"error\":\"encode failed\"}\n".utf8)
    }
    sendPackets(frame)
    log("sent id=\(rid) \(clip(req))")
    pendingLock.lock()
    let deadline = Date().addingTimeInterval(TIMEOUT)
    var reply = pending.removeValue(forKey: rid)
    while reply == nil && Date() < deadline {
        pendingLock.wait(until: deadline)
        reply = pending.removeValue(forKey: rid)
    }
    pendingLock.unlock()
    if let r = reply {
        log("reply id=\(rid) \(clip(r))")
        return r + Data("\n".utf8)
    }
    log("timeout id=\(rid)")
    return Data("{\"id\":\(rid),\"error\":\"timeout\"}\n".utf8)
}

func serve() {
    unlink(SOCK)
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else { log("socket() failed"); exit(1) }
    var addr = sockaddr_un()
    addr.sun_family = sa_family_t(AF_UNIX)
    _ = withUnsafeMutablePointer(to: &addr.sun_path) {
        $0.withMemoryRebound(to: CChar.self, capacity: 104) { dst in
            SOCK.withCString { strncpy(dst, $0, 104) }
        }
    }
    let len = socklen_t(MemoryLayout<sockaddr_un>.size)
    let bOK = withUnsafePointer(to: &addr) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            bind(fd, $0, len) == 0
        }
    }
    guard bOK, listen(fd, 4) == 0 else { log("bind/listen failed"); exit(1) }
    log("listening on \(SOCK)")
    while true {
        let cfd = accept(fd, nil, nil)
        if cfd < 0 { continue }
        var buf = [UInt8](repeating: 0, count: 65536)
        var acc = Data()
        while true {
            let n = recv(cfd, &buf, buf.count, 0)
            if n <= 0 { break }
            acc.append(buf, count: n)
            while let nl = acc.firstIndex(of: 0x0A) {
                let line = acc.subdata(in: 0..<nl)
                acc.removeFirst(nl + 1)
                let reply = handleRequest(line)
                _ = reply.withUnsafeBytes { send(cfd, $0.baseAddress, reply.count, 0) }
            }
        }
        close(cfd)
    }
}

var status = MIDIClientCreateWithBlock("synthexis-flbridge" as CFString, &client) { _ in }
guard status == noErr else { log("client create failed \(status)"); exit(1) }
status = MIDISourceCreate(client, "synthexis bridge" as CFString, &source)
guard status == noErr else { log("source create failed \(status)"); exit(1) }
status = MIDIDestinationCreateWithBlock(
    client, "synthexis bridge return" as CFString, &dest
) { packetList, _ in
    forEachPacket(packetList) { ptr, len in
        if DEBUG {
            let head = (0..<min(len, 4)).map { String(format: "%02X", ptr[$0]) }
            log("in packet len=\(len) head=\(head.joined(separator: " "))")
        }
        feedInbound(ptr, len)
    }
}
guard status == noErr else { log("dest create failed \(status)"); exit(1) }
// Fixed unique IDs so hosts can re-identify the ports after a daemon restart.
MIDIObjectSetIntegerProperty(source, kMIDIPropertyUniqueID, 0x5359_4E01)
MIDIObjectSetIntegerProperty(dest, kMIDIPropertyUniqueID, 0x5359_4E02)
log("flbridged up: source='synthexis bridge' dest='synthexis bridge return' max=\(MAX_PAYLOAD) timeout=\(TIMEOUT)")

Thread.detachNewThread(serve)
RunLoop.main.run()
