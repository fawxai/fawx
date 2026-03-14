import SwiftUI

struct AuthStatusList: View {
    let providers: [AuthProvider]
    let errorMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            if let errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxError)
            }

            if providers.isEmpty {
                Text("No authentication configured. Run `fawx setup` on your server.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                ForEach(providers) { provider in
                    AuthProviderCard(provider: provider)
                }
            }
        }
    }
}

private struct AuthProviderCard: View {
    let provider: AuthProvider

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                Text(provider.displayName)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: FawxSpacing.paddingMD)

                Text(provider.displayStatus)
                    .font(FawxTypography.status)
                    .foregroundStyle(provider.isConfigured ? Color.fawxSuccess : Color.fawxWarning)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, 4)
                    .background((provider.isConfigured ? Color.fawxSuccess : Color.fawxWarning).opacity(0.12))
                    .clipShape(Capsule())
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                Label("\(provider.modelCount) models", systemImage: "cube.box")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Label(provider.authMethodsSummary, systemImage: "key")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
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
