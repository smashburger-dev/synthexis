import CoreMIDI
import Foundation

// synthexis FL bridge daemon: a CoreMIDI virtual source "synthexis bridge"
// (commands -> FL) and a virtual destination "synthexis bridge return"
// (replies <- FL), fronted by a unix socket at
// /tmp/synthexis-flbridge.sock speaking JSON lines.
// Wire framing both ways: F0 7D <ascii JSON> F7. Replies matched by "id",
// 2 s timeout -> {"id":n,"error":"timeout"}.
// Log: ~/Library/Logs/synthexis-flbridge.log

let SOCK = "/tmp/synthexis-flbridge.sock"
let LOG = NSHomeDirectory() + "/Library/Logs/synthexis-flbridge.log"
let MAGIC: UInt8 = 0x7D
let MAX_PAYLOAD = 4096

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

var replyBuf = Data()          // partial inbound sysex accumulation
var pendingLock = NSCondition()
var pending: [Int: Data] = [:] // id -> reply JSON bytes

var nextId = 1

func sendPackets(_ bytes: [UInt8]) {
    var list = MIDIPacketList(numPackets: 1, packet: MIDIPacket())
    withUnsafeMutablePointer(to: &list) { ptr in
        let pkt = MIDIPacketListInit(ptr)
        _ = MIDIPacketListAdd(ptr, MemoryLayout<MIDIPacketList>.size, pkt,
                              0, bytes.count, bytes)
    }
    MIDIReceived(source, &list)
}

// Iterate a MIDIPacketList's byte payloads.
func forEachPacket(_ list: UnsafePointer<MIDIPacketList>,
                   _ f: (UnsafePointer<UInt8>, Int) -> Void) {
    var pkt = list.pointee.packet
    for _ in 0..<list.pointee.numPackets {
        let len = Int(pkt.length)
        withUnsafePointer(to: &pkt.data) { d in
            d.withMemoryRebound(to: UInt8.self, capacity: len) { f($0, len) }
        }
        pkt = MIDIPacketNext(&pkt).pointee
    }
}

func feedInbound(_ bytes: UnsafePointer<UInt8>, _ count: Int) {
    replyBuf.append(bytes, count: count)
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
        let payload = replyBuf.subdata(in: 2..<end)
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
    log("sent id=\(rid) \(String(data: req, encoding: .utf8) ?? "")")
    pendingLock.lock()
    let deadline = Date().addingTimeInterval(2)
    var reply = pending.removeValue(forKey: rid)
    while reply == nil && Date() < deadline {
        pendingLock.wait(until: deadline)
        reply = pending.removeValue(forKey: rid)
    }
    pendingLock.unlock()
    if let r = reply {
        log("reply id=\(rid) \(String(data: r, encoding: .utf8) ?? "")")
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
    forEachPacket(packetList) { ptr, len in feedInbound(ptr, len) }
}
guard status == noErr else { log("dest create failed \(status)"); exit(1) }
log("flbridged up: source='synthexis bridge' dest='synthexis bridge return'")

Thread.detachNewThread(serve)
RunLoop.main.run()
