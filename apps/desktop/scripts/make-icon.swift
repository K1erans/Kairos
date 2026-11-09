// Draws the Kairos icon family.
//   swift make-icon.swift app <out.iconset>      macOS iconset for Kairos Desktop
//   swift make-icon.swift shield|read <out dir>  icon16/48/128.png for a Chrome extension
import AppKit

let kind = CommandLine.arguments[1]
let out = CommandLine.arguments[2]
try FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)

struct Look {
    let top: NSColor, bottom: NSColor, symbol: String?
}

let looks: [String: Look] = [
    "app": Look(top: NSColor(red: 0.33, green: 0.42, blue: 0.95, alpha: 1), bottom: NSColor(red: 0.12, green: 0.13, blue: 0.36, alpha: 1), symbol: nil),
    "shield": Look(top: NSColor(red: 0.20, green: 0.74, blue: 0.62, alpha: 1), bottom: NSColor(red: 0.05, green: 0.30, blue: 0.33, alpha: 1), symbol: "shield.lefthalf.filled"),
    "read": Look(top: NSColor(red: 0.98, green: 0.58, blue: 0.30, alpha: 1), bottom: NSColor(red: 0.62, green: 0.18, blue: 0.34, alpha: 1), symbol: "waveform"),
]
guard let look = looks[kind] else { fatalError("unknown icon kind \(kind)") }

func render(_ px: Int, margin: CGFloat) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
                               hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = CGFloat(px), inset = s * margin
    let rect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let radius = rect.width * 0.235
    let body = NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)
    NSGradient(colors: [look.top, look.bottom])!.draw(in: body, angle: -90)

    // Glass: a soft highlight across the top half and a thin bright rim.
    NSGraphicsContext.saveGraphicsState()
    body.addClip()
    let gloss = NSBezierPath(ovalIn: NSRect(x: rect.minX - rect.width * 0.25, y: rect.midY + rect.height * 0.05,
                                            width: rect.width * 1.5, height: rect.height * 0.9))
    NSGradient(colors: [NSColor.white.withAlphaComponent(0.30), NSColor.white.withAlphaComponent(0.02)])!.draw(in: gloss, angle: -90)
    NSGraphicsContext.restoreGraphicsState()
    let rim = NSBezierPath(roundedRect: rect.insetBy(dx: s * 0.006, dy: s * 0.006), xRadius: radius, yRadius: radius)
    rim.lineWidth = max(1, s * 0.012)
    NSColor.white.withAlphaComponent(0.28).setStroke()
    rim.stroke()

    let c = NSPoint(x: rect.midX, y: rect.midY)
    if let name = look.symbol {
        let config = NSImage.SymbolConfiguration(pointSize: rect.width * 0.5, weight: .semibold)
            .applying(.init(paletteColors: [.white]))
        if let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config) {
            let size = symbol.size
            let scale = min(rect.width * 0.58 / size.width, rect.height * 0.58 / size.height)
            let w = size.width * scale, h = size.height * scale
            symbol.draw(in: NSRect(x: c.x - w / 2, y: c.y - h / 2, width: w, height: h))
        }
    } else {
        // Kairos: a timer ring three-quarters run, with a dot at "the right moment".
        let r = rect.width * 0.30, width = rect.width * 0.075
        let track = NSBezierPath()
        track.appendArc(withCenter: c, radius: r, startAngle: 0, endAngle: 360)
        track.lineWidth = width
        NSColor.white.withAlphaComponent(0.22).setStroke()
        track.stroke()
        let arc = NSBezierPath()
        arc.appendArc(withCenter: c, radius: r, startAngle: 90, endAngle: 180, clockwise: true)
        arc.lineWidth = width
        arc.lineCapStyle = .round
        NSColor.white.setStroke()
        arc.stroke()
        NSColor.white.setFill()
        let d = rect.width * 0.13
        NSBezierPath(ovalIn: NSRect(x: c.x - d / 2, y: c.y - d / 2, width: d, height: d)).fill()
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

if kind == "app" {
    for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256),
                       ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
        try render(px, margin: 0.1).write(to: URL(fileURLWithPath: "\(out)/icon_\(name).png"))
    }
} else {
    for px in [16, 48, 128] {
        try render(px, margin: 0.04).write(to: URL(fileURLWithPath: "\(out)/icon\(px).png"))
    }
}
