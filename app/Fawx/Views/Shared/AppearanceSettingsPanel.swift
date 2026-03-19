import Observation
import SwiftUI

struct AppearanceSettingsPanel: View {
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
                    .background(Color.fawxSurface)
                    .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
                    .overlay(
                        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                            .stroke(Color.fawxBorder, lineWidth: 1)
                    )
            }
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
}
