import CoreMIDI
import Foundation

// noteprobe: opens the "synthexis" source and destination like the plugin
// (synthexis must not be loaded anywhere), answers FL's device inquiry with
// the plugin's identity, and runs the note tests of RESULT.md on commands
// read line by line from a FIFO. Logs to stdout.
//
//   noteprobe --fifo PATH [--name synthexis]
//
// Commands:
//   ping | chans | held | level C | sel C | echo on|off | offvel V
//   map                      clear the slot map;  map S C [S C ...]  slot -> channel
//   rec | norec | play | stop
//   seq SLOT NOTE N STEP_MS LEN_MS      plain notes (N2 when SLOT unmapped, N3a when mapped)
//   sx CH NOTE N STEP_MS LEN_MS         note commands (N3b)
//   ts SLOT NOTE                        four notes 125 ms apart, one packet list, future stamps (N4)
//   load SLOT SECONDS                   30 note-ons/s, offs 20 ms later, ping every 100 ms
//   stats                               delivery and ping round trips so far, then reset
//   quit

var fifo = "", name = "synthexis"
var it = CommandLine.arguments.dropFirst().makeIterator()
while let a = it.next() {
    switch a {
    case "--fifo": fifo = it.next() ?? ""
    case "--name": name = it.next() ?? name
    default: print("unknown arg \(a)"); exit(2)
    }
}
let identity: [UInt8] = [0x7D, 0x53, 0x58, 0x46, 0x4C, 0x05, 0, 0, 0]

var tb = mach_timebase_info_data_t()
mach_timebase_info(&tb)
func nowNs() -> UInt64 { mach_absolute_time() * UInt64(tb.numer) / UInt64(tb.denom) }
func ticks(ns: UInt64) -> UInt64 { ns * UInt64(tb.denom) / UInt64(tb.numer) }
let t0 = nowNs()
let logLock = NSLock()
func log(_ s: String) {
    logLock.lock()
    print(String(format: "%8.3f ", Double(nowNs() - t0) / 1e9) + s)
    fflush(stdout)
    logLock.unlock()
}

var client = MIDIClientRef()
MIDIClientCreateWithBlock("noteprobe" as CFString, &client) { _ in }
var source = MIDIEndpointRef()
guard MIDISourceCreate(client, name as CFString, &source) == noErr else { log("source failed"); exit(1) }

// packets: (timestamp ticks, bytes); timestamp 0 = now
func sendPackets(_ packets: [(UInt64, [UInt8])]) {
    let size = 1024 + packets.reduce(0) { $0 + $1.1.count + 16 }
    let raw = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: 8)
    defer { raw.deallocate() }
    let list = raw.bindMemory(to: MIDIPacketList.self, capacity: 1)
    var p = MIDIPacketListInit(list)
    for (ts, bytes) in packets {
        bytes.withUnsafeBufferPointer { b in
            p = MIDIPacketListAdd(list, size, p, ts, bytes.count, b.baseAddress!)
        }
    }
    MIDIReceived(source, list)
}
func send(_ bytes: [UInt8]) { sendPackets([(0, bytes)]) }

// round-trip bookkeeping
let stLock = NSLock()
var waitingSeen: [String: [UInt64]] = [:]   // "status:note" -> send times
var waitingId: [Int: (UInt64, String)] = [:]
var rtt: [String: [Double]] = [:]
var nextId = 1
var lastReply: [String: Any] = [:]
let replied = DispatchSemaphore(value: 0)
var waitFor = -1

func record(_ kind: String, _ ms: Double) { rtt[kind, default: []].append(ms) }

func json(_ obj: [String: Any]) -> [UInt8] {
    let d = (try? JSONSerialization.data(withJSONObject: obj)) ?? Data()
    return [0xF0, 0x7D] + [UInt8](d) + [0xF7]
}
@discardableResult
func call(_ obj: [String: Any], kind: String = "cmd", wait: Bool = true) -> [String: Any]? {
    stLock.lock()
    let id = nextId; nextId += 1
    var o = obj; o["id"] = id
    waitingId[id] = (nowNs(), kind)
    if wait { waitFor = id }
    stLock.unlock()
    send(json(o))
    guard wait else { return nil }
    if replied.wait(timeout: .now() + 2) == .timedOut { log("timeout \(obj)"); return nil }
    stLock.lock(); defer { stLock.unlock() }
    return lastReply
}

func onFrame(_ f: [UInt8]) {
    if f.count >= 6 && f[1] == 0x7E && f[3] == 0x06 && f[4] == 0x01 {
        send([0xF0, 0x7E, 0x7F, 0x06, 0x02] + identity + [0xF7])
        log("device inquiry answered")
        return
    }
    guard f.count > 3, f[1] == 0x7D,
          let v = try? JSONSerialization.jsonObject(with: Data(f[2..<(f.count - 1)])) as? [String: Any] else { return }
    let t = nowNs()
    stLock.lock(); defer { stLock.unlock() }
    if let ev = v["ev"] as? String {
        if ev == "seen", let s = v["s"] as? Int, let n = v["n"] as? Int {
            let k = "\(s & 0xF0 == 0x90 && (v["v"] as? Int ?? 0) > 0 ? "on" : "off"):\(s & 0x0F):\(n)"
            if var q = waitingSeen[k], !q.isEmpty {
                let sent = q.removeFirst(); waitingSeen[k] = q
                record("seen", Double(t - sent) / 1e6)
            }
        } else {
            log("event \(v)")
        }
        return
    }
    if let id = v["id"] as? Int, let (sent, kind) = waitingId.removeValue(forKey: id) {
        record(kind, Double(t - sent) / 1e6)
        if v["error"] != nil { log("error reply \(v)") }
        if id == waitFor { lastReply = v; waitFor = -1; replied.signal() }
    }
}

var buf: [UInt8] = []
var dest = MIDIEndpointRef()
let st = MIDIDestinationCreateWithBlock(client, name as CFString, &dest) { list, _ in
    var p = list.pointee.packet
    for _ in 0..<list.pointee.numPackets {
        let bytes = withUnsafeBytes(of: p.data) { Array($0.prefix(Int(p.length))) }
        buf += bytes
        while let s = buf.firstIndex(of: 0xF0) {
            if s > 0 { buf.removeFirst(s) }
            guard let e = buf.firstIndex(of: 0xF7) else { break }
            let f = Array(buf[...e]); buf.removeFirst(e + 1)
            onFrame(f)
        }
        if !buf.isEmpty && buf.first != 0xF0 { buf.removeAll() }
        p = MIDIPacketNext(&p).pointee
    }
}
guard st == noErr else { log("destination failed \(st)"); exit(1) }
log("open \"\(name)\" identity \(identity.map { String(format: "%02X", $0) }.joined(separator: " "))")

func note(_ slot: Int, _ n: Int, _ v: Int) {
    let status = UInt8((v > 0 ? 0x90 : 0x80) | (slot & 0x0F))
    stLock.lock()
    waitingSeen["\(v > 0 ? "on" : "off"):\(slot & 0x0F):\(n)", default: []].append(nowNs())
    stLock.unlock()
    send([status, UInt8(n & 0x7F), UInt8(v & 0x7F)])
}
func sleepUntil(_ ns: UInt64) {
    let now = nowNs()
    if ns > now { usleep(useconds_t((ns - now) / 1000)) }
}
func percentiles(_ v: [Double]) -> String {
    guard !v.isEmpty else { return "n=0" }
    let s = v.sorted()
    let q = { (p: Double) in s[min(s.count - 1, Int(Double(s.count - 1) * p + 0.5))] }
    return String(format: "n=%d med=%.2f p95=%.2f max=%.2f ms", s.count, q(0.5), q(0.95), s.last ?? 0)
}

func seq(_ slot: Int, _ n: Int, _ count: Int, _ stepMs: Double, _ lenMs: Double, sysex ch: Int?) {
    let start = nowNs() + 50_000_000
    var events: [(UInt64, Bool)] = []
    for i in 0..<count {
        let at = start + UInt64(Double(i) * stepMs * 1e6)
        events.append((at, true)); events.append((at + UInt64(lenMs * 1e6), false))
    }
    events.sort { $0.0 < $1.0 }
    for (at, on) in events {
        sleepUntil(at)
        if let c = ch {
            call(["cmd": "note", "c": c, "n": n, "v": on ? 100 : 0], kind: "note", wait: false)
        } else {
            note(slot, n, on ? 100 : 0)
        }
    }
    log("sent \(count) notes \(n) \(ch.map { "sysex ch \($0)" } ?? "slot \(slot)")")
}

func handle(_ line: String) -> Bool {
    let w = line.split(separator: " ").map(String.init)
    guard let cmd = w.first else { return true }
    let i = { (k: Int) in k < w.count ? Int(w[k]) ?? 0 : 0 }
    let d = { (k: Int) in k < w.count ? Double(w[k]) ?? 0 : 0 }
    switch cmd {
    case "quit": return false
    case "ping": log("ping \(call(["cmd": "ping"], kind: "ping") ?? [:])")
    case "chans": log("chans \(call(["cmd": "chans"]) ?? [:])")
    case "held": log("held \(call(["cmd": "held"]) ?? [:])")
    case "level": log("level \(call(["cmd": "level", "c": i(1)]) ?? [:])")
    case "sel": log("sel \(call(["cmd": "sel", "c": i(1)]) ?? [:])")
    case "echo": log("echo \(call(["cmd": "echo", "on": w.count > 1 && w[1] == "on"]) ?? [:])")
    case "offvel": log("offvel \(call(["cmd": "offvel", "v": i(1)]) ?? [:])")
    case "map":
        var m: [String: Int] = [:]
        var k = 1
        while k + 1 < w.count { m[w[k]] = Int(w[k + 1]) ?? 0; k += 2 }
        log("map \(m) \(call(["cmd": "nmap", "m": m]) ?? [:])")
    case "rec", "norec", "play", "stop": log("transport \(call(["cmd": "transport", "do": cmd]) ?? [:])")
    case "seq": seq(i(1), i(2), i(3), d(4), d(5), sysex: nil)
    case "sx": seq(0, i(2), i(3), d(4), d(5), sysex: i(1))
    case "ts":
        let base = nowNs() + 50_000_000
        var packets: [(UInt64, [UInt8])] = []
        for k in 0..<4 {
            let at = base + UInt64(k) * 125_000_000
            packets.append((ticks(ns: at), [UInt8(0x90 | (i(1) & 0x0F)), UInt8(i(2)), 100]))
            packets.append((ticks(ns: at + 100_000_000), [UInt8(0x80 | (i(1) & 0x0F)), UInt8(i(2)), 0]))
        }
        packets.sort { $0.0 < $1.0 }
        sendPackets(packets)
        log("ts sent 4 notes \(i(2)) slot \(i(1)) in one packet list, 125 ms apart")
    case "load":
        let slot = i(1), secs = d(2)
        let start = nowNs() + 50_000_000
        let end = start + UInt64(secs * 1e9)
        var nextNote = start, nextPing = start, k = 0
        var offs: [(UInt64, Int)] = []
        while true {
            let now = nowNs()
            if now >= end && offs.isEmpty { break }
            if now >= nextNote && now < end {
                let n = 48 + (k % 24); k += 1
                note(slot, n, 90)
                offs.append((now + 20_000_000, n))
                nextNote += 33_333_333
            }
            while let f = offs.first, f.0 <= now { note(slot, f.1, 0); offs.removeFirst() }
            if now >= nextPing && now < end { call(["cmd": "ping"], kind: "ping", wait: false); nextPing += 100_000_000 }
            usleep(500)
        }
        log("load sent \(k) notes in \(secs) s on slot \(slot)")
    case "stats":
        usleep(300_000)
        stLock.lock()
        for (k, v) in rtt.sorted(by: { $0.key < $1.key }) { log("rtt \(k): \(percentiles(v))") }
        let lost = waitingId.values.filter { $0.1 == "ping" }.count
        let unseen = waitingSeen.values.reduce(0) { $0 + $1.count }
        log("pings unanswered \(lost), notes not echoed \(unseen)")
        rtt = [:]; waitingSeen = [:]; waitingId = waitingId.filter { $0.value.1 != "ping" }
        stLock.unlock()
    default: log("unknown \(line)")
    }
    return true
}

// commands come from the FIFO; reopen after each writer closes it
DispatchQueue.global().async {
    while true {
        guard let h = FileHandle(forReadingAtPath: fifo) else { log("no fifo \(fifo)"); exit(1) }
        let text = String(decoding: h.readDataToEndOfFile(), as: UTF8.self)
        h.closeFile()
        for line in text.split(separator: "\n") {
            let l = line.trimmingCharacters(in: .whitespaces)
            guard !l.isEmpty else { continue }
            log("> \(l)")
            if !handle(l) { log("done"); exit(0) }
        }
    }
}
RunLoop.main.run()
