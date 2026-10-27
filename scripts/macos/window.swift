// Print the window number of the panel of Papers Desktop (on screen, the largest).
import CoreGraphics
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
let panel = windows
    .filter { ($0[kCGWindowOwnerName as String] as? String)?.lowercased().contains("papers") == true }
    .max { a, b in
        let area = { (w: [String: Any]) -> Double in
            let b = w[kCGWindowBounds as String] as! [String: Double]
            return b["Width"]! * b["Height"]!
        }
        return area(a) < area(b)
    }
if let panel { print(panel[kCGWindowNumber as String]!) }
