import SwiftUI

struct BottomSearchBar: View {
    @Binding var text: String
    let prompt: String
    var accessibilityIdentifier: String?
    var includesContainerChrome = true

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(Color.fawxTextSecondary)

            TextField(prompt, text: $text)
                .autocorrectionDisabled()

            if text.isEmpty == false {
                Button {
                    text = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Color.fawxTextSecondary)
                }
                .buttonStyle(.plain)
            }
        }
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxText)
        .padding(.horizontal, FawxSpacing.paddingLG)
        .padding(.vertical, FawxSpacing.paddingMD)
        .background(
            Capsule()
                .fill(Color.fawxSurface)
        )
        .overlay(
            Capsule()
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .modifier(BottomSearchContainerChrome(isEnabled: includesContainerChrome))
        .accessibilityIdentifier(accessibilityIdentifier ?? "bottomSearchBar")
    }
}

private struct BottomSearchContainerChrome: ViewModifier {
    let isEnabled: Bool

    func body(content: Content) -> some View {
        if isEnabled {
            content
                .padding(.horizontal, FawxSpacing.paddingLG)
                .padding(.top, FawxSpacing.paddingSM)
                .padding(.bottom, FawxSpacing.paddingMD)
                .background(Color.fawxBackground.opacity(0.96))
        } else {
            content
        }
    }
}
