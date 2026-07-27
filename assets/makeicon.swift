// Generates icon_1024.png for Voice — "Sticker" concept:
// ivory squircle, white capsule with bold ink outline, lavender offset
// shadow, ink waveform bars. Run: swift makeicon.swift
import AppKit

let px = 1024
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                           isPlanar: false, colorSpaceName: .deviceRGB,
                           bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

let size = CGFloat(px)
let cream = NSColor(calibratedRed: 0.969, green: 0.955, blue: 0.925, alpha: 1)
let creamDeep = NSColor(calibratedRed: 0.933, green: 0.912, blue: 0.869, alpha: 1)
let ink = NSColor(calibratedRed: 0.129, green: 0.125, blue: 0.110, alpha: 1)
let lavender = NSColor(calibratedRed: 0.788, green: 0.671, blue: 0.933, alpha: 1)
let paper = NSColor(calibratedRed: 1.0, green: 0.992, blue: 0.965, alpha: 1)

// Background: ivory rounded square with a soft vertical gradient
let radius = size * 0.2237
let bgRect = NSRect(x: 0, y: 0, width: size, height: size).insetBy(dx: size * 0.03, dy: size * 0.03)
let bg = NSBezierPath(roundedRect: bgRect, xRadius: radius, yRadius: radius)
NSGradient(colors: [cream, creamDeep])!.draw(in: bg, angle: -90)

// Lavender offset capsule (the sticker's drop shadow)
let capW: CGFloat = 688, capH: CGFloat = 300
let whiteRect = NSRect(x: 160, y: 380, width: capW, height: capH)
let lavRect = whiteRect.offsetBy(dx: 48, dy: -48)
lavender.setFill()
NSBezierPath(roundedRect: lavRect, xRadius: capH / 2, yRadius: capH / 2).fill()

// White capsule with a bold ink outline
let capsule = NSBezierPath(roundedRect: whiteRect, xRadius: capH / 2, yRadius: capH / 2)
paper.setFill()
capsule.fill()
ink.setStroke()
capsule.lineWidth = 26
capsule.stroke()

// Ink waveform bars
let heights: [CGFloat] = [60, 112, 156, 192, 126, 168, 92, 134, 68]
let barW: CGFloat = 28
let step: CGFloat = 60
var x: CGFloat = 256
let midY = whiteRect.midY
ink.setFill()
for h in heights {
    let r = NSRect(x: x, y: midY - h / 2, width: barW, height: h)
    NSBezierPath(roundedRect: r, xRadius: barW / 2, yRadius: barW / 2).fill()
    x += step
}

NSGraphicsContext.restoreGraphicsState()

let png = rep.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: "icon_1024.png"))
print("wrote icon_1024.png")
