// Render an SVG into a square PNG, leaving a margin around it:
//   swift render.swift in.svg out.png size margin
import AppKit

let args = CommandLine.arguments
let (size, margin) = (Int(args[3])!, CGFloat(Double(args[4])!))
guard let image = NSImage(contentsOf: URL(fileURLWithPath: args[1])) else { fatalError("cannot read \(args[1])") }
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
image.draw(in: NSRect(x: margin, y: margin, width: CGFloat(size) - 2 * margin, height: CGFloat(size) - 2 * margin))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
