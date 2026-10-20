// Draws Encore's app icon (1024×1024) on Apple's macOS icon grid:
// an 824pt rounded square, magenta→coral gradient, glossy white equalizer bars.
// Usage: swift scripts/make_icon.swift <out.png>
import AppKit

let size: CGFloat = 1024
let out = CommandLine.arguments[1]
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

// Body with a soft drop shadow.
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
let shape = CGPath(roundedRect: body, cornerWidth: 185, cornerHeight: 185, transform: nil)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: rgb(0x000000, 0.35))
ctx.addPath(shape); ctx.setFillColor(rgb(0xd81b60)); ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(shape); ctx.clip()
let bg = CGGradient(colorsSpace: space, colors: [rgb(0x7b1fa2), rgb(0xe91e63), rgb(0xff6a4d)] as CFArray,
                    locations: [0, 0.55, 1])!
ctx.drawLinearGradient(bg, start: CGPoint(x: 160, y: 924), end: CGPoint(x: 864, y: 100), options: [])
// Top sheen.
let sheen = CGGradient(colorsSpace: space, colors: [rgb(0xffffff, 0.22), rgb(0xffffff, 0)] as CFArray,
                       locations: [0, 1])!
ctx.drawLinearGradient(sheen, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 560), options: [])
ctx.restoreGState()

// Equalizer: five capsules, tallest in the middle, with a subtle shadow.
let heights: [CGFloat] = [250, 400, 540, 360, 220]
let barW: CGFloat = 78, gap: CGFloat = 44
let total = CGFloat(heights.count) * barW + CGFloat(heights.count - 1) * gap
var x = (size - total) / 2
let baseY: CGFloat = 512 - 270
for h in heights {
    let r = CGRect(x: x, y: baseY + (540 - h) / 2, width: barW, height: h)
    let bar = CGPath(roundedRect: r, cornerWidth: barW / 2, cornerHeight: barW / 2, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -8), blur: 18, color: rgb(0x4a0024, 0.35))
    ctx.addPath(bar); ctx.setFillColor(rgb(0xffffff)); ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(bar); ctx.clip()
    let shade = CGGradient(colorsSpace: space, colors: [rgb(0xffffff), rgb(0xffe3ec)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(shade, start: CGPoint(x: x, y: r.maxY), end: CGPoint(x: x, y: r.minY), options: [])
    ctx.restoreGState()
    x += barW + gap
}

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
