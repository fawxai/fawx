import SwiftUI

struct QueuedMessageChip: View {
    let text: String
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Label("Queued", systemImage: "clock")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxAccent)

            Text(text)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxText)
                .lineLimit(1)

            Spacer()

            Button(action: dismiss) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.plain)
            .foregroundStyle(Color.fawxTextSecondary)
            .accessibilityLabel("Dismiss queued message")
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxAccentSubtle)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxAccent.opacity(0.3), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Queued message")
        .accessibilityValue(text)
    }
}
