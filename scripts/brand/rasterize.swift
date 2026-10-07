// Rasterizes SVG files to square PNGs with AppKit (no external tool).
// Usage: swift rasterize.swift <in.svg> <out.png> <pixels> [<in.svg> <out.png> <pixels> ...]
import AppKit

let args = Array(CommandLine.arguments.dropFirst())
guard args.count % 3 == 0, !args.isEmpty else {
    FileHandle.standardError.write("usage: rasterize.swift <in.svg> <out.png> <pixels> ...\n".data(using: .utf8)!)
    exit(2)
}
for i in stride(from: 0, to: args.count, by: 3) {
    guard let image = NSImage(contentsOf: URL(fileURLWithPath: args[i])), let px = Int(args[i + 2]),
          let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
                                     samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                     bytesPerRow: 0, bitsPerPixel: 0)
    else {
        FileHandle.standardError.write("cannot rasterize \(args[i])\n".data(using: .utf8)!)
        exit(1)
    }
    rep.size = NSSize(width: px, height: px)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: px, height: px))
    NSGraphicsContext.restoreGraphicsState()
    try rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[i + 1]))
}
