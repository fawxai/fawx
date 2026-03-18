import AuthenticationServices
import Observation
import SwiftUI

struct AuthStatusList: View {
    @Bindable var appState: AppState
    @State private var oauthCoordinator = OAuthSessionCoordinator()
    @State private var activeOAuthProvider: String?
    @State private var isPresentingProviderEditor = false
    @State private var selectedProviderForEditor: SetupProvider = .openai
    @State private var localErrorMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack {
                Spacer(minLength: 0)

                Button("Add Provider") {
                    openProviderEditor()
                }
                .buttonStyle(.bordered)
            }

            if let errorMessage = displayedErrorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxError)
            }

            if appState.authProviders.isEmpty {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    Text("No authentication configured yet.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxText)

                    Text("Add Claude or ChatGPT credentials here instead of dropping to setup commands.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            } else {
                ForEach(appState.authProviders) { provider in
                    AuthProviderCard(
                        provider: provider,
                        isAuthenticating: activeOAuthProvider == provider.provider.lowercased(),
                        startOAuth: provider.provider.lowercased() == "openai"
                            ? { await startOAuthLogin(provider: provider.provider) }
                            : nil,
                        manageProvider: setupProvider(for: provider.provider).map { setupProvider in
                            {
                                openProviderEditor(setupProvider)
                            }
                        },
                        verifyProvider: provider.isConfigured
                            ? { await verifyConfiguredProvider(provider.provider) }
                            : nil,
                        removeProvider: provider.isConfigured
                            ? { await removeConfiguredProvider(provider.provider) }
                            : nil
                    )
                }
            }
        }
        .sheet(isPresented: $isPresentingProviderEditor) {
            NavigationStack {
                ProviderManagementSheet(
                    appState: appState,
                    initialProvider: selectedProviderForEditor
                )
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

    private func openProviderEditor(_ provider: SetupProvider? = nil) {
        selectedProviderForEditor = provider ?? preferredProviderForEditor
        isPresentingProviderEditor = true
    }

    private var preferredProviderForEditor: SetupProvider {
        let configuredProviders = Set(appState.authProviders.filter(\.isConfigured).map { $0.provider.lowercased() })
        if !configuredProviders.contains(SetupProvider.openai.providerID) {
            return .openai
        }
        if !configuredProviders.contains(SetupProvider.anthropic.providerID) {
            return .anthropic
        }
        return .openai
    }

    private func setupProvider(for provider: String) -> SetupProvider? {
        switch provider.lowercased() {
        case SetupProvider.openai.providerID:
            .openai
        case SetupProvider.anthropic.providerID:
            .anthropic
        default:
            nil
        }
    }

    private func verifyConfiguredProvider(_ provider: String) async {
        do {
            let response = try await appState.verifyProvider(provider)
            localErrorMessage = nil
            appState.showToast(
                message: response.message,
                style: response.verified ? .success : .warning
            )
        } catch {
            localErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func removeConfiguredProvider(_ provider: String) async {
        do {
            try await appState.deleteProvider(provider)
            localErrorMessage = nil
            appState.showToast(
                message: "Removed \(providerDisplayName(provider)).",
                style: .info
            )
        } catch {
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
    let manageProvider: (() -> Void)?
    let verifyProvider: (() async -> Void)?
    let removeProvider: (() async -> Void)?

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

            HStack(spacing: FawxSpacing.paddingMD) {
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

                if let manageProvider {
                    Button(provider.isConfigured ? "Update" : "Configure") {
                        manageProvider()
                    }
                    .buttonStyle(.bordered)
                }

                if let verifyProvider {
                    Button("Verify") {
                        Task {
                            await verifyProvider()
                        }
                    }
                    .buttonStyle(.bordered)
                }

                if let removeProvider {
                    Button("Remove", role: .destructive) {
                        Task {
                            await removeProvider()
                        }
                    }
                    .buttonStyle(.bordered)
                }
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

private struct ProviderManagementSheet: View {
    @Environment(\.dismiss) private var dismiss

    let appState: AppState
    let initialProvider: SetupProvider

    @State private var selectedProvider: SetupProvider
    @State private var selectedAuthMethod: SetupProviderAuthMethod
    @State private var credentialInput = ""
    @State private var configuredProviderIDs: Set<String>
    @State private var isSubmitting = false
    @State private var statusKind: ConnectionTestKind = .idle
    @State private var statusMessage: String?
    @State private var oauthCoordinator = OAuthSessionCoordinator()

    init(appState: AppState, initialProvider: SetupProvider) {
        self.appState = appState
        self.initialProvider = initialProvider
        _selectedProvider = State(initialValue: initialProvider)
        _selectedAuthMethod = State(initialValue: .subscription)
        _configuredProviderIDs = State(
            initialValue: Set(appState.authProviders.filter(\.isConfigured).map { $0.provider.lowercased() })
        )
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                Text("Connect, update, verify, or remove provider credentials for this server.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .fixedSize(horizontal: false, vertical: true)

                settingsBlock {
                    Text("Provider")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    Picker("Provider", selection: $selectedProvider) {
                        ForEach(SetupProvider.allCases) { provider in
                            Text(provider.displayName).tag(provider)
                        }
                    }
                    .pickerStyle(.segmented)
                }

                settingsBlock {
                    Text("Authentication")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    Picker("Authentication", selection: $selectedAuthMethod) {
                        ForEach(SetupProviderAuthMethod.allCases) { method in
                            Text(method.title).tag(method)
                        }
                    }
                    .pickerStyle(.segmented)
                }

                settingsBlock {
                    providerInstructions

                    if showsCredentialInput {
                        SecureField(providerFieldPrompt, text: $credentialInput)
                            .textFieldStyle(.roundedBorder)
                    }

                    actionRow
                }

                SetupStatusMessageView(kind: statusKind, message: statusMessage)
            }
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground)
        .navigationTitle("Manage Provider")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Done") {
                    dismiss()
                }
            }
        }
        .frame(minWidth: 460, minHeight: 420)
    }

    private var isConfigured: Bool {
        configuredProviderIDs.contains(selectedProvider.providerID)
    }

    private var usesOAuthSubscriptionFlow: Bool {
        selectedProvider == .openai && selectedAuthMethod == .subscription
    }

    private var showsCredentialInput: Bool {
        !usesOAuthSubscriptionFlow
    }

    @ViewBuilder
    private var providerInstructions: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if selectedProvider == .anthropic && selectedAuthMethod == .subscription {
                Text("Run `claude setup-token` in Terminal to start Anthropic sign-in and paste the returned setup token here.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else if selectedProvider == .openai && selectedAuthMethod == .subscription {
                Text("Use your ChatGPT subscription to sign in with OpenAI.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)

                Text("Fawx opens ChatGPT sign-in in a secure browser sheet and completes setup automatically.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                Text("Paste the credential you want this server to use for \(selectedProvider.displayName).")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
    }

    private var actionRow: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            if usesOAuthSubscriptionFlow {
                Button(isSubmitting ? "Connecting..." : (isConfigured ? "Reconnect ChatGPT" : "Sign in with ChatGPT")) {
                    Task {
                        await startOpenAIOAuth()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(isSubmitting)
            } else {
                Button(isSubmitting ? "Saving..." : saveButtonTitle) {
                    Task {
                        await saveCredentials()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(isSubmitting)
            }

            if isConfigured {
                Button("Verify") {
                    Task {
                        await verifySelectedProvider()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(isSubmitting)

                Button("Remove", role: .destructive) {
                    Task {
                        await removeSelectedProvider()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(isSubmitting)
            }
        }
    }

    private var saveButtonTitle: String {
        switch (selectedProvider, selectedAuthMethod) {
        case (.anthropic, .subscription):
            "Save Setup Token"
        case (_, .apiKey):
            "Save API Key"
        case (.openai, .subscription):
            "Sign in with ChatGPT"
        }
    }

    private var providerFieldPrompt: String {
        switch (selectedProvider, selectedAuthMethod) {
        case (.anthropic, .subscription):
            "Paste the Anthropic setup token"
        case (.anthropic, .apiKey):
            "Paste your Anthropic API key"
        case (.openai, _):
            "Paste your OpenAI API key"
        }
    }

    private func settingsBlock<Content: View>(
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            content()
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func saveCredentials() async {
        let trimmedCredential = credentialInput.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedCredential.isEmpty else {
            statusKind = .failure
            statusMessage = "Enter a credential before saving."
            return
        }

        isSubmitting = true
        defer { isSubmitting = false }

        do {
            let response: ProviderAuthActionResponse
            switch (selectedProvider, selectedAuthMethod) {
            case (.anthropic, .subscription):
                response = try await appState.storeAnthropicSetupToken(trimmedCredential)
            case (_, .apiKey):
                response = try await appState.storeProviderAPIKey(
                    provider: selectedProvider.providerID,
                    apiKey: trimmedCredential
                )
            case (.openai, .subscription):
                return
            }

            configuredProviderIDs.insert(selectedProvider.providerID)
            credentialInput = ""
            statusKind = response.verified ? .success : .warning
            statusMessage = response.verified
                ? "\(selectedProvider.displayName) is ready."
                : "\(selectedProvider.displayName) was saved. Verification may still be required."
            appState.showToast(
                message: response.verified
                    ? "\(selectedProvider.displayName) updated."
                    : "\(selectedProvider.displayName) saved.",
                style: response.verified ? .success : .warning
            )
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func verifySelectedProvider() async {
        isSubmitting = true
        defer { isSubmitting = false }

        do {
            let response = try await appState.verifyProvider(selectedProvider.providerID)
            statusKind = response.verified ? .success : .warning
            statusMessage = response.message
            appState.showToast(
                message: response.message,
                style: response.verified ? .success : .warning
            )
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func removeSelectedProvider() async {
        isSubmitting = true
        defer { isSubmitting = false }

        do {
            try await appState.deleteProvider(selectedProvider.providerID)
            configuredProviderIDs.remove(selectedProvider.providerID)
            credentialInput = ""
            statusKind = .success
            statusMessage = "\(selectedProvider.displayName) removed."
            appState.showToast(
                message: "Removed \(selectedProvider.displayName).",
                style: .info
            )
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func startOpenAIOAuth() async {
        guard !isSubmitting else {
            return
        }

        isSubmitting = true
        defer { isSubmitting = false }

        do {
            let startResponse = try await appState.client.oauthStart(provider: SetupProvider.openai.providerID)
            guard let authorizeURL = URL(string: startResponse.authorizeUrl) else {
                throw APIError.invalidURL(startResponse.authorizeUrl)
            }
            guard let providerRedirectURL = URL(string: startResponse.redirectUri) else {
                throw APIError.invalidURL(startResponse.redirectUri)
            }
            guard let nativeCallbackURL = URL(string: "fawx-auth://openai/callback") else {
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
                provider: SetupProvider.openai.providerID,
                code: code,
                flowToken: startResponse.flowToken
            )

            await appState.refreshSettingsState()
            configuredProviderIDs.insert(SetupProvider.openai.providerID)
            statusKind = response.verified ? .success : .warning
            statusMessage = response.verified
                ? "ChatGPT connected."
                : "ChatGPT sign-in needs verification."
            appState.showToast(
                message: response.verified
                    ? "ChatGPT connected."
                    : "ChatGPT sign-in needs verification.",
                style: response.verified ? .success : .warning
            )
        } catch {
            let nsError = error as NSError
            if nsError.domain == ASWebAuthenticationSessionErrorDomain
                && nsError.code == ASWebAuthenticationSessionError.canceledLogin.rawValue
            {
                statusKind = .warning
                statusMessage = "Sign-in cancelled."
                appState.showToast(message: "Sign-in cancelled.", style: .info)
                return
            }

            statusKind = .failure
            statusMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }
}
