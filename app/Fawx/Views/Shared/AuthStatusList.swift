import AuthenticationServices
import Observation
import SwiftUI

struct AuthStatusList: View {
    @Bindable var appState: AppState
    @State private var oauthCoordinator = OAuthSessionCoordinator()
    @State private var activeOAuthProvider: String?
    @State private var localErrorMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            if let errorMessage = displayedErrorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxError)
            }

            if appState.authProviders.isEmpty {
                Text("No authentication configured. Run `fawx setup` on your server.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                ForEach(appState.authProviders) { provider in
                    AuthProviderCard(
                        provider: provider,
                        isAuthenticating: activeOAuthProvider == provider.provider.lowercased(),
                        startOAuth: provider.provider.lowercased() == "openai"
                            ? { await startOAuthLogin(provider: provider.provider) }
                            : nil
                    )
                }
            }
        }
    }

    private var displayedErrorMessage: String? {
        localErrorMessage ?? appState.authProvidersError
    }

    private func startOAuthLogin(
        provider: String
    ) async {
        guard activeOAuthProvider == nil else {
            return
        }

        activeOAuthProvider = provider.lowercased()
        defer { activeOAuthProvider = nil }

        do {
            let startResponse = try await appState.client.oauthStart(provider: provider)
            guard let authorizeURL = URL(string: startResponse.authorizeUrl) else {
                throw APIError.invalidURL(startResponse.authorizeUrl)
            }
            guard let providerRedirectURL = URL(string: startResponse.redirectUri) else {
                throw APIError.invalidURL(startResponse.redirectUri)
            }
            guard let nativeCallbackURL = URL(string: "fawx-auth://\(provider.lowercased())/callback") else {
                throw APIError.invalidResponse
            }

            let callbackURL = try await oauthCoordinator.authenticate(
                authorizeURL: authorizeURL,
                providerRedirectURL: providerRedirectURL,
                callbackURL: nativeCallbackURL
            )

            if
                let components = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false),
                let oauthError = components.queryItems?.first(where: { $0.name == "error" })?.value
            {
                let description = components.queryItems?
                    .first(where: { $0.name == "error_description" })?
                    .value
                throw APIError.streamError(description ?? oauthError)
            }

            guard
                let components = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false),
                let code = components.queryItems?.first(where: { $0.name == "code" })?.value,
                !code.isEmpty
            else {
                throw APIError.decoding("Missing authorization code")
            }

            let response = try await appState.client.oauthCallback(
                provider: provider,
                code: code,
                flowToken: startResponse.flowToken
            )

            await appState.refreshSettingsState()
            localErrorMessage = nil
            appState.showToast(
                message: response.verified
                    ? "ChatGPT connected."
                    : "\(providerDisplayName(provider)) sign-in needs verification.",
                style: response.verified ? .success : .warning
            )
        } catch {
            if isOAuthCancellation(error) {
                localErrorMessage = "Sign-in cancelled."
                appState.showToast(message: "Sign-in cancelled.", style: .info)
                return
            }

            localErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func isOAuthCancellation(_ error: Error) -> Bool {
        let nsError = error as NSError
        return nsError.domain == ASWebAuthenticationSessionErrorDomain
            && nsError.code == ASWebAuthenticationSessionError.canceledLogin.rawValue
    }

    private func providerDisplayName(_ provider: String) -> String {
        switch provider.lowercased() {
        case "openai":
            "OpenAI"
        case "anthropic":
            "Anthropic"
        case "google":
            "Google"
        case "openrouter":
            "OpenRouter"
        default:
            provider
                .replacingOccurrences(of: "-", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }
    }
}

private struct AuthProviderCard: View {
    let provider: AuthProvider
    let isAuthenticating: Bool
    let startOAuth: (() async -> Void)?

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

            if let startOAuth {
                Button(isAuthenticating ? "Connecting..." : oauthButtonTitle) {
                    Task {
                        await startOAuth()
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isAuthenticating)
                .accessibilityLabel(oauthButtonTitle)
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
        .accessibilityIdentifier("authProvider_\(provider.id)")
    }

    private var oauthButtonTitle: String {
        provider.isConfigured ? "Reconnect ChatGPT" : "Sign in with ChatGPT"
    }
}
