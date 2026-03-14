import Observation
import SwiftUI

struct AppearanceSettingsPanel: View {
    @AppStorage("theme") private var storedThemeRawValue = AppTheme.system.rawValue
    @AppStorage("font_size") private var storedFontSizeRawValue = AppFontSize.medium.rawValue

    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            Picker("Theme", selection: themeBinding) {
                ForEach(AppTheme.allCases, id: \.self) { theme in
                    Text(theme.displayName).tag(theme)
                }
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                HStack {
                    Text("Font Size")
                    Spacer()
                    Text(currentFontSize.displayName)
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
                    .font(.system(size: 14 * currentFontSize.scale, weight: .regular))
                    .foregroundStyle(Color.fawxText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingMD)
                    .background(Color.fawxSurface)
                    .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
                    .overlay(
                        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                            .stroke(Color.fawxBorder, lineWidth: 1)
                    )
            }
        }
        .onAppear {
            appState.setTheme(currentTheme)
            appState.setFontSize(currentFontSize)
        }
    }

    private var currentTheme: AppTheme {
        AppTheme(rawValue: storedThemeRawValue) ?? .system
    }

    private var currentFontSize: AppFontSize {
        AppFontSize(rawValue: storedFontSizeRawValue) ?? .medium
    }

    private var themeBinding: Binding<AppTheme> {
        Binding(
            get: { currentTheme },
            set: { newValue in
                storedThemeRawValue = newValue.rawValue
                appState.setTheme(newValue)
            }
        )
    }

    private var fontSizeSliderBinding: Binding<Double> {
        Binding(
            get: { currentFontSize.sliderValue },
            set: { newValue in
                let selectedFontSize = AppFontSize.fromSliderValue(newValue)
                storedFontSizeRawValue = selectedFontSize.rawValue
                appState.setFontSize(selectedFontSize)
            }
        )
    }
}
