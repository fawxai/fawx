import Foundation
import SwiftUI

#if os(macOS)
import AppKit
private typealias PlatformColor = NSColor
#else
import UIKit
private typealias PlatformColor = UIColor
#endif

private enum FawxPaletteHex {
    static let backgroundLight: UInt32 = 0xFFFFFF
    static let backgroundDark: UInt32 = 0x1A1A1A
}

struct AppAccentColor: Equatable, Sendable {
    enum Channel {
        case red
        case green
        case blue
    }

    static let `default` = AppAccentColor(red: 212.0 / 255.0, green: 94.0 / 255.0, blue: 20.0 / 255.0)
    private static let contrastBlendIterations = 10

    let red: Double
    let green: Double
    let blue: Double

    init(red: Double, green: Double, blue: Double) {
        self.red = Self.clamp(red)
        self.green = Self.clamp(green)
        self.blue = Self.clamp(blue)
    }

    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }

    init?(hexString: String) {
        let trimmed = hexString.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalized = trimmed.hasPrefix("#") ? String(trimmed.dropFirst()) : trimmed
        guard normalized.count == 6, let value = UInt32(normalized, radix: 16) else {
            return nil
        }

        self.init(hex: value)
    }

    var hexString: String {
        String(format: "#%02X%02X%02X", redByte, greenByte, blueByte)
    }

    var color: Color {
        Color(red: red, green: green, blue: blue)
    }

    var contrastingTextColor: Color {
        relativeLuminance > 0.32 ? .black : .white
    }

    var redByte: Int { byteValue(red) }
    var greenByte: Int { byteValue(green) }
    var blueByte: Int { byteValue(blue) }

    func updating(_ channel: Channel, byteValue: Double) -> AppAccentColor {
        let componentValue = Self.clamp(byteValue / 255)
        switch channel {
        case .red:
            return AppAccentColor(red: componentValue, green: green, blue: blue)
        case .green:
            return AppAccentColor(red: red, green: componentValue, blue: blue)
        case .blue:
            return AppAccentColor(red: red, green: green, blue: componentValue)
        }
    }

    private func byteValue(_ value: Double) -> Int {
        Int((Self.clamp(value) * 255).rounded())
    }

    var relativeLuminance: Double {
        (0.2126 * linearized(red)) + (0.7152 * linearized(green)) + (0.0722 * linearized(blue))
    }

    func contrastRatio(against backgroundLuminance: Double) -> Double {
        let lighter = max(relativeLuminance, backgroundLuminance)
        let darker = min(relativeLuminance, backgroundLuminance)
        return (lighter + 0.05) / (darker + 0.05)
    }

    func resolvedForFawxChrome(in scheme: FawxAccentContrastScheme) -> AppAccentColor {
        guard contrastRatio(against: scheme.backgroundLuminance) < scheme.minimumContrast else {
            return self
        }

        var low = 0.0
        var high = 1.0
        var best = self
        for _ in 0 ..< Self.contrastBlendIterations {
            let midpoint = (low + high) / 2
            let candidate = blended(with: scheme.adjustmentColor, amount: midpoint)
            if candidate.contrastRatio(against: scheme.backgroundLuminance) >= scheme.minimumContrast {
                best = candidate
                high = midpoint
            } else {
                low = midpoint
            }
        }
        return best
    }

    private func blended(with other: AppAccentColor, amount: Double) -> AppAccentColor {
        let clampedAmount = Self.clamp(amount)
        return AppAccentColor(
            red: red + ((other.red - red) * clampedAmount),
            green: green + ((other.green - green) * clampedAmount),
            blue: blue + ((other.blue - blue) * clampedAmount)
        )
    }

    private func linearized(_ value: Double) -> Double {
        let clampedValue = Self.clamp(value)
        if clampedValue <= 0.04045 {
            return clampedValue / 12.92
        }
        return pow((clampedValue + 0.055) / 1.055, 2.4)
    }

    private static func clamp(_ value: Double) -> Double {
        min(1, max(0, value))
    }
}

enum FawxAccentContrastScheme {
    case light
    case dark

    var minimumContrast: Double { 3.0 }

    var backgroundLuminance: Double {
        switch self {
        case .light:
            return AppAccentColor(hex: FawxPaletteHex.backgroundLight).relativeLuminance
        case .dark:
            return AppAccentColor(hex: FawxPaletteHex.backgroundDark).relativeLuminance
        }
    }

    var adjustmentColor: AppAccentColor {
        switch self {
        case .light:
            return AppAccentColor(red: 0, green: 0, blue: 0)
        case .dark:
            return AppAccentColor(red: 1, green: 1, blue: 1)
        }
    }
}

enum FawxAccentPalette {
    private static let lock = NSLock()
    // SAFETY: `currentAccent` is a Sendable value type, and every read/write is
    // protected by `lock`. Dynamic platform colors can resolve outside SwiftUI's
    // main render pass, so the palette needs a synchronized process-wide cache.
    // TODO: move this into scene-local environment if Fawx ever supports per-window accents.
    nonisolated(unsafe) private static var currentAccent = AppAccentColor.default

    static func update(_ accent: AppAccentColor) {
        lock.lock()
        defer { lock.unlock() }
        currentAccent = accent
    }

    private static func currentAccentSnapshot() -> AppAccentColor {
        lock.lock()
        defer { lock.unlock() }
        return currentAccent
    }

    static var color: Color {
#if os(macOS)
        Color(nsColor: PlatformColor(name: nil) { appearance in
            currentPlatformColor(for: FawxAccentContrastScheme(appearance: appearance))
        })
#else
        Color(uiColor: PlatformColor { traits in
            currentPlatformColor(for: FawxAccentContrastScheme(traits: traits))
        })
#endif
    }

    static var textColor: Color {
#if os(macOS)
        Color(nsColor: PlatformColor(name: nil) { appearance in
            currentPlatformTextColor(for: FawxAccentContrastScheme(appearance: appearance))
        })
#else
        Color(uiColor: PlatformColor { traits in
            currentPlatformTextColor(for: FawxAccentContrastScheme(traits: traits))
        })
#endif
    }

    private static func currentPlatformColor(for scheme: FawxAccentContrastScheme) -> PlatformColor {
        let accent = currentAccentSnapshot()
        let resolvedAccent = accent.resolvedForFawxChrome(in: scheme)

        return PlatformColor(
            red: CGFloat(resolvedAccent.red),
            green: CGFloat(resolvedAccent.green),
            blue: CGFloat(resolvedAccent.blue),
            alpha: 1
        )
    }

    private static func currentPlatformTextColor(for scheme: FawxAccentContrastScheme) -> PlatformColor {
        let accent = currentAccentSnapshot()
        let resolvedAccent = accent.resolvedForFawxChrome(in: scheme)
        let useBlackText = resolvedAccent.relativeLuminance > 0.32

        return PlatformColor(
            red: useBlackText ? 0 : 1,
            green: useBlackText ? 0 : 1,
            blue: useBlackText ? 0 : 1,
            alpha: 1
        )
    }
}

private struct FawxAccentInvalidationTokenKey: EnvironmentKey {
    static let defaultValue = AppAccentColor.default.hexString
}

extension EnvironmentValues {
    var fawxAccentInvalidationToken: String {
        get { self[FawxAccentInvalidationTokenKey.self] }
        set { self[FawxAccentInvalidationTokenKey.self] = newValue }
    }
}

extension View {
    func fawxAccentInvalidation(_ accent: AppAccentColor) -> some View {
        environment(\.fawxAccentInvalidationToken, accent.hexString)
    }
}

extension Color {
    static var fawxBackground: Color {
        palette(light: FawxPaletteHex.backgroundLight, dark: FawxPaletteHex.backgroundDark)
    }
    static var fawxSurface: Color { fawxBackground }
    static var fawxSurfaceHover: Color { fawxText.opacity(0.05) }
    static var fawxSurfaceActive: Color { fawxText.opacity(0.08) }
    static var fawxText: Color { palette(light: 0x1A1A1A, dark: 0xE8E8E8) }
    static var fawxTextSecondary: Color { palette(light: 0x666666, dark: 0x999999) }
    static var fawxAccent: Color { FawxAccentPalette.color }
    static var fawxAccentText: Color { FawxAccentPalette.textColor }
    static var fawxAccentSubtle: Color { fawxAccent.opacity(0.1) }
    static var fawxUserBubble: Color { palette(light: 0xE8BC9E, dark: 0x4B3022) }
    static var fawxUserBubbleText: Color { palette(light: 0x24170F, dark: 0xF7ECE4) }
    static var fawxSuccess: Color { palette(light: 0x22C55E, dark: 0x4ADE80) }
    static var fawxWarning: Color { palette(light: 0xD97706, dark: 0xFBBF24) }
    static var fawxError: Color { palette(light: 0xDC2626, dark: 0xF87171) }
    static var fawxBorder: Color { palette(light: 0xE5E5E5, dark: 0x333333) }
    static var fawxCode: Color { fawxText.opacity(0.06) }

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

#if os(macOS)
extension FawxAccentContrastScheme {
    init(appearance: NSAppearance) {
        self = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? .dark : .light
    }
}

extension NSColor {
    static var fawxTextInsertionPoint: NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(hex: 0xE8E8E8, alpha: 1)
                : NSColor(hex: 0x1A1A1A, alpha: 1)
        }
    }

    static var fawxTextSelectionBackground: NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(hex: 0x4A4A4A, alpha: 1)
                : NSColor(hex: 0xDCE1E8, alpha: 1)
        }
    }

    static var fawxTextSelectionForeground: NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(hex: 0xF2F2F2, alpha: 1)
                : NSColor(hex: 0x111111, alpha: 1)
        }
    }
}

extension NSTextView {
    func applyFawxTextSelectionChrome() {
        selectedTextAttributes = [
            .backgroundColor: NSColor.fawxTextSelectionBackground,
            .foregroundColor: NSColor.fawxTextSelectionForeground,
        ]
    }
}
#else
extension FawxAccentContrastScheme {
    init(traits: UITraitCollection) {
        self = traits.userInterfaceStyle == .dark ? .dark : .light
    }
}
#endif

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
