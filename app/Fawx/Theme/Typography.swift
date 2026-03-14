import SwiftUI

@MainActor
enum FawxTypography {
    private static var fontScale: CGFloat = 1

    static var chatBody: Font { scaledFont(size: 14, weight: .regular) }
    static var sidebar: Font { scaledFont(size: 13, weight: .regular) }
    static var sidebarTitle: Font { scaledFont(size: 13, weight: .semibold) }
    static var input: Font { scaledFont(size: 14, weight: .regular) }
    static var status: Font { scaledFont(size: 11, weight: .regular) }
    static var heading1: Font { scaledFont(size: 18, weight: .bold) }
    static var heading2: Font { scaledFont(size: 16, weight: .semibold) }
    static var code: Font { scaledFont(size: 13, weight: .regular, design: .monospaced) }

    static func setScale(_ scale: CGFloat) {
        fontScale = max(0.85, min(scale, 1.25))
    }

    private static func scaledFont(
        size: CGFloat,
        weight: Font.Weight,
        design: Font.Design = .default
    ) -> Font {
        .system(size: size * fontScale, weight: weight, design: design)
    }
}
