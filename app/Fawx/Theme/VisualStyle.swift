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
    static let shadowStrong = 0.1
}

enum FawxSurfaceRole {
    case page
    case rail
    case section
    case composer
    case field
    case transient
    case callout
    case code

    var backgroundColor: Color {
        switch self {
        case .page, .rail, .section, .composer, .field, .transient, .callout:
            return .fawxBackground
        case .code:
            return .fawxCode.opacity(FawxOpacity.codeBackground)
        }
    }

    var borderColor: Color? {
        switch self {
        case .page, .rail, .section, .composer, .field, .transient, .callout, .code:
            return nil
        }
    }

    var cornerRadius: CGFloat? {
        switch self {
        case .field, .transient, .callout, .code:
            return FawxSpacing.cornerRadius
        case .page, .rail, .section, .composer:
            return nil
        }
    }
}

enum FawxRowSelectionStyle {
    case fill
    case accentOnly
}

enum FawxDropdownRowRole {
    case normal
    case destructive
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
        radius: 8,
        y: 3
    )
    static let elevatedCapsule = FawxShadowStyle(
        color: .black.opacity(FawxOpacity.shadowLight),
        radius: 3,
        y: 1
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

private struct FawxSurfaceModifier: ViewModifier {
    let role: FawxSurfaceRole

    @ViewBuilder
    func body(content: Content) -> some View {
        if let cornerRadius = role.cornerRadius {
            content
                .background(role.backgroundColor)
                .clipShape(RoundedRectangle(cornerRadius: cornerRadius))
                .overlay {
                    if let borderColor = role.borderColor {
                        RoundedRectangle(cornerRadius: cornerRadius)
                            .stroke(borderColor, lineWidth: 1)
                    }
                }
        } else {
            content
                .background(role.backgroundColor)
        }
    }
}

private struct FawxTransientSurfaceModifier: ViewModifier {
    let borderColor: Color?
    let shadowStyle: FawxShadowStyle?

    @ViewBuilder
    func body(content: Content) -> some View {
        let card = content
            .background(FawxSurfaceRole.transient.backgroundColor)
            .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
            .overlay {
                if let borderColor = borderColor ?? FawxSurfaceRole.transient.borderColor {
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                        .stroke(borderColor, lineWidth: 1)
                }
            }

        if let shadowStyle {
            card.fawxShadow(shadowStyle)
        } else {
            card
        }
    }
}

private struct FawxRowChromeModifier: ViewModifier {
    let isSelected: Bool
    let isHovering: Bool
    let selectionStyle: FawxRowSelectionStyle
    let cornerRadius: CGFloat

    func body(content: Content) -> some View {
        content
            .background(rowBackground)
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius))
    }

    private var rowBackground: Color {
        if selectionStyle == .accentOnly {
            return .clear
        }
        if isSelected, selectionStyle == .fill {
            return .fawxAccentSubtle.opacity(0.72)
        }
        if isHovering {
            return .fawxSurfaceHover.opacity(0.55)
        }
        return .clear
    }
}

extension View {
    func fawxSurface(_ role: FawxSurfaceRole) -> some View {
        modifier(FawxSurfaceModifier(role: role))
    }

    func fawxTransientSurface(
        borderColor: Color? = nil,
        shadowStyle: FawxShadowStyle? = nil
    ) -> some View {
        modifier(FawxTransientSurfaceModifier(borderColor: borderColor, shadowStyle: shadowStyle))
    }

    func fawxRowChrome(
        isSelected: Bool = false,
        isHovering: Bool = false,
        selectionStyle: FawxRowSelectionStyle = .fill,
        cornerRadius: CGFloat = FawxSpacing.cornerRadius
    ) -> some View {
        modifier(
            FawxRowChromeModifier(
                isSelected: isSelected,
                isHovering: isHovering,
                selectionStyle: selectionStyle,
                cornerRadius: cornerRadius
            )
        )
    }

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

struct FawxDropdownMenu<Label: View, Content: View>: View {
    private let minWidth: CGFloat
    private let maxHeight: CGFloat?
    private let label: () -> Label
    private let content: (_ dismiss: @escaping () -> Void) -> Content

    @State private var isPresented = false

    init(
        minWidth: CGFloat = 180,
        maxHeight: CGFloat? = 360,
        @ViewBuilder label: @escaping () -> Label,
        @ViewBuilder content: @escaping (_ dismiss: @escaping () -> Void) -> Content
    ) {
        self.minWidth = minWidth
        self.maxHeight = maxHeight
        self.label = label
        self.content = content
    }

    var body: some View {
        Button {
            isPresented.toggle()
        } label: {
            label()
        }
        .buttonStyle(.plain)
        .popover(isPresented: $isPresented, arrowEdge: .top) {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    content(dismiss)
                }
                .padding(FawxSpacing.paddingXS)
                .frame(minWidth: minWidth, alignment: .leading)
            }
            .scrollIndicators(.automatic)
            .frame(minWidth: minWidth, maxHeight: maxHeight)
            .fawxSurface(.transient)
        }
    }

    private func dismiss() {
        isPresented = false
    }
}

struct FawxDropdownActionRow: View {
    let title: String
    var systemImage: String?
    var isSelected = false
    var isEnabled = true
    var role = FawxDropdownRowRole.normal
    let action: () -> Void

    @State private var isHovering = false

    var body: some View {
        Button {
            guard isEnabled else {
                return
            }
            action()
        } label: {
            HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 12, weight: .semibold))
                        .frame(width: 14)
                }

                Text(title)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if isSelected {
                    Image(systemName: "checkmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(Color.fawxAccent)
                }
            }
            .font(FawxTypography.status)
            .foregroundStyle(foregroundColor)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(RoundedRectangle(cornerRadius: 8))
            .fawxRowChrome(
                isSelected: isSelected,
                isHovering: isHovering,
                cornerRadius: 8
            )
        }
        .buttonStyle(.plain)
        .disabled(!isEnabled)
        .opacity(isEnabled ? 1 : 0.45)
#if os(macOS)
        .onHover { isHovering = $0 }
#endif
    }

    private var foregroundColor: Color {
        if role == .destructive {
            return .fawxError
        }
        if isSelected {
            return .fawxText
        }
        return .fawxTextSecondary
    }
}

struct FawxDropdownInfoRow: View {
    let title: String
    var systemImage: String?

    var body: some View {
        HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: 12, weight: .semibold))
                    .frame(width: 14)
            }

            Text(title)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, 6)
    }
}

struct FawxDropdownSectionHeader: View {
    let title: String

    var body: some View {
        Text(title)
            .font(FawxTypography.status)
            .foregroundStyle(Color.fawxTextSecondary)
            .textCase(.uppercase)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.top, FawxSpacing.paddingXS)
            .padding(.bottom, 2)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct FawxDropdownDivider: View {
    var body: some View {
        Divider()
            .overlay(Color.fawxBorder)
            .padding(.vertical, FawxSpacing.paddingXS)
    }
}
