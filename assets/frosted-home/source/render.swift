import Foundation
import CoreGraphics
import CoreText
import ImageIO
import UniformTypeIdentifiers

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
for f in ["Inter.ttf", "Inter-Bold.ttf", "LexendDeca.ttf"] {
    CTFontManagerRegisterFontsForURL(root.appendingPathComponent("assets/" + f) as CFURL, .process, nil)
}
let sceneURL = URL(fileURLWithPath: CommandLine.arguments[1])
let scenes = try JSONSerialization.jsonObject(with: Data(contentsOf: sceneURL)) as! [[String: Any]]
var diagnostics: [[String: Any]] = []
var columnMetrics: [[String: Any]] = []
for scene in scenes {
    let w = scene["width"] as! Int, h = scene["height"] as! Int
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    ctx.setFillColor(gray: 1, alpha: 1); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
    ctx.translateBy(x: 0, y: CGFloat(h)); ctx.scaleBy(x: 1, y: -1)
    for op in scene["ops"] as! [[String: Any]] {
        func n(_ key: String, _ d: Double = 0) -> CGFloat { CGFloat((op[key] as? NSNumber)?.doubleValue ?? d) }
        let type = op["type"] as! String
        if type == "image" {
            let url = root.appendingPathComponent(op["file"] as! String)
            let source = CGImageSourceCreateWithURL(url as CFURL, nil)!
            var image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
            if let crop = op["source_rect"] as? [Double] {
                image = image.cropping(to: CGRect(x: crop[0], y: crop[1], width: crop[2], height: crop[3]))!
            }
            ctx.saveGState(); ctx.interpolationQuality = op["smooth"] as? Bool == true ? .high : .none
            if op["blend_mode"] as? String == "multiply" { ctx.setBlendMode(.multiply) }
            ctx.translateBy(x: n("x"), y: n("y") + n("h")); ctx.scaleBy(x: 1, y: -1)
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: n("w"), height: n("h"))); ctx.restoreGState()
            continue
        }
        let fill = op["fill"] as? String ?? "none"
        let stroke = op["stroke"] as? String ?? "none"
        ctx.setFillColor(gray: fill == "white" ? 1 : 0, alpha: 1)
        ctx.setStrokeColor(gray: stroke == "white" ? 1 : stroke == "#555555" ? 1/3 : 0, alpha: 1)
        ctx.setLineWidth(n("sw", 2)); ctx.setLineCap(.round); ctx.setLineJoin(.round)
        if type == "text" {
            let requestedName = op["font"] as? String ?? "Inter18pt-Regular"
            let name = op["bold"] as? Bool == true ? requestedName.replacingOccurrences(of: "-Regular", with: "-Bold") : requestedName
            let font = CTFontCreateWithName(name as CFString, n("size", 24), nil)
            // Font fallback must be visible as a build failure, never a silently wrong mockup.
            let actualName = CTFontCopyPostScriptName(font) as String
            precondition(actualName == name, "Font substitution: requested \(name), rendered \(actualName)")
            let attrs: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): font, NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray: fill == "white" ? 1 : 0, alpha: 1)]
            let str = NSAttributedString(string: op["text"] as! String, attributes: attrs)
            let line = CTLineCreateWithAttributedString(str)
            let width = CTLineGetTypographicBounds(line, nil, nil, nil)
            var x = n("x")
            if op["anchor"] as? String == "middle" { x -= width/2 }
            if op["anchor"] as? String == "end" { x -= width }
            let top = n("y") - CTFontGetAscent(font)
            let bottom = n("y") + CTFontGetDescent(font)
            if x < 0 || x + width > CGFloat(w) || top < 0 || bottom > CGFloat(h) { diagnostics.append(["screen": scene["name"]!, "text": op["text"]!, "x": x, "width": width, "top":top, "bottom":bottom]) }
            if let limit = op["max_width"] as? Double {
                columnMetrics.append(["screen": scene["name"]!, "text": op["text"]!, "left": x, "right": x + width, "width": width, "limit": limit])
                if width > limit { diagnostics.append(["screen": scene["name"]!, "text": op["text"]!, "column_width": width, "limit": limit]) }
            }
            ctx.saveGState(); ctx.translateBy(x: x, y: n("y")); ctx.scaleBy(x: 1, y: -1); ctx.textPosition = .zero; CTLineDraw(line, ctx); ctx.restoreGState()
            continue
        }
        let path = CGMutablePath()
        if type == "rect" { path.addRoundedRect(in: CGRect(x: n("x"), y: n("y"), width: n("w"), height: n("h")), cornerWidth: n("r"), cornerHeight: n("r")) }
        if type == "ellipse" { path.addEllipse(in: CGRect(x: n("x"), y: n("y"), width: n("w"), height: n("h"))) }
        if type == "path" {
            for cmd in op["commands"] as! [[Any]] {
                func v(_ i: Int) -> CGFloat { CGFloat((cmd[i] as! NSNumber).doubleValue) }
                switch cmd[0] as! String {
                case "M": path.move(to: CGPoint(x: v(1), y: v(2)))
                case "L": path.addLine(to: CGPoint(x: v(1), y: v(2)))
                case "C": path.addCurve(to: CGPoint(x: v(5), y: v(6)), control1: CGPoint(x: v(1), y: v(2)), control2: CGPoint(x: v(3), y: v(4)))
                case "Q": path.addQuadCurve(to: CGPoint(x: v(3), y: v(4)), control: CGPoint(x: v(1), y: v(2)))
                case "Z": path.closeSubpath()
                default: break
                }
            }
        }
        ctx.addPath(path)
        ctx.drawPath(using: fill != "none" && stroke != "none" ? .fillStroke : fill != "none" ? .fill : .stroke)
    }
    // Device fixtures use four scalar levels; comparison sheets retain host grays.
    let bytes = ctx.data!.assumingMemoryBound(to: UInt8.self)
    let grayCandidate = scene["grayCandidate"] as? Bool == true
    let keepGrays = scene["keepGrays"] as? Bool == true
    for i in 0..<(w*h) { if !keepGrays { bytes[i] = grayCandidate ? UInt8(min(3, Int((Double(bytes[i])/85).rounded())) * 85) : bytes[i] < 160 ? 0 : 255 } }
    let image = ctx.makeImage()!
    let dest = CGImageDestinationCreateWithURL(root.appendingPathComponent("screens/" + (scene["name"] as! String) + ".png") as CFURL, UTType.png.identifier as CFString, 1, nil)!
    let properties: [CFString:Any] = keepGrays && scene["name"] as? String == "device-size-proof" ? [kCGImagePropertyDPIWidth:96,kCGImagePropertyDPIHeight:96] : [:]
    CGImageDestinationAddImage(dest, image, properties as CFDictionary); CGImageDestinationFinalize(dest)
}
let data = try JSONSerialization.data(withJSONObject: diagnostics, options: [.prettyPrinted, .sortedKeys])
let diagnosticsName = "text-bounds-" + sceneURL.lastPathComponent.replacingOccurrences(of: "-scenes.json", with: "").replacingOccurrences(of: "-scene.json", with: "") + ".json"
try data.write(to: root.appendingPathComponent(diagnosticsName))
if !columnMetrics.isEmpty {
    let metrics = try JSONSerialization.data(withJSONObject: columnMetrics, options: [.prettyPrinted,.sortedKeys])
    try metrics.write(to: root.appendingPathComponent("column-metrics.json"))
}
print("Rendered \(scenes.count) screens; text bounds failures: \(diagnostics.count)")
precondition(diagnostics.isEmpty, "Text bounds failed; revise the mockups before delivery")
