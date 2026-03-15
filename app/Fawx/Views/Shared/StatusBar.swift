import SwiftUI

struct StatusBar: View {
    let connectionStatus: ConnectionStatus
    let permissionPreset: String
    let modelName: String?
    let context: ContextInfo?
    let selectedSessionMessageCount: Int

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Label {
                Text(connectionLabel)
                    .font(FawxTypography.status)
            } icon: {
                Circle()
                    .fill(connectionColor)
                    .frame(width: 8, height: 8)
            }
            .accessibilityIdentifier("connectionIndicator")

            Divider()

            Text(permissionPreset)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(1)

            Divider()

            Text(modelName.map { compactModelName($0, limit: 28) } ?? "Server model unavailable")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(1)
                .accessibilityIdentifier("modelLabel")
                .help(modelName.map { abbreviateModelName($0) } ?? "Server model unavailable")

            Spacer()

            if let displayContext = displayContext {
                HStack(spacing: FawxSpacing.paddingSM) {
                    ContextProgressBar(percentage: displayContext.percentage)
                        .frame(width: 42, height: 6)

                    Text("\(Int(displayContext.percentage.rounded()))% ctx")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .accessibilityIdentifier("contextLabel")
                }
                .help("\(displayContext.usedTokens) / \(displayContext.maxTokens) tokens")
            } else {
                Text("—")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .accessibilityIdentifier("contextLabel")
            }
        }
        .padding(.horizontal, FawxSpacing.paddingLG)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(Color.fawxBorder)
                .frame(height: 1)
        }
    }

    private var connectionLabel: String {
        switch connectionStatus {
        case .connected:
            return "Connected"
        case .connecting:
            return "Connecting"
        case .reconnecting:
            return "Reconnecting"
        case .disconnected:
            return "Disconnected"
        }
    }

    private var connectionColor: Color {
        switch connectionStatus {
        case .connected:
            return .fawxSuccess
        case .connecting, .reconnecting:
            return .fawxWarning
        case .disconnected:
            return .fawxError
        }
    }

    private var displayContext: DisplayContext? {
        guard let context else {
            return nil
        }

        return DisplayContext(
            percentage: context.normalizedPercentage,
            usedTokens: context.usedTokens,
            maxTokens: context.maxTokens
        )
    }
}

private struct DisplayContext {
    let percentage: Double
    let usedTokens: Int
    let maxTokens: Int
}

private struct ContextProgressBar: View {
    let percentage: Double

    var body: some View {
        GeometryReader { proxy in
            let clamped = min(max(percentage, 0), 100)
            let width = proxy.size.width * (clamped / 100)

            ZStack(alignment: .leading) {
                Capsule()
                    .fill(Color.fawxBorder)

                Capsule()
                    .fill(progressColor)
                    .frame(width: width)
            }
        }
    }

    private var progressColor: Color {
        switch percentage {
        case ..<60:
            return .fawxSuccess
        case ..<85:
            return .fawxWarning
        default:
            return .fawxError
        }
    }
}
