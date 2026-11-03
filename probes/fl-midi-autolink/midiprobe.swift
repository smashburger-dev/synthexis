import CoreMIDI
import Foundation

// Opens a virtual source + destination like the plugin does and logs what FL
// sends. Optional: answer a Universal Device Inquiry (F0 7E <dev> 06 01 F7)
// with an identity reply, and ping a device script once a second.
//
//   midiprobe --src NAME --dst NAME [--uid HEX] [--id "7D 01 00 01 00 00 00 00 00"]
//             [--ping] [--seconds N]

var srcName = "synthexis probe", dstName = "synthexis probe return"
var uid: Int32? = nil
var identity: [UInt8]? = nil
var ping = false
var seconds = 60.0
var it = CommandLine.arguments.dropFirst().makeIterator()
while let a = it.next() {
    switch a {
    case "--src": srcName = it.next()!
    case "--dst": dstName = it.next()!
    case "--uid": uid = Int32(it.next()!, radix: 16)
    case "--id": identity = it.next()!.split(separator: " ").map { UInt8($0, radix: 16)! }
    case "--ping": ping = true
    case "--seconds": seconds = Double(it.next()!)!
    default: fatalError("unknown arg \(a)")
    }
}

let t0 = Date()
func log(_ s: String) {
    print(String(format: "%7.3f ", Date().timeIntervalSince(t0)) + s)
    fflush(stdout)
}
func hex(_ b: [UInt8]) -> String { b.map { String(format: "%02X", $0) }.joined(separator: " ") }

var client = MIDIClientRef()
MIDIClientCreateWithBlock("midiprobe" as CFString, &client) { n in
    log("notify \(n.pointee.messageID.rawValue)")
}
var source = MIDIEndpointRef()
guard MIDISourceCreate(client, srcName as CFString, &source) == noErr else { fatalError("source") }

func send(_ bytes: [UInt8]) {
    var list = MIDIPacketList()
    let p = MIDIPacketListInit(&list)
    bytes.withUnsafeBufferPointer { b in
        _ = MIDIPacketListAdd(&list, MemoryLayout<MIDIPacketList>.size, p, 0, bytes.count, b.baseAddress!)
    }
    MIDIReceived(source, &list)
}

var buf: [UInt8] = []
let lock = NSLock()
var dest = MIDIEndpointRef()
let st = MIDIDestinationCreateWithBlock(client, dstName as CFString, &dest) { list, _ in
    var p = list.pointee.packet
    for _ in 0..<list.pointee.numPackets {
        let bytes = withUnsafeBytes(of: p.data) { Array($0.prefix(Int(p.length))) }
        lock.lock()
        buf += bytes
        // whole SysEx frames, plus any short message on its own
        while let s = buf.firstIndex(of: 0xF0) {
            if s > 0 { log("in  short \(hex(Array(buf[..<s])))"); buf.removeFirst(s) }
            guard let e = buf.firstIndex(of: 0xF7) else { break }
            let f = Array(buf[...e]); buf.removeFirst(e + 1)
            if f.count >= 6 && f[1] == 0x7E && f[3] == 0x06 && f[4] == 0x01 {
                log("in  DEVICE INQUIRY \(hex(f))")
                if let id = identity {
                    let reply: [UInt8] = [0xF0, 0x7E, 0x7F, 0x06, 0x02] + id + [0xF7]
                    send(reply)
                    log("out identity \(hex(reply))")
                }
            } else if f.count > 3 && f[1] == 0x7D {
                log("in  json \(String(decoding: f[2..<(f.count - 1)], as: UTF8.self))")
            } else {
                log("in  sysex \(hex(f))")
            }
        }
        if !buf.isEmpty && buf.first != 0xF0 { log("in  short \(hex(buf))"); buf.removeAll() }
        lock.unlock()
        p = MIDIPacketNext(&p).pointee
    }
}
guard st == noErr else { fatalError("destination \(st)") }
if let u = uid {
    MIDIObjectSetIntegerProperty(source, kMIDIPropertyUniqueID, u)
    MIDIObjectSetIntegerProperty(dest, kMIDIPropertyUniqueID, u + 1)
}
log("open src=\"\(srcName)\" dst=\"\(dstName)\" identity=\(identity.map(hex) ?? "none")")

var n = 0
let end = Date().addingTimeInterval(seconds)
while Date() < end {
    RunLoop.main.run(until: Date().addingTimeInterval(1))
    if ping {
        n += 1
        let msg = Array("{\"cmd\":\"ping\",\"id\":\(n)}".utf8)
        send([0xF0, 0x7D] + msg + [0xF7])
    }
}
log("done")
