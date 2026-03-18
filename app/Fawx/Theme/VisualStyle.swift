import SwiftUI

enum FawxOpacity {
    static let surfaceStrong = 0.98
    static let surfaceOverlay = 0.96
    static let surfaceMuted = 0.94
    static let borderEmphasis = 0.9
    static let borderStrong = 0.85
    static let borderMedium = 0.8
    static let borderSubtle = 0.7
    static let accentBorder = 0.2
    static let warningBorder = 0.28
    static let fillSubtle = 0.12
    static let fillMuted = 0.08
    static let errorFill = 0.06
    static let errorBorder = 0.25
    static let borderHighlight = 0.3
    static let codeBackground = 0.9
    static let backgroundScrim = 0.86
    static let iconSecondary = 0.35
    static let shadowLight = 0.08
    static let shadowStrong = 0.14
}

struct FawxShadowStyle {
    let color: Color
    let radius: CGFloat
    let x: CGFloat
    let y: CGFloat

    init(color: Color, radius: CGFloat, x: CGFloat = 0, y: CGFloat = 0) {
        self.color = color
        self.radius = radius
        self.x = x
        self.y = y
    }
}

enum FawxShadow {
    static let floatingPanel = FawxShadowStyle(
        color: .black.opacity(FawxOpacity.shadowLight),
        radius: 12,
        y: 4
    )
    static let elevatedCapsule = FawxShadowStyle(
        color: .black.opacity(FawxOpacity.shadowLight),
        radius: 6,
        y: 2
    )
    static let loadingOverlay = FawxShadowStyle(
        color: .black.opacity(FawxOpacity.shadowStrong),
        radius: 12,
        y: 4
    )
}

private struct FawxShadowModifier: ViewModifier {
    let style: FawxShadowStyle

    func body(content: Content) -> some View {
        content.shadow(color: style.color, radius: style.radius, x: style.x, y: style.y)
    }
}

extension View {
    func fawxShadow(_ style: FawxShadowStyle) -> some View {
        modifier(FawxShadowModifier(style: style))
    }

    func fawxOpaqueTintedSurface<S: Shape>(
        _ shape: S,
        tint: Color,
        tintOpacity: Double = FawxOpacity.fillSubtle
    ) -> some View {
        background {
            shape
                .fill(Color.fawxBackground)
                .overlay {
                    shape.fill(tint.opacity(tintOpacity))
                }
        }
    }

    @ViewBuilder
    func fawxOpaqueModalPresentation() -> some View {
        self
            .presentationBackground(Color.fawxBackground)
    }
}
