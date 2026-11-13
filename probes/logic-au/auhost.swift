// Minimal in-process AUv2 host without NSCameraUsageDescription, the same
// situation as Logic Pro 12.4: instantiates aumu sgnl Snxs, opens its view
// (which makes synthexis want the camera), waits, prints the plugin log
// lines about camera access and quits.
// Build: swiftc -O auhost.swift -o auhost   Run: ./auhost [seconds]
import AVFoundation
import AppKit
import AudioToolbox
import CoreAudioKit

func fourCC(_ s: String) -> OSType { s.utf8.reduce(0) { $0 << 8 | OSType($1) } }

let seconds = Double(CommandLine.arguments.dropFirst().first ?? "15") ?? 15
let desc = AudioComponentDescription(componentType: fourCC("aumu"), componentSubType: fourCC("sgnl"),
                                     componentManufacturer: fourCC("Snxs"), componentFlags: 0, componentFlagsMask: 0)
// Held for the whole run: a released AVAudioUnit closes the AU and its editor.
var held: AVAudioUnit?
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
print("main bundle camera key:", Bundle.main.object(forInfoDictionaryKey: "NSCameraUsageDescription") ?? "none")
AVAudioUnit.instantiate(with: desc, options: [.loadInProcess]) { unit, err in
    guard let unit else { print("instantiate failed:", err ?? "?"); exit(1) }
    held = unit
    print("instantiated:", unit.name, "by", unit.manufacturerName)
    unit.auAudioUnit.requestViewController { vc in
        guard let vc else { print("no view controller"); exit(2) }
        let win = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 900, height: 600),
                           styleMask: [.titled], backing: .buffered, defer: false)
        win.contentViewController = vc
        win.orderFront(nil)
        print("view open:", vc.view.frame.size)
        func dump(_ v: NSView, _ d: Int) {
            print(String(repeating: "  ", count: d) + String(describing: type(of: v)), v.frame)
            v.subviews.forEach { dump($0, d + 1) }
        }
        for t in [0.5, 3.0] {
            DispatchQueue.main.asyncAfter(deadline: .now() + t) {
                print("views after \(t) s:")
                if let root = win.contentView { dump(root, 1) }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { print("done"); exit(0) }
    }
}
app.run()
