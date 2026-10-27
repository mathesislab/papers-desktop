// Events to one process only: post PID click X Y [double] | key CODE [cmd] [shift] [alt] | type TEXT
import CoreGraphics
let a = CommandLine.arguments
let pid = pid_t(a[1])!
switch a[2] {
case "click":
    let p = CGPoint(x: Double(a[3])!, y: Double(a[4])!)
    let clicks = a.count > 5 ? 2 : 1
    for n in 1...clicks {
        for type in [CGEventType.leftMouseDown, .leftMouseUp] {
            let e = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!
            e.setIntegerValueField(.mouseEventClickState, value: Int64(n))
            e.postToPid(pid)
        }
    }
case "key":
    var flags: CGEventFlags = []
    if a.contains("cmd") { flags.insert(.maskCommand) }
    if a.contains("shift") { flags.insert(.maskShift) }
    if a.contains("alt") { flags.insert(.maskAlternate) }
    if a.contains("ctrl") { flags.insert(.maskControl) }
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(a[3])!, keyDown: down)!
        e.flags = flags
        e.postToPid(pid)
    }
default:
    for ch in a[3].utf16 {
        for down in [true, false] {
            let e = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down)!
            var c = ch
            e.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
            e.postToPid(pid)
        }
    }
}
