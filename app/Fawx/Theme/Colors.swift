import SwiftUI

#if os(macOS)
import AppKit
private typealias PlatformColor = NSColor
#else
import UIKit
private typealias PlatformColor = UIColor
#endif

extension Color {
    static var fawxBackground: Color { palette(light: 0xFFFFFF, dark: 0x1A1A1A) }
    static var fawxSurface: Color { palette(light: 0xF5F5F5, dark: 0x242424) }
    static var fawxSurfaceHover: Color { palette(light: 0xEBEBEB, dark: 0x2E2E2E) }
    static var fawxSurfaceActive: Color { palette(light: 0xE0E0E0, dark: 0x383838) }
    static var fawxText: Color { palette(light: 0x1A1A1A, dark: 0xE8E8E8) }
    static var fawxTextSecondary: Color { palette(light: 0x666666, dark: 0x999999) }
    static var fawxAccent: Color { palette(light: 0xD45E14, dark: 0xE8711A) }
    static var fawxAccentSubtle: Color { palette(light: 0xD45E14, dark: 0xE8711A, lightAlpha: 0.08, darkAlpha: 0.13) }
    static var fawxSuccess: Color { palette(light: 0x22C55E, dark: 0x4ADE80) }
    static var fawxWarning: Color { palette(light: 0xD97706, dark: 0xFBBF24) }
    static var fawxError: Color { palette(light: 0xDC2626, dark: 0xF87171) }
    static var fawxBorder: Color { palette(light: 0xE5E5E5, dark: 0x333333) }
    static var fawxCode: Color { palette(light: 0xF0F0F0, dark: 0x2D2D2D) }

    private static func palette(
        light: UInt32,
        dark: UInt32,
        lightAlpha: Double = 1,
        darkAlpha: Double = 1
    ) -> Color {
        Color(
            light: PlatformColor(hex: light, alpha: lightAlpha),
            dark: PlatformColor(hex: dark, alpha: darkAlpha)
        )
    }

    private init(light: PlatformColor, dark: PlatformColor) {
#if os(macOS)
        self.init(nsColor: .init(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        })
#else
        self.init(uiColor: .init { traits in
            traits.userInterfaceStyle == .dark ? dark : light
        })
#endif
    }
}

private extension PlatformColor {
    convenience init(hex: UInt32, alpha: Double) {
        let red = CGFloat((hex >> 16) & 0xFF) / 255
        let green = CGFloat((hex >> 8) & 0xFF) / 255
        let blue = CGFloat(hex & 0xFF) / 255

#if os(macOS)
        self.init(red: red, green: green, blue: blue, alpha: alpha)
#else
        self.init(red: red, green: green, blue: blue, alpha: alpha)
#endif
    }
}
