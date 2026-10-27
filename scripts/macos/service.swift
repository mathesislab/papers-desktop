// Send a text to the "Search Papers" service, as the Services menu does.
import AppKit
let pboard = NSPasteboard(name: NSPasteboard.Name("papers-desktop-test"))
pboard.clearContents()
pboard.setString(CommandLine.arguments[1], forType: .string)
print(NSPerformService("Search Papers", pboard))
