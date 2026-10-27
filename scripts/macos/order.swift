// On-screen windows, front to back, that contain point (x, y).
import CoreGraphics
let x = Double(CommandLine.arguments[1])!, y = Double(CommandLine.arguments[2])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in windows {
    let b = w[kCGWindowBounds as String] as! [String: Double]
    if x >= b["X"]! && x <= b["X"]! + b["Width"]! && y >= b["Y"]! && y <= b["Y"]! + b["Height"]! && (w[kCGWindowLayer as String] as! Int) == 0 {
        print(w[kCGWindowOwnerName as String]!, "|", w[kCGWindowName as String] ?? "", "|", b)
    }
}
