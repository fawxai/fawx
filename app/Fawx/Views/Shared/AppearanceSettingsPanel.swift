import Observation
import SwiftUI

struct AppearanceSettingsPanel: View {
    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            ThemeSelectionControl(selection: themeBinding)

            accentColorSection

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                HStack {
                    Text("Font Size")
                    Spacer()
                    Text(appState.fontSize.displayName)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Slider(value: fontSizeSliderBinding, in: 0 ... 2, step: 1)
                    .accessibilityIdentifier("fontSizeSlider")

                HStack {
                    Text("Small")
                    Spacer()
                    Text("Medium")
                    Spacer()
                    Text("Large")
                }
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

                Text("Preview the chat UI at your preferred reading size.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Text("The quick brown fox jumps over the lazy dog.")
                    .font(.system(size: 14 * appState.fontSize.scale, weight: .regular))
                    .foregroundStyle(Color.fawxText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingMD)
                    .fawxSurface(.field)
            }
        }
    }

    private var accentColorSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .center, spacing: FawxSpacing.paddingMD) {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(appState.accentColor.color)
                    .frame(width: 44, height: 44)
                    .overlay {
                        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                            .stroke(Color.fawxBorder.opacity(0.65), lineWidth: 1)
                    }
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Accent Color")
                        .font(FawxTypography.heading2)
                        .foregroundStyle(Color.fawxText)

                    Text(appState.accentColor.hexString)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)

                Button("Reset") {
                    appState.setAccentColor(.default)
                }
                .buttonStyle(.plain)
                .foregroundStyle(Color.fawxTextSecondary)
                .disabled(appState.accentColor == .default)
            }

            accentSlider(title: "Red", value: accentChannelBinding(.red), byteValue: appState.accentColor.redByte)
            accentSlider(
                title: "Green",
                value: accentChannelBinding(.green),
                byteValue: appState.accentColor.greenByte
            )
            accentSlider(title: "Blue", value: accentChannelBinding(.blue), byteValue: appState.accentColor.blueByte)

            Text("Saves the exact color; Fawx renders contrast-safe variants across light and dark themes.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingMD)
        .fawxSurface(.field)
        .accessibilityIdentifier("accentColorPalette")
    }

    private func accentSlider(
        title: String,
        value: Binding<Double>,
        byteValue: Int
    ) -> some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 44, alignment: .leading)

            AccentChannelSlider(title: title, value: value, tint: Color.fawxAccent)
                .accessibilityIdentifier("accent\(title)Slider")

            Text("\(byteValue)")
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 32, alignment: .trailing)
        }
    }

    private var themeBinding: Binding<AppTheme> {
        Binding(
            get: { appState.theme },
            set: { newValue in
                appState.setTheme(newValue)
            }
        )
    }

    private var fontSizeSliderBinding: Binding<Double> {
        Binding(
            get: { appState.fontSize.sliderValue },
            set: { newValue in
                let selectedFontSize = AppFontSize.fromSliderValue(newValue)
                appState.setFontSize(selectedFontSize)
            }
        )
    }

    private func accentChannelBinding(_ channel: AppAccentColor.Channel) -> Binding<Double> {
        Binding(
            get: {
                switch channel {
                case .red:
                    return Double(appState.accentColor.redByte)
                case .green:
                    return Double(appState.accentColor.greenByte)
                case .blue:
                    return Double(appState.accentColor.blueByte)
                }
            },
            set: { newValue in
                appState.setAccentColor(appState.accentColor.updating(channel, byteValue: newValue))
            }
        )
    }
}

private struct ThemeSelectionControl: View {
    @Binding var selection: AppTheme

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .firstTextBaseline) {
                Text("Theme")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: FawxSpacing.paddingMD)

                Text(selection.displayName)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            HStack(spacing: 0) {
                ForEach(AppTheme.allCases, id: \.self) { theme in
                    if theme != AppTheme.allCases.first {
                        Rectangle()
                            .fill(Color.fawxText.opacity(0.1))
                            .frame(width: 1)
                            .padding(.vertical, FawxSpacing.paddingXS)
                    }

                    ThemeSelectionSegment(
                        theme: theme,
                        isSelected: selection == theme
                    ) {
                        selection = theme
                    }
                }
            }
            .padding(2)
            .background(Color.fawxText.opacity(0.04))
            .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM))
            .overlay {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM)
                    .stroke(Color.fawxText.opacity(0.14), lineWidth: 1)
            }
        }
        .accessibilityIdentifier("themeSelectionControl")
    }
}

private struct ThemeSelectionSegment: View {
    let theme: AppTheme
    let isSelected: Bool
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            HStack(spacing: FawxSpacing.paddingXS) {
                Image(systemName: iconName)
                    .font(.system(size: 11, weight: .semibold))

                Text(theme.displayName)
                    .font(FawxTypography.status)
            }
            .foregroundStyle(isSelected ? Color.fawxAccent : Color.fawxTextSecondary)
            .frame(maxWidth: .infinity)
            .padding(.vertical, FawxSpacing.paddingXS)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM - 2)
                .fill(isSelected ? Color.fawxAccentSubtle : Color.clear)
        )
        .accessibilityIdentifier("themeSelection\(theme.displayName)Button")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private var iconName: String {
        switch theme {
        case .system:
            return "circle.lefthalf.filled"
        case .light:
            return "sun.max"
        case .dark:
            return "moon"
        }
    }
}

private struct AccentChannelSlider: View {
    let title: String
    @Binding var value: Double
    let tint: Color

    var body: some View {
        GeometryReader { proxy in
            let clampedValue = min(max(value, 0), 255)
            let availableWidth = max(proxy.size.width, 1)
            let fillWidth = availableWidth * CGFloat(clampedValue / 255)

            ZStack(alignment: .leading) {
                Rectangle()
                    .fill(Color.fawxText.opacity(0.12))
                    .frame(height: 3)

                Rectangle()
                    .fill(tint)
                    .frame(width: fillWidth, height: 3)

                Circle()
                    .fill(Color.fawxText.opacity(0.88))
                    .frame(width: 12, height: 12)
                    .offset(x: min(max(fillWidth - 6, 0), max(availableWidth - 12, 0)))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { gesture in
                        value = Double(min(max(gesture.location.x / availableWidth, 0), 1) * 255)
                    }
            )
            .accessibilityElement()
            .accessibilityLabel("\(title) accent channel")
            .accessibilityValue("\(Int(clampedValue.rounded()))")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment:
                    value = min(value + 1, 255)
                case .decrement:
                    value = max(value - 1, 0)
                @unknown default:
                    break
                }
            }
        }
        .frame(height: 24)
    }
}
