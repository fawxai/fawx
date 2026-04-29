import SwiftUI

struct QueuedMessageChip: View {
    let text: String
    let isSteering: Bool
    let canSteer: Bool
    let toggleSteering: () -> Void
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Label(
                isSteering ? "Steering" : "Queued",
                systemImage: isSteering ? "arrow.turn.up.right" : "clock"
            )
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            Text(text)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxText)
                .lineLimit(1)

            Spacer()

            if canSteer {
                Button(isSteering ? "Queue" : "Steer", action: toggleSteering)
                    .buttonStyle(.plain)
                    .font(FawxTypography.status.weight(.semibold))
                    .foregroundStyle(Color.fawxTextSecondary)
                    .accessibilityIdentifier("queuedMessageSteerToggle")
                    .accessibilityLabel(isSteering ? "Queue this message" : "Steer current response")
            }

            Button(action: dismiss) {
                Image(systemName: "trash")
            }
            .buttonStyle(.plain)
            .foregroundStyle(Color.fawxTextSecondary)
            .accessibilityLabel("Cancel queued message")
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurfaceHover)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(isSteering ? "Steering message" : "Queued message")
        .accessibilityValue(text)
    }
}
