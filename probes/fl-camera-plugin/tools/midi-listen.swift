// midi-listen — attaches a CoreMIDI input port to the virtual source named
// "synthexis" (created by the probe plugin on write path D) and counts the
// control-change messages it receives.
//
// Usage: midi-listen [seconds]   (default 12)
// Prints one status line per second and a final summary.

import CoreMIDI
import Foundation

let seconds = CommandLine.arguments.count > 1
    ? (Double(CommandLine.arguments[1]) ?? 12.0)
    : 12.0

var total = 0
var minValue = -1
var maxValue = -1
var ccNumbers = Set<Int>()
var channels = Set<Int>()

var client = MIDIClientRef()
MIDIClientCreateWithBlock("synthexis-midi-listen" as CFString, &client, nil)

var port = MIDIPortRef()
MIDIInputPortCreateWithProtocol(client, "listen" as CFString, ._1_0, &port) { list, _ in
    let numPackets = Int(list.pointee.numPackets)
    // Packets are variable-length: 8-byte timestamp + 4-byte wordCount +
    // wordCount words, laid out right after protocol + numPackets (8 bytes).
    var ptr = UnsafeRawPointer(list).advanced(by: 8)
    for _ in 0..<numPackets {
        let packet = ptr.load(as: MIDIEventPacket.self)
        let wordCount = Int(packet.wordCount)
        withUnsafeBytes(of: packet.words) { raw in
            let words = raw.bindMemory(to: UInt32.self)
            for i in 0..<min(wordCount, words.count) {
                let word = words[i]
                // Only UMP message type 0x2 (MIDI 1.0 channel voice).
                guard (word >> 28) & 0xF == 0x2 else { continue }
                let status = UInt8((word >> 16) & 0xFF)
                let data1 = Int((word >> 8) & 0xFF)
                let data2 = Int(word & 0xFF)
                guard (status & 0xF0) == 0xB0 else { continue }
                total += 1
                ccNumbers.insert(data1)
                channels.insert(Int(status & 0x0F) + 1)
                if minValue < 0 || data2 < minValue { minValue = data2 }
                if data2 > maxValue { maxValue = data2 }
            }
        }
        ptr = ptr.advanced(by: 12 + wordCount * 4)
    }
}

let t0 = Date()
var connected = false
var lastCount = 0
while Date().timeIntervalSince(t0) < seconds {
    if !connected {
        for i in 0..<MIDIGetNumberOfSources() {
            let source = MIDIGetSource(i)
            var name: Unmanaged<CFString>?
            MIDIObjectGetStringProperty(source, kMIDIPropertyName, &name)
            if (name?.takeUnretainedValue() as String?) == "synthexis" {
                MIDIPortConnectSource(port, source, nil)
                print("connected to virtual source 'synthexis'")
                connected = true
            }
        }
    }
    RunLoop.current.run(until: Date().addingTimeInterval(1.0))
    let perSecond = total - lastCount
    lastCount = total
    print("t+\(Int(Date().timeIntervalSince(t0)))s cc/s=\(perSecond) total=\(total) connected=\(connected)")
}

print("summary: total_cc=\(total) cc_numbers=\(ccNumbers.sorted()) " +
      "channels=\(channels.sorted()) min=\(minValue) max=\(maxValue)")
