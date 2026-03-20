import AuthenticationServices
import Foundation
import Observation

enum SetupStep: Int, CaseIterable, Identifiable, Sendable {
    case welcome
    case tailscale
    case provider
    case ready

    var id: Int { rawValue }
}

enum SetupProvider: String, CaseIterable, Identifiable, Sendable {
    case anthropic
    case openai

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .anthropic:
            "Claude"
        case .openai:
            "ChatGPT"
        }
    }

    var companyName: String {
        switch self {
        case .anthropic:
            "Anthropic"
        case .openai:
            "OpenAI"
        }
    }

    var providerID: String {
        rawValue
    }
}

enum SetupProviderAuthMethod: String, CaseIterable, Identifiable, Sendable {
    case subscription
    case apiKey

    var id: String { rawValue }

    var title: String {
        switch self {
        case .subscription:
            "I have a subscription"
        case .apiKey:
            "I have an API key"
        }
    }
}

@MainActor
@Observable
final class SetupViewModel {
    typealias LocalSetupAction = (Bool, @escaping @MainActor @Sendable (String) -> Void) async throws -> Void
    typealias Phase4StateRefreshAction = () async -> Void

    var step: SetupStep = .welcome
    var selectedProvider: SetupProvider = .anthropic
    var selectedAuthMethod: SetupProviderAuthMethod = .subscription
    var credentialInput = ""
    var providerStatusKind: ConnectionTestKind = .idle
    var providerStatusMessage: String?
    var tailscaleStatusKind: ConnectionTestKind = .idle
    var tailscaleStatusMessage: String?
    var readyStatusKind: ConnectionTestKind = .idle
    var readyStatusMessage: String?
    var isRefreshing = false
    var isSubmittingProvider = false
    var isTogglingAutoStart = false
    var isBootstrapping = false
    var bootstrapProgress: String?

    private let appState: AppState
    private let completeLocalSetupAction: LocalSetupAction
    private let refreshPhase4StateAction: Phase4StateRefreshAction
    private var attemptedCertificateHostname: String?

    init(
        appState: AppState,
        completeLocalSetupAction: LocalSetupAction? = nil,
        refreshPhase4StateAction: Phase4StateRefreshAction? = nil
    ) {
        self.appState = appState
        self.completeLocalSetupAction = completeLocalSetupAction ?? { markSetupComplete, progress in
            try await appState.completeLocalSetup(markSetupComplete: markSetupComplete, progress: progress)
        }
        self.refreshPhase4StateAction = refreshPhase4StateAction ?? {
            await appState.refreshPhase4State()
        }
    }

    var refreshKey: String {
        "\(step.rawValue)|\(selectedProvider.rawValue)|\(selectedAuthMethod.rawValue)|\(appState.setupWizardKey)"
    }

    var tailscaleStatus: SetupTailscaleStatus? {
        appState.setupStatus?.tailscale
    }

    var localServerStatus: SetupLocalServerStatus? {
        appState.setupStatus?.localServer
    }

    var qrPairing: QrPairingResponse? {
        appState.qrPairingResponse
    }

    var configuredProviderIDs: Set<String> {
        let authenticatedProviders = Set(appState.authProviders.filter(\.isConfigured).map(\.provider))
        let setupConfiguredProviders = Set(appState.setupStatus?.auth.providersConfigured ?? [])
        return authenticatedProviders.union(setupConfiguredProviders)
    }

    var selectedProviderConfigured: Bool {
        configuredProviderIDs.contains(selectedProvider.providerID)
    }

    var supportsSubscriptionFlow: Bool {
        selectedProvider == .anthropic || selectedProvider == .openai
    }

    var usesOAuthSubscriptionFlow: Bool {
        selectedProvider == .openai && selectedAuthMethod == .subscription
    }

    var showsCredentialInput: Bool {
        !usesOAuthSubscriptionFlow
    }

    var showsCredentialSubmitButton: Bool {
        !usesOAuthSubscriptionFlow
    }

    var canContinueFromTailscale: Bool {
        tailscaleStatus?.certReady == true
    }

    var providerActionTitle: String {
        switch (selectedProvider, selectedAuthMethod) {
        case (.anthropic, .subscription):
            "Save Setup Token"
        case (_, .apiKey):
            "Save API Key"
        case (.openai, .subscription):
            "Sign in with ChatGPT"
        }
    }

    var providerFieldTitle: String {
        switch (selectedProvider, selectedAuthMethod) {
        case (.anthropic, .subscription):
            "Setup Token"
        case (_, .apiKey):
            "API Key"
        case (.openai, .subscription):
            "API Key"
        }
    }

    var providerFieldPrompt: String {
        switch (selectedProvider, selectedAuthMethod) {
        case (.anthropic, .subscription):
            "Paste the Anthropic setup token"
        case (.anthropic, .apiKey):
            "Paste your Anthropic API key"
        case (.openai, _):
            "Paste your OpenAI API key"
        }
    }

    var readyAutoStartEnabled: Bool {
        appState.autoStartEnabled
    }

    func prepareCurrentStep() async {
        isRefreshing = true
        defer { isRefreshing = false }

        switch step {
        case .welcome:
            break
        case .tailscale:
            await refreshTailscaleState()
        case .provider:
            await refreshProviderState()
        case .ready:
            await refreshReadyState()
        }
    }

    func continueFromWelcome() {
        step = .tailscale
    }

    func goBack() {
        switch step {
        case .welcome:
            break
        case .tailscale:
            step = .welcome
        case .provider:
            step = .tailscale
        case .ready:
            step = .provider
        }
    }

    func skipTailscale() {
        step = .provider
    }

    func continueFromTailscale() {
        guard canContinueFromTailscale else {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = "Finish Tailscale HTTPS setup or skip for now."
            return
        }

        step = .provider
    }

    func skipProvider() {
        step = .ready
    }

    func continueFromProvider() {
        step = .ready
    }

    func selectProvider(_ provider: SetupProvider) {
        selectedProvider = provider
        credentialInput = ""
        providerStatusKind = .idle
        providerStatusMessage = nil
    }

    func selectAuthMethod(_ method: SetupProviderAuthMethod) {
        selectedAuthMethod = method
        credentialInput = ""
        providerStatusKind = .idle
        providerStatusMessage = nil
    }

    func submitProviderCredentials() async {
        let trimmedCredential = credentialInput.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedCredential.isEmpty else {
            providerStatusKind = .failure
            providerStatusMessage = "Enter your \(providerFieldTitle.lowercased()) first."
            return
        }

        isSubmittingProvider = true
        defer { isSubmittingProvider = false }

        do {
            let response: ProviderAuthActionResponse
            switch (selectedProvider, selectedAuthMethod) {
            case (.anthropic, .subscription):
                response = try await appState.storeAnthropicSetupToken(trimmedCredential)
            case (.openai, .subscription):
                providerStatusKind = .warning
                providerStatusMessage = "Use Sign in with ChatGPT to connect your subscription."
                return
            case (_, .apiKey):
                response = try await appState.storeProviderAPIKey(
                    provider: selectedProvider.providerID,
                    apiKey: trimmedCredential
                )
            }

            credentialInput = ""
            providerStatusKind = .success
            providerStatusMessage = "\(response.provider.capitalized) is authenticated."
        } catch {
            providerStatusKind = .failure
            providerStatusMessage = error.localizedDescription
        }
    }

    func startOpenAISubscriptionLogin(
        using coordinator: OAuthSessionCoordinator
    ) async {
        guard !isSubmittingProvider else {
            return
        }

        isSubmittingProvider = true
        defer { isSubmittingProvider = false }

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

            let callbackURL = try await coordinator.authenticate(
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

            try await refreshAfterOAuthSuccess()
            applyOAuthSuccess(response)
        } catch {
            if isOAuthCancellation(error) {
                providerStatusKind = .warning
                providerStatusMessage = "Sign-in cancelled."
                return
            }

            providerStatusKind = .failure
            providerStatusMessage = error.localizedDescription
        }
    }

    func verifySelectedProvider() async {
        isSubmittingProvider = true
        defer { isSubmittingProvider = false }

        do {
            let response = try await appState.verifyProvider(selectedProvider.providerID)
            providerStatusKind = response.verified ? .success : .warning
            providerStatusMessage = response.message
        } catch {
            providerStatusKind = .failure
            providerStatusMessage = error.localizedDescription
        }
    }

    func setAutoStartEnabled(_ enabled: Bool) async {
        isTogglingAutoStart = true
        defer { isTogglingAutoStart = false }

        do {
            let message = try await appState.setLaunchAgentEnabled(enabled)
            readyStatusKind = .success
            readyStatusMessage = message
        } catch {
            readyStatusKind = .failure
            readyStatusMessage = error.localizedDescription
        }
    }

    func finishSetup() async {
        guard !isBootstrapping else {
            return
        }

        isBootstrapping = true
        bootstrapProgress = "Creating Fawx configuration..."
        readyStatusKind = .idle
        readyStatusMessage = nil
        defer {
            isBootstrapping = false
            bootstrapProgress = nil
        }

        do {
            try await completeLocalSetupAction(true) { [weak self] message in
                self?.bootstrapProgress = message
            }
        } catch {
            readyStatusKind = .failure
            readyStatusMessage = error.localizedDescription
        }
    }

    private func refreshTailscaleState() async {
        await refreshPhase4StateAction()

        guard let tailscale = tailscaleStatus else {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = "Tailscale status is unavailable right now."
            return
        }

        if !tailscale.installed {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = "Install Tailscale to enable secure iPhone pairing."
            return
        }

        if !tailscale.running || !tailscale.loggedIn {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = "Tailscale is installed, but you still need to sign in."
            return
        }

        if tailscale.certReady {
            tailscaleStatusKind = .success
            tailscaleStatusMessage = "Tailscale HTTPS is ready."
            return
        }

        guard let hostname = tailscale.hostname, attemptedCertificateHostname != hostname else {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = "Tailscale is ready. Finish the HTTPS certificate step to continue."
            return
        }

        attemptedCertificateHostname = hostname

        do {
            let response = try await appState.requestTailscaleCertificate(hostname: hostname)
            tailscaleStatusKind = .success
            tailscaleStatusMessage = "HTTPS certificate configured for \(response.hostname)."
        } catch {
            tailscaleStatusKind = .warning
            tailscaleStatusMessage = error.localizedDescription
        }
    }

    private func refreshProviderState() async {
        guard await ensureProviderServerIsRunning() else {
            return
        }

        await refreshPhase4StateAction()

        if !configuredProviderIDs.isEmpty {
            providerStatusKind = .success
            providerStatusMessage = "Provider authentication is ready."
        } else if appState.setupStatus == nil {
            providerStatusKind = .warning
            providerStatusMessage = "Provider status is unavailable until the server reconnects."
        } else {
            providerStatusKind = .idle
            providerStatusMessage = nil
        }
    }

    private func refreshReadyState() async {
        await refreshPhase4StateAction()

        if appState.qrPairingResponse != nil {
            readyStatusKind = .success
            readyStatusMessage = "Your Mac is ready for iPhone pairing."
        } else if appState.setupStatus?.tailscale.certReady == false {
            readyStatusKind = .warning
            readyStatusMessage = "Set up Tailscale HTTPS in Settings to enable secure iPhone pairing."
        } else {
            readyStatusKind = .idle
            readyStatusMessage = nil
        }
    }

    private func refreshAfterOAuthSuccess() async throws {
        if appState.isConfigured {
            await appState.refreshSettingsState()
        } else {
            await refreshPhase4StateAction()
        }
    }

    private func ensureProviderServerIsRunning() async -> Bool {
        guard !appState.isConfigured else {
            return true
        }

        providerStatusKind = .idle
        providerStatusMessage = nil
        bootstrapProgress = "Starting Fawx server..."

        do {
            let service = LocalBootstrapService()
            let result = try await service.performFullBootstrap { [weak self] message in
                await MainActor.run { self?.bootstrapProgress = message }
            }
            await appState.configureClientForBootstrap(
                serverURL: "http://\(result.host):\(result.port)",
                bearerToken: result.bearerToken
            )
            bootstrapProgress = nil
            return true
        } catch {
            providerStatusKind = .failure
            providerStatusMessage = "Could not start the server: \(error.localizedDescription)"
            bootstrapProgress = nil
            return false
        }
    }

    private func applyOAuthSuccess(_ response: OAuthCallbackResponse) {
        providerStatusKind = response.verified ? .success : .warning
        providerStatusMessage = response.verified
            ? "ChatGPT is authenticated."
            : "ChatGPT sign-in needs verification."
    }

    private func isOAuthCancellation(_ error: Error) -> Bool {
        let nsError = error as NSError
        return nsError.domain == ASWebAuthenticationSessionErrorDomain
            && nsError.code == ASWebAuthenticationSessionError.canceledLogin.rawValue
    }
}
