import CoreGraphics
import Foundation
// Prints "id x y w h" of the on-screen windows of the given pid.
let pid = Int32(CommandLine.arguments[1]) ?? 0
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list {
    guard let p = w[kCGWindowOwnerPID as String] as? Int32, p == pid else { continue }
    let id = w[kCGWindowNumber as String] as? Int ?? 0
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    print(id, b["X"] ?? 0, b["Y"] ?? 0, b["Width"] ?? 0, b["Height"] ?? 0, w[kCGWindowLayer as String] ?? "")
}
