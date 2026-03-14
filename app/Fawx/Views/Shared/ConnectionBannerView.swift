import SwiftUI

struct ConnectionBannerView: View {
    let banner: ConnectionBannerState
    let retryAction: () -> Void

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: banner.tone == .warning ? "exclamationmark.triangle.fill" : "wifi.slash")
                .foregroundStyle(iconColor)

            Text(banner.message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .frame(maxWidth: .infinity, alignment: .leading)

            if banner.showsRetry {
                Button("Retry", action: retryAction)
                    .buttonStyle(.borderedProminent)
                    .tint(.fawxAccent)
            }
        }
        .padding(.horizontal, FawxSpacing.paddingLG)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(backgroundColor)
        .overlay(
            Rectangle()
                .fill(borderColor)
                .frame(height: 1),
            alignment: .bottom
        )
    }

    private var backgroundColor: Color {
        switch banner.tone {
        case .warning:
            return Color.fawxWarning.opacity(0.12)
        case .error:
            return Color.fawxError.opacity(0.12)
        }
    }

    private var borderColor: Color {
        switch banner.tone {
        case .warning:
            return Color.fawxWarning.opacity(0.35)
        case .error:
            return Color.fawxError.opacity(0.35)
        }
    }

    private var iconColor: Color {
        switch banner.tone {
        case .warning:
            return .fawxWarning
        case .error:
            return .fawxError
        }
    }
}
