import Observation
import SwiftUI
import UserNotifications

enum ConnectionStatus: String, Sendable {
    case disconnected
    case connecting
    case connected
    case reconnecting
}

enum AppTheme: String, CaseIterable, Sendable {
    case system
    case light
    case dark

    var displayName: String {
        rawValue.capitalized
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system:
            nil
        case .light:
            .light
        case .dark:
            .dark
        }
    }
}

enum AppFontSize: String, CaseIterable, Sendable {
    case small
    case medium
    case large

    var displayName: String {
        rawValue.capitalized
    }

    var scale: CGFloat {
        switch self {
        case .small:
            0.92
        case .medium:
            1
        case .large:
            1.12
        }
    }

    var sliderValue: Double {
        switch self {
        case .small:
            0
        case .medium:
            1
        case .large:
            2
        }
    }

    static func fromSliderValue(_ value: Double) -> AppFontSize {
        switch Int(value.rounded()) {
        case ...0:
            .small
        case 2...:
            .large
        default:
            .medium
        }
    }
}

enum AppToastStyle: Sendable, Equatable {
    case info
    case success
    case warning
    case error
}

struct AppToast: Identifiable, Equatable, Sendable {
    let id = UUID()
    let message: String
    let style: AppToastStyle
}

enum ConnectionBannerTone: Sendable, Equatable {
    case warning
    case error
}

struct ConnectionBannerState: Sendable, Equatable {
    let message: String
    let tone: ConnectionBannerTone
    let showsRetry: Bool
}

enum AppConnectionMode: String, Sendable {
    case local
    case remote
}

enum AppRootDestination: Sendable, Equatable {
    case main
    case setupWizard
    case remoteOnboarding
}

enum RefreshCadence {
    static let dashboardPanels: Duration = .seconds(30)
}

@MainActor
@Observable
final class AppState {
    private enum StorageKey {
        static let serverURL = "server_url"
        static let pairedDeviceName = "paired_device_name"
        static let theme = "theme"
        static let fontSize = "font_size"
        static let setupComplete = "setup_complete"
        static let connectionMode = "connection_mode"
    }

    private static let defaultLocalSetupServerURLString = "http://127.0.0.1:8400"

    var connectionStatus: ConnectionStatus = .disconnected
    var connectionMode: AppConnectionMode
    var rootDestination: AppRootDestination
    var serverURLString: String
    var pairedDeviceName: String?
    var activeModel: ModelInfo?
    var thinkingLevel: ThinkingLevel?
    var availableThinkingLevels: [ThinkingLevel] = ThinkingLevel.defaultOptions
    var availableModels: [ModelInfo] = []
    var skills: [SkillSummary] = []
    var authProviders: [AuthProvider] = []
    var lastHealth: HealthResponse?
    var currentContext: ContextInfo?
    var permissionPresetName = "Power User"
    var permissionMode: PermissionMode = .prompt
    var ripcordStatus: RipcordStatusResponse?
    var connectionError: String?
    var theme: AppTheme
    var fontSize: AppFontSize
    var isUpdatingServerSettings = false
    var authProvidersError: String?
    var serverStatusError: String?
    var sidebarSelection: SidebarSelection?
    var toast: AppToast?
    var setupStatus: SetupStatusResponse?
    var localServerStatus: LocalServerRuntimeStatus?
    var launchAgentStatus: LaunchAgentStatusResponse?
    var qrPairingResponse: QrPairingResponse?
    var localInstallConfiguration: LocalInstallConfiguration?
    var setupActionError: String?

    let client: FawxClient
    private var authToken: String?
    private var isSetupComplete: Bool
    @ObservationIgnored private var reconnectTask: Task<Void, Never>?
    @ObservationIgnored private var hasRequestedNotificationAuthorization = false
    @ObservationIgnored private var toastDismissTask: Task<Void, Never>?
    @ObservationIgnored private var reconnectAttempt = 0

    init() {
        if UITestLaunchOptions.shouldResetState {
            Self.resetPersistedConfiguration()
        }

        let defaults = UserDefaults.standard
        let storedServerURL = defaults.string(forKey: StorageKey.serverURL) ?? ""
        let storedTheme = AppTheme(rawValue: defaults.string(forKey: StorageKey.theme) ?? AppTheme.system.rawValue) ?? .system
        let storedFontSize = AppFontSize(rawValue: defaults.string(forKey: StorageKey.fontSize) ?? AppFontSize.medium.rawValue) ?? .medium
        let defaultConnectionMode = Self.defaultConnectionMode(forStoredServerURL: storedServerURL)
        let storedConnectionMode = AppConnectionMode(
            rawValue: defaults.string(forKey: StorageKey.connectionMode) ?? defaultConnectionMode.rawValue
        ) ?? defaultConnectionMode
        let storedSetupComplete = defaults.bool(forKey: StorageKey.setupComplete)
        let storedToken = try? KeychainHelper.token(forServer: storedServerURL)
        let storedDeviceName = defaults.string(forKey: StorageKey.pairedDeviceName)
        let detectedLocalInstall = LocalInstallConfiguration.loadDefault()

        var resolvedServerURL = UITestLaunchOptions.serverURLOverride ?? storedServerURL
        var resolvedToken = UITestLaunchOptions.bearerTokenOverride ?? storedToken
        var resolvedConnectionMode = storedConnectionMode

#if os(macOS)
        if (resolvedServerURL.isEmpty || resolvedToken?.isEmpty != false), let detectedLocalInstall {
            resolvedServerURL = detectedLocalInstall.baseURLString
            resolvedToken = detectedLocalInstall.bearerToken
            resolvedConnectionMode = .local
        } else if resolvedServerURL.isEmpty && resolvedConnectionMode == .local {
            resolvedServerURL = Self.defaultLocalSetupServerURLString
        }
#endif

        let resolvedDeviceName = UITestLaunchOptions.pairedDeviceNameOverride
            ?? storedDeviceName
            ?? (UITestLaunchOptions.isUITesting && resolvedToken != nil ? "UI Test Device" : nil)

        let resolvedSetupComplete = storedSetupComplete || detectedLocalInstall != nil
        let resolvedBaseURL = URL(string: resolvedServerURL)

        self.connectionMode = resolvedConnectionMode
        self.rootDestination = Self.resolveInitialDestination(
            isConfigured: !resolvedServerURL.isEmpty && resolvedToken?.isEmpty == false,
            setupComplete: resolvedSetupComplete,
            connectionMode: resolvedConnectionMode,
            hasStoredServerURL: !storedServerURL.isEmpty,
            hasLocalInstall: detectedLocalInstall != nil
        )
        self.serverURLString = resolvedServerURL
        self.pairedDeviceName = resolvedDeviceName
        self.theme = storedTheme
        self.fontSize = storedFontSize
        self.authToken = resolvedToken
        self.localInstallConfiguration = detectedLocalInstall
        self.isSetupComplete = resolvedSetupComplete
        self.client = FawxClient(baseURL: resolvedBaseURL, bearerToken: resolvedToken)

        if resolvedSetupComplete {
            defaults.set(true, forKey: StorageKey.setupComplete)
        }
        defaults.set(resolvedConnectionMode.rawValue, forKey: StorageKey.connectionMode)
    }

    var isConfigured: Bool {
        guard !serverURLString.isEmpty else {
            return false
        }
        guard let authToken, !authToken.isEmpty else {
            return false
        }
        return true
    }

    var showsMainExperience: Bool {
        rootDestination == .main
    }

    var isRemoteClient: Bool {
        connectionMode == .remote
    }

    var canOpenRemoteOnboarding: Bool {
#if os(macOS)
        true
#else
        false
#endif
    }

    var localLogFileURL: URL? {
        localInstallConfiguration?.logFileURL
    }

    var advertisedHost: String? {
        if let displayHost = qrPairingResponse?.displayHost, !displayHost.isEmpty {
            return displayHost
        }
        if let hostname = setupStatus?.tailscale.hostname, !hostname.isEmpty {
            return hostname
        }
        return nil
    }

    var displayedHost: String {
        if let host = URL(string: serverURLString)?.host, !host.isEmpty {
            return host
        }
        if let host = localServerStatus?.host, !host.isEmpty {
            return host
        }
        if let host = setupStatus?.localServer.host, !host.isEmpty {
            return host
        }
        return "Not connected"
    }

    var displayedPort: Int? {
        if let port = URL(string: serverURLString)?.port {
            return port
        }
        if let port = localServerStatus?.port {
            return port
        }
        if let port = setupStatus?.localServer.port {
            return port
        }
        return nil
    }

    var displayedServerURLString: String {
        if !serverURLString.isEmpty {
            return serverURLString
        }
        let host = localServerStatus?.host ?? setupStatus?.localServer.host
        let port = localServerStatus?.port ?? setupStatus?.localServer.port
        let prefersHTTPS = localServerStatus?.httpsEnabled ?? setupStatus?.localServer.httpsEnabled ?? false
        if let host, let port {
            return "\(prefersHTTPS ? "https" : "http")://\(host):\(port)"
        }
        return ""
    }

    var serverStatusLabel: String {
        switch connectionStatus {
        case .connected:
            return localServerStatus?.status.capitalized ?? "Connected"
        case .connecting:
            return "Connecting"
        case .reconnecting:
            return "Reconnecting"
        case .disconnected:
            if let localServerStatus, localServerStatus.status == "stopped" {
                return "Stopped"
            }
            return "Disconnected"
        }
    }

    var autoStartEnabled: Bool {
        launchAgentStatus?.installed
            ?? setupStatus?.launchagent.autoStartEnabled
            ?? false
    }

    var canManageServerLocally: Bool {
#if os(macOS)
        connectionMode == .local
#else
        false
#endif
    }

    var configurationKey: String {
        [
            serverURLString,
            pairedDeviceName ?? "",
            isConfigured ? "paired" : "unpaired",
            connectionMode.rawValue,
            rootDestinationKey,
        ].joined(separator: "|")
    }

    var setupWizardKey: String {
        [
            localInstallConfiguration?.baseURLString ?? "",
            setupStatus?.mode ?? "",
            setupStatus?.launchagent.loaded == true ? "launchagent" : "no-launchagent",
            setupStatus?.tailscale.hostname ?? "",
            authProviders.map(\.provider).joined(separator: ","),
        ].joined(separator: "|")
    }

    var preferredColorScheme: ColorScheme? {
        theme.colorScheme
    }

    var connectionBanner: ConnectionBannerState? {
        guard showsMainExperience, isConfigured, let connectionError, !connectionError.isEmpty else {
            if showsMainExperience, connectionStatus == .reconnecting {
                return ConnectionBannerState(
                    message: "Reconnecting to Fawx server at \(serverURLString)...",
                    tone: .warning,
                    showsRetry: false
                )
            }
            return nil
        }

        switch connectionStatus {
        case .reconnecting:
            return ConnectionBannerState(message: connectionError, tone: .warning, showsRetry: false)
        case .disconnected:
            return ConnectionBannerState(message: connectionError, tone: .error, showsRetry: true)
        case .connecting, .connected:
            return nil
        }
    }

    var activeRipcordStatus: RipcordStatusResponse? {
        guard let ripcordStatus, ripcordStatus.active else {
            return nil
        }
        return ripcordStatus
    }

    func bootstrap() async {
        guard showsMainExperience, isConfigured else {
            reconnectTask?.cancel()
            reconnectTask = nil
            if showsMainExperience {
                connectionStatus = .disconnected
            }
            clearRipcordState()
            return
        }

        reconnectAttempt = 0
        await attemptConnection(initialStatus: .connecting, allowReconnect: true)
        await refreshPhase4State()
        await refreshRipcordState()
    }

    func beginRemoteOnboarding() {
        connectionMode = .remote
        rootDestination = .remoteOnboarding
        setupActionError = nil
        persistConnectionMode()
    }

    func returnToLocalSetup() {
#if os(macOS)
        connectionMode = .local
        rootDestination = .setupWizard
        setupActionError = nil
        persistConnectionMode()
#endif
    }

    func completeLocalSetup() async throws {
        reloadLocalInstallConfiguration()

        let resolvedServerURL: String

        if let localInstallConfiguration {
            resolvedServerURL = localInstallConfiguration.baseURLString
        } else if !serverURLString.isEmpty {
            resolvedServerURL = serverURLString
        } else {
            resolvedServerURL = Self.defaultLocalSetupServerURLString
        }

        guard
            let canonicalURLString = canonicalizeServerURL(resolvedServerURL),
            let url = URL(string: canonicalURLString)
        else {
            throw APIError.invalidURL(resolvedServerURL)
        }

        let setupClient = FawxClient(baseURL: url)
        let requestedDeviceName = Self.defaultLocalDeviceName()
        let response = try await setupClient.adoptLocalDevice(deviceName: requestedDeviceName)
        let pairedDeviceName = response.deviceName?.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedDeviceName = pairedDeviceName?.nonEmpty ?? requestedDeviceName

        try await savePairing(
            serverURLString: canonicalURLString,
            token: response.token,
            deviceName: resolvedDeviceName,
            connectionMode: .local
        )
        isSetupComplete = true
        UserDefaults.standard.set(true, forKey: StorageKey.setupComplete)
        await bootstrap()
    }

    func savePairing(
        serverURLString rawURL: String,
        token: String,
        deviceName: String,
        connectionMode: AppConnectionMode = .remote
    ) async throws {
        guard let canonicalURLString = canonicalizeServerURL(rawURL) else {
            throw APIError.invalidURL(rawURL)
        }
        guard let url = URL(string: canonicalURLString) else {
            throw APIError.invalidURL(canonicalURLString)
        }

        serverURLString = canonicalURLString
        pairedDeviceName = deviceName
        authToken = token
        self.connectionMode = connectionMode
        rootDestination = .main

        let defaults = UserDefaults.standard
        defaults.set(canonicalURLString, forKey: StorageKey.serverURL)
        defaults.set(deviceName, forKey: StorageKey.pairedDeviceName)
        defaults.set(connectionMode.rawValue, forKey: StorageKey.connectionMode)
        try KeychainHelper.saveToken(token, forServer: canonicalURLString)

        await client.updateConfiguration(baseURL: url, bearerToken: token)
        connectionError = nil
        serverStatusError = nil
        connectionStatus = .connected
        setupActionError = nil
    }

    func unpair() async throws {
        if !serverURLString.isEmpty {
            try? KeychainHelper.deleteToken(forServer: serverURLString)
        }

        let hasLocalInstall = localInstallConfiguration != nil
        let setupComplete = isSetupComplete
        let previousConnectionMode = connectionMode

        authToken = nil
        pairedDeviceName = nil
        lastHealth = nil
        activeModel = nil
        thinkingLevel = nil
        availableThinkingLevels = ThinkingLevel.defaultOptions
        currentContext = nil
        availableModels = []
        skills = []
        authProviders = []
        permissionPresetName = "Power User"
        permissionMode = .prompt
        ripcordStatus = nil
        connectionError = nil
        connectionStatus = .disconnected
        serverStatusError = nil
        setupStatus = nil
        localServerStatus = nil
        launchAgentStatus = nil
        qrPairingResponse = nil
        clearRipcordState()

        UserDefaults.standard.removeObject(forKey: StorageKey.serverURL)
        UserDefaults.standard.removeObject(forKey: StorageKey.pairedDeviceName)
        await client.updateConfiguration(baseURL: nil, bearerToken: nil)

        rootDestination = Self.resolveInitialDestination(
            isConfigured: false,
            setupComplete: setupComplete,
            connectionMode: previousConnectionMode,
            hasStoredServerURL: false,
            hasLocalInstall: hasLocalInstall && previousConnectionMode == .local
        )
    }

    func refreshServerState() async throws {
        async let modelsTask = client.listModels()
        async let legacyStatusTask = client.serverStatus()
        async let permissionsTask = client.getPermissions()

        let models = try await modelsTask
        let legacyStatus: ServerStatusResponse?
        do {
            legacyStatus = try await legacyStatusTask
            serverStatusError = nil
        } catch {
            legacyStatus = nil
            serverStatusError = error.localizedDescription
        }

        let permissionsResponse: PermissionsResponse?
        do {
            permissionsResponse = try await permissionsTask
        } catch {
            permissionsResponse = nil
        }

        do {
            let thinking = try await client.thinking()
            thinkingLevel = thinking.level
            availableThinkingLevels = thinking.available
        } catch {
            thinkingLevel = nil
        }

        await refreshAuthProviders()

        availableModels = models.models
        let activeModelID = legacyStatus?.model ?? models.activeModel
        activeModel = models.models.first(where: { $0.modelID == activeModelID }) ?? models.models.first
        if let permissionsResponse {
            permissionPresetName = permissionPresetLabel(permissionsResponse.preset)
            permissionMode = permissionsResponse.mode
        } else {
            permissionPresetName = resolvePermissionPreset(from: legacyStatus?.config)
        }
        await refreshPhase4State()
    }

    func refreshAuthProviders() async {
        do {
            let auth = try await client.authProviders()
            authProviders = auth.providers.sorted { lhs, rhs in
                if lhs.isConfigured != rhs.isConfigured {
                    return lhs.isConfigured && !rhs.isConfigured
                }
                return lhs.displayName.localizedCaseInsensitiveCompare(rhs.displayName) == .orderedAscending
            }
            authProvidersError = nil
        } catch {
            authProviders = []
            authProvidersError = error.localizedDescription
        }
    }

    func refreshSettingsState() async {
        if isConfigured {
            do {
                try await refreshServerState()
                return
            } catch {
                await noteRecoverableRequestFailure(error)
            }
        }
        await refreshAuthProviders()
        await refreshPhase4State()
    }

    func refreshRipcordState() async {
        guard isConfigured else {
            clearRipcordState()
            return
        }

        do {
            let status = try await client.ripcordStatus()
            let previousStatus = ripcordStatus
            ripcordStatus = status

            if shouldNotifyForRipcordActivation(from: previousStatus, to: status) {
                postRipcordNotification(for: status)
            }
        } catch let error as APIError where error.statusCode == 503 {
            clearRipcordState()
        } catch {
            if ConnectionStateMachine.shouldHandleAsConnectionIssue(error) {
                await noteRecoverableRequestFailure(error)
            }
        }
    }

    func loadRipcordJournal() async throws -> [JournalEntry] {
        let response = try await client.ripcordJournal()
        return response.entries.sorted { lhs, rhs in
            lhs.id < rhs.id
        }
    }

    func pullRipcord() async throws -> RipcordReport {
        let report = try await client.pullRipcord()
        ripcordStatus = .inactive
        return report
    }

    func approveRipcord() async throws {
        try await client.approveRipcord()
        ripcordStatus = .inactive
    }

    func refreshPhase4State() async {
        let canQueryLocalSetupServer: Bool
#if os(macOS)
        canQueryLocalSetupServer = rootDestination == .setupWizard
            && connectionMode == .local
            && !serverURLString.isEmpty
#else
        canQueryLocalSetupServer = false
#endif

        guard isConfigured || canQueryLocalSetupServer else {
            setupStatus = nil
            localServerStatus = nil
            launchAgentStatus = nil
            qrPairingResponse = nil
            return
        }

        do {
            setupStatus = try await client.setupStatus()
            setupActionError = nil
        } catch {
            setupStatus = nil
        }

        do {
            localServerStatus = try await client.runtimeStatus()
        } catch {
            if localServerStatus?.status != "stopped" {
                localServerStatus = nil
            }
        }

        do {
            launchAgentStatus = try await client.launchAgentStatus()
        } catch {
            launchAgentStatus = nil
        }

        do {
            qrPairingResponse = try await client.qrPairing()
        } catch {
            qrPairingResponse = nil
        }
    }

    func setLaunchAgentEnabled(_ enabled: Bool) async throws -> String {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        if enabled {
            let response = try await client.installLaunchAgent(autoStart: true)
            showToast(message: response.message, style: .success)
            await refreshPhase4State()
            return response.message
        } else {
            let response = try await client.uninstallLaunchAgent()
            showToast(message: response.message, style: .info)
            await refreshPhase4State()
            return response.message
        }
    }

    func restartLocalServer() async throws -> String {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        let response = try await client.restartServer()
        connectionStatus = .reconnecting
        if let localServerStatus {
            self.localServerStatus = LocalServerRuntimeStatus(
                status: "starting",
                version: localServerStatus.version,
                uptimeSeconds: 0,
                pid: localServerStatus.pid,
                host: localServerStatus.host,
                port: localServerStatus.port,
                httpsEnabled: localServerStatus.httpsEnabled
            )
        }
        showToast(message: response.message, style: .info)
        await waitForLocalServerReconnect()
        await revalidateConnection(allowReconnect: true)
        await refreshPhase4State()
        return response.message
    }

    func stopLocalServer() async throws -> String {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        let response = try await client.stopServer()
        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0
        connectionStatus = .disconnected
        connectionError = connectionMode == .local ? nil : connectionMessage(for: APIError.invalidResponse)
        localServerStatus = LocalServerRuntimeStatus(
            status: "stopped",
            version: localServerStatus?.version ?? "",
            uptimeSeconds: 0,
            pid: 0,
            host: localServerStatus?.host ?? setupStatus?.localServer.host ?? displayedHost,
            port: localServerStatus?.port ?? setupStatus?.localServer.port ?? displayedPort ?? 8400,
            httpsEnabled: localServerStatus?.httpsEnabled ?? setupStatus?.localServer.httpsEnabled ?? false
        )
        showToast(message: response.message, style: .info)
        await refreshPhase4State()
        return response.message
    }

    func updateServerPort(_ port: Int) async throws -> ConfigPatchResponse {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        let response = try await client.patchConfig(
            changes: .object([
                "http": .object([
                    "port": .number(Double(port)),
                ]),
            ])
        )
        if response.restartRequired {
            showToast(message: "Port updated. Restart the server to apply it.", style: .info)
        } else {
            showToast(message: "Server port updated.", style: .success)
        }
        await refreshPhase4State()
        return response
    }

    func fetchPairingQRCode() async throws -> QrPairingResponse {
        let response = try await client.qrPairing()
        qrPairingResponse = response
        return response
    }

    func generatePairingCode() async throws -> PairingCodeResponse {
        try await client.generatePairingCode()
    }

    func requestTailscaleCertificate(hostname: String) async throws -> TailscaleCertResponse {
        let response = try await client.tailscaleCert(hostname: hostname)
        await refreshPhase4State()
        return response
    }

    func storeAnthropicSetupToken(_ token: String) async throws -> ProviderAuthActionResponse {
        let response = try await client.exchangeAnthropicSetupToken(token)
        if isConfigured {
            try await refreshServerState()
        } else {
            await refreshPhase4State()
        }
        return response
    }

    func storeProviderAPIKey(provider: String, apiKey: String) async throws -> ProviderAuthActionResponse {
        let response = try await client.storeAPIKey(provider: provider, apiKey: apiKey)
        if isConfigured {
            try await refreshServerState()
        } else {
            await refreshPhase4State()
        }
        return response
    }

    func verifyProvider(_ provider: String) async throws -> ProviderVerificationResponse {
        let response = try await client.verifyProvider(provider)
        if isConfigured {
            try await refreshServerState()
        } else {
            await refreshPhase4State()
        }
        return response
    }

    func deleteProvider(_ provider: String) async throws {
        _ = try await client.deleteProvider(provider)
        try await refreshServerState()
    }

    func retryConnection() async {
        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0
        await attemptConnection(initialStatus: .connecting, allowReconnect: true)
    }

    func revalidateConnection(allowReconnect: Bool = true) async {
        guard isConfigured else {
            reconnectTask?.cancel()
            reconnectTask = nil
            reconnectAttempt = 0
            lastHealth = nil
            connectionError = nil
            connectionStatus = .disconnected
            clearRipcordState()
            return
        }

        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0

        let initialStatus: ConnectionStatus = allowReconnect ? .reconnecting : .connecting
        await attemptConnection(initialStatus: initialStatus, allowReconnect: allowReconnect)
        await refreshPhase4State()
    }

    func markDisconnected(from error: Error) {
        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0
        lastHealth = nil
        connectionError = connectionMessage(for: error)
        connectionStatus = .disconnected
        clearRipcordState()
    }

    func userFacingConnectionMessage(for error: Error) -> String {
        connectionMessage(for: error)
    }

    func noteRecoverableRequestFailure(_ error: Error) async {
        guard ConnectionStateMachine.shouldHandleAsConnectionIssue(error) else {
            return
        }

        await handleConnectionFailure(error, allowReconnect: true)
    }

    func refreshContext(for sessionID: String?) async {
        guard let sessionID, !sessionID.isEmpty else {
            currentContext = nil
            return
        }

        do {
            currentContext = try await client.sessionContext(id: sessionID)
        } catch {
            currentContext = nil
        }
    }

    func clearContext() {
        currentContext = nil
    }

    func setModel(_ modelID: String) async throws {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        let response = try await client.setModel(modelID)
        activeModel = availableModels.first(where: { $0.modelID == response.activeModel }) ?? activeModel

        if let thinkingAdjusted = response.thinkingAdjusted {
            thinkingLevel = thinkingAdjusted.to
            showToast(message: "Thinking adjusted to \(thinkingAdjusted.to.displayName).", style: .info)
        }

        try await refreshServerState()
    }

    func setThinking(_ level: ThinkingLevel) async throws {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        let response = try await client.setThinking(level)
        thinkingLevel = response.level
        availableThinkingLevels = response.available
        try await refreshServerState()
    }

    func setTheme(_ theme: AppTheme) {
        self.theme = theme
        UserDefaults.standard.set(theme.rawValue, forKey: StorageKey.theme)
    }

    func setFontSize(_ fontSize: AppFontSize) {
        self.fontSize = fontSize
        UserDefaults.standard.set(fontSize.rawValue, forKey: StorageKey.fontSize)
        FawxTypography.setScale(fontSize.scale)
    }

    private func shouldNotifyForRipcordActivation(
        from previousStatus: RipcordStatusResponse?,
        to currentStatus: RipcordStatusResponse
    ) -> Bool {
        currentStatus.active && previousStatus?.active != true
    }

    private func postRipcordNotification(for status: RipcordStatusResponse) {
        let title = "Fawx - Tripwire Crossed"
        let subtitle = "\"\(status.displayDescription)\""
        let body = "Actions are being journaled. Review when ready."

        Task { @MainActor [weak self] in
            guard let self else {
                return
            }

            let center = UNUserNotificationCenter.current()
            if !hasRequestedNotificationAuthorization {
                hasRequestedNotificationAuthorization = true
                _ = try? await center.requestAuthorization(options: [.alert, .sound])
            }

            let content = UNMutableNotificationContent()
            content.title = title
            content.subtitle = subtitle
            content.body = body
            content.sound = .default

            let identifier = "ripcord-\(status.tripwireId ?? UUID().uuidString)"
            let request = UNNotificationRequest(identifier: identifier, content: content, trigger: nil)
            try? await center.add(request)
        }
    }

    private func clearRipcordState() {
        ripcordStatus = nil
    }

    private var rootDestinationKey: String {
        switch rootDestination {
        case .main:
            "main"
        case .setupWizard:
            "setup"
        case .remoteOnboarding:
            "remote"
        }
    }

    private func resolvePermissionPreset(from config: JSONValue?) -> String {
        let rawPreset = config?
            .value(at: ["permissions", "preset"])?
            .stringValue

        return permissionPresetLabel(rawPreset)
    }

    private func waitForLocalServerReconnect() async {
        for _ in 0 ..< 12 {
            if Task.isCancelled {
                return
            }

            do {
                _ = try await client.health()
                return
            } catch {
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private func attemptConnection(initialStatus: ConnectionStatus, allowReconnect: Bool) async {
        connectionStatus = initialStatus

        do {
            lastHealth = try await client.health()
            try await refreshServerState()
            handleConnectionRecovered(showsToast: initialStatus != .connecting)
        } catch {
            await handleConnectionFailure(error, allowReconnect: allowReconnect)
        }
    }

    private func handleConnectionRecovered(showsToast: Bool) {
        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0
        connectionStatus = .connected
        connectionError = nil

        if showsToast {
            showToast(message: "Connection restored", style: .success)
        }
    }

    private func handleConnectionFailure(_ error: Error, allowReconnect: Bool) async {
        connectionError = connectionMessage(for: error)
        clearRipcordState()

        switch ConnectionStateMachine.failureStatus(for: error, allowReconnect: allowReconnect) {
        case .reconnecting:
            connectionStatus = .reconnecting
            scheduleReconnectIfNeeded()
        case .disconnected, .connecting, .connected:
            reconnectTask?.cancel()
            reconnectTask = nil
            connectionStatus = .disconnected
        }
    }

    private func scheduleReconnectIfNeeded() {
        guard reconnectTask == nil, isConfigured else {
            return
        }

        reconnectTask = Task { @MainActor [weak self] in
            guard let self else {
                return
            }

            while !Task.isCancelled && self.isConfigured {
                let delaySeconds = min(pow(2, Double(self.reconnectAttempt)), 30)
                do {
                    try await Task.sleep(for: .seconds(delaySeconds))
                } catch is CancellationError {
                    return
                } catch {
                    return
                }

                guard !Task.isCancelled else {
                    return
                }

                do {
                    self.lastHealth = try await self.client.health()
                    guard !Task.isCancelled else {
                        return
                    }
                    try await self.refreshServerState()
                    guard !Task.isCancelled else {
                        return
                    }
                    self.handleConnectionRecovered(showsToast: true)
                    return
                } catch is CancellationError {
                    return
                } catch {
                    guard !Task.isCancelled else {
                        return
                    }
                    self.reconnectAttempt = min(self.reconnectAttempt + 1, 5)
                    self.connectionError = self.connectionMessage(for: error)
                    let nextStatus = ConnectionStateMachine.retryFailureStatus(
                        for: error,
                        reconnectAttempt: self.reconnectAttempt
                    )
                    self.connectionStatus = nextStatus

                    if nextStatus == .disconnected {
                        self.reconnectTask = nil
                        return
                    }
                }
            }
        }
    }

    private func isAuthenticationFailure(_ error: Error) -> Bool {
        ConnectionStateMachine.issueKind(for: error) == .authentication
    }

    private func isConnectivityFailure(_ error: Error) -> Bool {
        ConnectionStateMachine.issueKind(for: error) == .connectivity
    }

    private func connectionMessage(for error: Error) -> String {
        let destination = serverURLString.isEmpty ? "your Fawx server" : serverURLString

        if isAuthenticationFailure(error) {
            return "Authentication failed. Check your pairing in Settings."
        }

        if isConnectivityFailure(error) {
            return "Fawx server at \(destination) is offline or unreachable."
        }

        return "Fawx server at \(destination) returned an unexpected response."
    }

    func showToast(message: String, style: AppToastStyle) {
        toastDismissTask?.cancel()
        toast = AppToast(message: message, style: style)

        toastDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(3))
            self?.toast = nil
        }
    }

    private func reloadLocalInstallConfiguration() {
#if os(macOS)
        if let configuration = LocalInstallConfiguration.loadDefault() {
            localInstallConfiguration = configuration
        }
#endif
    }

    private func persistConnectionMode() {
        UserDefaults.standard.set(connectionMode.rawValue, forKey: StorageKey.connectionMode)
    }

    private static func resolveInitialDestination(
        isConfigured: Bool,
        setupComplete: Bool,
        connectionMode: AppConnectionMode,
        hasStoredServerURL: Bool,
        hasLocalInstall: Bool
    ) -> AppRootDestination {
#if os(iOS)
        return isConfigured ? .main : .remoteOnboarding
#else
        if isConfigured && (setupComplete || hasLocalInstall || connectionMode == .remote) {
            return .main
        }
        if hasLocalInstall {
            return .main
        }
        if connectionMode == .remote || hasStoredServerURL {
            return .remoteOnboarding
        }
        return .setupWizard
#endif
    }

    private static func defaultConnectionMode(forStoredServerURL storedServerURL: String) -> AppConnectionMode {
#if os(iOS)
        .remote
#else
        storedServerURL.isEmpty ? .local : .remote
#endif
    }

    private static func defaultLocalDeviceName() -> String {
#if os(macOS)
        if let localizedName = Host.current().localizedName, !localizedName.isEmpty {
            return localizedName
        }
#endif
        return "This Mac"
    }

    private static func resetPersistedConfiguration() {
        let defaults = UserDefaults.standard
        let storedServerURL = defaults.string(forKey: StorageKey.serverURL) ?? ""

        if !storedServerURL.isEmpty {
            try? KeychainHelper.deleteToken(forServer: storedServerURL)
        }

        defaults.removeObject(forKey: StorageKey.serverURL)
        defaults.removeObject(forKey: StorageKey.pairedDeviceName)
        defaults.removeObject(forKey: StorageKey.theme)
        defaults.removeObject(forKey: StorageKey.fontSize)
        defaults.removeObject(forKey: StorageKey.setupComplete)
        defaults.removeObject(forKey: StorageKey.connectionMode)
    }
}
