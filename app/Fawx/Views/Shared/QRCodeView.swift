import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

struct QRCodeView: View {
    let payload: String
    let size: CGFloat

    private let context = CIContext()
    private let filter = CIFilter.qrCodeGenerator()

    var body: some View {
        Group {
            if let image = qrImage {
#if os(macOS)
                Image(nsImage: image)
                    .interpolation(.none)
                    .resizable()
#else
                Image(uiImage: image)
                    .interpolation(.none)
                    .resizable()
#endif
            } else {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(Color.fawxSurfaceHover)
                    .overlay {
                        Image(systemName: "qrcode")
                            .font(.system(size: 28, weight: .medium))
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
        .background(Color.white)
    }

#if os(macOS)
    private var qrImage: NSImage? {
        guard let cgImage = cgImage else {
            return nil
        }
        return NSImage(cgImage: cgImage, size: NSSize(width: size, height: size))
    }
#else
    private var qrImage: UIImage? {
        guard let cgImage = cgImage else {
            return nil
        }
        return UIImage(cgImage: cgImage)
    }
#endif

    private var cgImage: CGImage? {
        filter.setValue(Data(payload.utf8), forKey: "inputMessage")
        filter.correctionLevel = "M"

        guard let outputImage = filter.outputImage else {
            return nil
        }

        let scale = max(1, floor(size / outputImage.extent.width))
        let transformed = outputImage.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
        return context.createCGImage(transformed, from: transformed.extent)
    }
}
