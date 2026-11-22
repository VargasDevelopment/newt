// Host-only ImageIO thumbnailing. Firmware consumes fixed NLC2 scanlines.
import Foundation
import CoreGraphics
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}
guard CommandLine.arguments.count == 3 else { fail("Expected input image and output bitmap") }
let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
guard let attributes = try? FileManager.default.attributesOfItem(atPath: input.path),
      let size = attributes[.size] as? NSNumber, size.intValue > 0,
      size.intValue <= 4 * 1024 * 1024,
      let source = CGImageSourceCreateWithURL(input as CFURL,
        [kCGImageSourceShouldCache: false] as CFDictionary),
      CGImageSourceGetCount(source) == 1,
      let type = CGImageSourceGetType(source) as String?,
      type == "public.png" || type == "public.jpeg",
      let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let width = props[kCGImagePropertyPixelWidth] as? Int,
      let height = props[kCGImagePropertyPixelHeight] as? Int,
      width > 0, height > 0, width <= 8192, height <= 8192,
      width * height <= 32 * 1024 * 1024 else { fail("Unsupported image bounds or format") }
let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
    kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceShouldCache: false,
    kCGImageSourceThumbnailMaxPixelSize: 188]
guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary),
      image.width <= 188, image.height <= 188,
      let context = CGContext(data: nil, width: 144, height: 188, bitsPerComponent: 8,
        bytesPerRow: 144, space: CGColorSpaceCreateDeviceGray(),
        bitmapInfo: CGImageAlphaInfo.none.rawValue) else { fail("Cannot thumbnail image") }
context.setFillColor(gray: 1, alpha: 1)
context.fill(CGRect(x: 0, y: 0, width: 144, height: 188))
let scale = min(144.0 / Double(image.width), 188.0 / Double(image.height))
let fittedWidth = Double(image.width) * scale, fittedHeight = Double(image.height) * scale
context.interpolationQuality = .high
context.draw(image, in: CGRect(x: (144 - fittedWidth) / 2, y: (188 - fittedHeight) / 2,
    width: fittedWidth, height: fittedHeight))
guard let raw = context.data?.assumingMemoryBound(to: UInt8.self) else { fail("No raster") }
// Host-only Floyd–Steinberg quantization avoids a fixed checkerboard texture.
// Two small error rows do not exist in firmware; output stays fixed 1bpp.
var current = [Double](repeating: 0, count: 146)
var following = [Double](repeating: 0, count: 146)
var packed = Data(count: 144 * 188 / 8)
for y in 0..<188 {
    for x in 0..<144 {
        let value = max(0, min(255, Double(raw[y * 144 + x]) + current[x + 1]))
        let tone = value < 128 ? 0.0 : 255.0
        if tone == 0 { packed[y * 18 + x / 8] |= UInt8(0x80 >> (x % 8)) }
        let error = value - tone
        current[x + 2] += error * 7 / 16
        following[x] += error * 3 / 16
        following[x + 1] += error * 5 / 16
        following[x + 2] += error / 16
    }
    swap(&current, &following)
    for x in following.indices { following[x] = 0 }
}
do { try packed.write(to: output, options: .atomic) } catch { fail("Cannot write bitmap") }
