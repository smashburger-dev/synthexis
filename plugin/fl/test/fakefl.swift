import CoreMIDI
import Foundation

// Fake FL for the bridge test without FL: connects to the plugin's virtual
// ports (source and destination both named "synthexis")
// and carries SysEx frames to and from fake_fl.py, which runs the real
// device script. Exits when fake_fl.py exits.
//
//   xcrun swiftc -O plugin/fl/test/fakefl.swift -o /tmp/fakefl
//   SYNTHEXIS_FL_PORTS="synthexis test" target/release/synthexis &
//   /tmp/fakefl plugin/fl/test/fake_fl.py "synthexis test"

let args = CommandLine.arguments
let script = args.count > 1 ? args[1] : "fake_fl.py"
let port = args.count > 2 ? args[2] : "synthexis"

func endpoint(_ name: String, sources: Bool) -> MIDIEndpointRef? {
    let n = sources ? MIDIGetNumberOfSources() : MIDIGetNumberOfDestinations()
    for i in 0..<n {
        let e = sources ? MIDIGetSource(i) : MIDIGetDestination(i)
        var cf: Unmanaged<CFString>?
        if MIDIObjectGetStringProperty(e, kMIDIPropertyName, &cf) == noErr,
           let s = cf?.takeRetainedValue() as String?, s == name {
            return e
        }
    }
    return nil
}

// the client first, and a running run loop, or the endpoint list stays stale
var client = MIDIClientRef()
MIDIClientCreateWithBlock("fakefl" as CFString, &client) { _ in }
var src: MIDIEndpointRef? = nil
var dst: MIDIEndpointRef? = nil
for _ in 0..<100 {
    src = endpoint(port, sources: true)
    dst = endpoint(port, sources: false)
    if src != nil && dst != nil { break }
    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
}
guard let source = src, let dest = dst else {
    FileHandle.standardError.write("fakefl: plugin ports not found\n".data(using: .utf8)!)
    exit(1)
}

let py = Process()
py.executableURL = URL(fileURLWithPath: "/usr/bin/env")
py.arguments = ["python3", script]
let toPy = Pipe(), fromPy = Pipe()
py.standardInput = toPy
py.standardOutput = fromPy
try py.run()

var outPort = MIDIPortRef()
MIDIOutputPortCreate(client, "fakefl out" as CFString, &outPort)
var inPort = MIDIPortRef()
var buf: [UInt8] = []
let lock = NSLock()
MIDIInputPortCreateWithBlock(client, "fakefl in" as CFString, &inPort) { list, _ in
    let off = MemoryLayout<MIDIPacket>.offset(of: \MIDIPacket.data) ?? 10
    for pkt in list.unsafeSequence() {
        let p = UnsafeRawPointer(pkt).advanced(by: off).assumingMemoryBound(to: UInt8.self)
        lock.lock()
        buf.append(contentsOf: UnsafeBufferPointer(start: p, count: Int(pkt.pointee.length)))
        while let s = buf.firstIndex(of: 0xF0), let e = buf[s...].firstIndex(of: 0xF7) {
            let hex = buf[s...e].map { String(format: "%02x", $0) }.joined() + "\n"
            toPy.fileHandleForWriting.write(hex.data(using: .utf8)!)
            buf.removeFirst(e + 1)
        }
        lock.unlock()
    }
}
MIDIPortConnectSource(inPort, source, nil)

func send(_ bytes: [UInt8]) {
    let size = bytes.count + 1024
    let raw = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: 8)
    defer { raw.deallocate() }
    let list = raw.bindMemory(to: MIDIPacketList.self, capacity: 1)
    let pkt = MIDIPacketListInit(list)
    _ = MIDIPacketListAdd(list, size, pkt, 0, bytes.count, bytes)
    MIDISend(outPort, dest, list)
}

Thread.detachNewThread {
    var acc = Data()
    while true {
        let d = fromPy.fileHandleForReading.availableData
        if d.isEmpty { exit(0) }
        acc.append(d)
        while let nl = acc.firstIndex(of: 0x0A) {
            let line = String(data: acc.subdata(in: acc.startIndex..<nl), encoding: .ascii) ?? ""
            acc.removeSubrange(acc.startIndex...nl)
            var bytes: [UInt8] = []
            var i = line.startIndex
            while let j = line.index(i, offsetBy: 2, limitedBy: line.endIndex), i < line.endIndex {
                if let b = UInt8(line[i..<j], radix: 16) { bytes.append(b) }
                i = j
            }
            send(bytes)
        }
    }
}
RunLoop.main.run()
