import Observation
import SwiftUI

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
            return 0.92
        case .medium:
            return 1.0
        case .large:
            return 1.12
        }
    }

    var sliderValue: Double {
        switch self {
        case .small:
            return 0
        case .medium:
            return 1
        case .large:
            return 2
        }
    }

    static func fromSliderValue(_ value: Double) -> AppFontSize {
        switch Int(value.rounded()) {
        case ...0:
            return .small
        case 2...:
            return .large
        default:
            return .medium
        }
    }
}

enum AppToastStyle: Sendable, Equatable {
    case info
    case success
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

@MainActor
@Observable
final class AppState {
    private enum StorageKey {
        static let serverURL = "server_url"
        static let pairedDeviceName = "paired_device_name"
        static let theme = "theme"
        static let fontSize = "font_size"
    }

    var connectionStatus: ConnectionStatus = .disconnected
    var serverURLString: String
    var pairedDeviceName: String?
    var activeModel: ModelInfo?
    var thinkingLevel: ThinkingLevel?
    var availableModels: [ModelInfo] = []
    var skills: [SkillSummary] = []
    var authProviders: [AuthProvider] = []
    var lastHealth: HealthResponse?
    var currentContext: ContextInfo?
    var permissionPresetName = "Power User"
    var connectionError: String?
    var theme: AppTheme
    var fontSize: AppFontSize
    var isUpdatingServerSettings = false
    var authProvidersError: String?
    var sidebarSelection: SidebarSelection?
    var toast: AppToast?

    let client: FawxClient
    private var authToken: String?
    @ObservationIgnored private var reconnectTask: Task<Void, Never>?
    @ObservationIgnored private var toastDismissTask: Task<Void, Never>?
    @ObservationIgnored private var reconnectAttempt = 0

    init() {
        if UITestLaunchOptions.shouldResetState {
            Self.resetPersistedConfiguration()
        }

        let storedServerURL = UserDefaults.standard.string(forKey: StorageKey.serverURL) ?? ""
        let storedTheme = AppTheme(rawValue: UserDefaults.standard.string(forKey: StorageKey.theme) ?? AppTheme.system.rawValue) ?? .system
        let storedFontSize = AppFontSize(rawValue: UserDefaults.standard.string(forKey: StorageKey.fontSize) ?? AppFontSize.medium.rawValue) ?? .medium
        let storedToken = try? KeychainHelper.token(forServer: storedServerURL)
        let storedDeviceName = UserDefaults.standard.string(forKey: StorageKey.pairedDeviceName)

        let resolvedServerURL = UITestLaunchOptions.serverURLOverride ?? storedServerURL
        let resolvedToken = UITestLaunchOptions.bearerTokenOverride ?? storedToken ?? nil
        let resolvedDeviceName = UITestLaunchOptions.pairedDeviceNameOverride
            ?? storedDeviceName
            ?? (UITestLaunchOptions.isUITesting && resolvedToken != nil ? "UI Test Device" : nil)

        self.serverURLString = resolvedServerURL
        self.pairedDeviceName = resolvedDeviceName
        self.theme = storedTheme
        self.fontSize = storedFontSize
        self.authToken = resolvedToken
        self.client = FawxClient(
            baseURL: URL(string: resolvedServerURL),
            bearerToken: resolvedToken
        )
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

    var configurationKey: String {
        [
            serverURLString,
            pairedDeviceName ?? "",
            isConfigured ? "paired" : "unpaired",
        ].joined(separator: "|")
    }

    var preferredColorScheme: ColorScheme? {
        theme.colorScheme
    }

    var connectionBanner: ConnectionBannerState? {
        guard isConfigured, let connectionError, !connectionError.isEmpty else {
            if connectionStatus == .reconnecting {
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

    func bootstrap() async {
        guard isConfigured else {
            reconnectTask?.cancel()
            reconnectTask = nil
            connectionStatus = .disconnected
            return
        }

        reconnectAttempt = 0
        await attemptConnection(initialStatus: .connecting, allowReconnect: true)
    }

    func savePairing(serverURLString rawURL: String, token: String, deviceName: String) async throws {
        guard let canonicalURLString = canonicalizeServerURL(rawURL) else {
            throw APIError.invalidURL(rawURL)
        }
        guard let url = URL(string: canonicalURLString) else {
            throw APIError.invalidURL(canonicalURLString)
        }

        serverURLString = canonicalURLString
        pairedDeviceName = deviceName
        authToken = token

        UserDefaults.standard.set(canonicalURLString, forKey: StorageKey.serverURL)
        UserDefaults.standard.set(deviceName, forKey: StorageKey.pairedDeviceName)
        try KeychainHelper.saveToken(token, forServer: canonicalURLString)
        await client.updateConfiguration(baseURL: url, bearerToken: token)
        connectionError = nil
        connectionStatus = .connected
    }

    func unpair() async throws {
        if !serverURLString.isEmpty {
            try KeychainHelper.deleteToken(forServer: serverURLString)
        }

        authToken = nil
        pairedDeviceName = nil
        lastHealth = nil
        activeModel = nil
        thinkingLevel = nil
        currentContext = nil
        availableModels = []
        skills = []
        authProviders = []
        permissionPresetName = "Power User"
        connectionError = nil
        connectionStatus = .disconnected

        UserDefaults.standard.removeObject(forKey: StorageKey.pairedDeviceName)
        await client.updateConfiguration(baseURL: URL(string: serverURLString), bearerToken: nil)
    }

    func storedToken() -> String {
        authToken ?? ""
    }

    func refreshServerState() async throws {
        async let modelsTask = client.listModels()
        async let statusTask = client.serverStatus()
        let models = try await modelsTask

        let status = try? await statusTask
        do {
            let thinking = try await client.thinking()
            thinkingLevel = thinking.level
        } catch {
            thinkingLevel = nil
        }
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

        availableModels = models.models
        let activeModelID = status?.model ?? models.activeModel
        activeModel = models.models.first(where: { $0.modelID == activeModelID })
            ?? models.models.first
        permissionPresetName = resolvePermissionPreset(from: status?.config)
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
            return
        }

        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0

        let initialStatus: ConnectionStatus = allowReconnect ? .reconnecting : .connecting
        await attemptConnection(initialStatus: initialStatus, allowReconnect: allowReconnect)
    }

    func markDisconnected(from error: Error) {
        reconnectTask?.cancel()
        reconnectTask = nil
        reconnectAttempt = 0
        lastHealth = nil
        connectionError = connectionMessage(for: error)
        connectionStatus = .disconnected
    }

    func userFacingConnectionMessage(for error: Error) -> String {
        connectionMessage(for: error)
    }

    func noteRecoverableRequestFailure(_ error: Error) async {
        guard shouldReconnect(for: error) else {
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

        _ = try await client.setModel(modelID)
        try await refreshServerState()
    }

    func setThinking(_ level: ThinkingLevel) async throws {
        isUpdatingServerSettings = true
        defer { isUpdatingServerSettings = false }

        _ = try await client.setThinking(level)
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

    private func resolvePermissionPreset(from config: JSONValue?) -> String {
        let rawPreset = config?
            .value(at: ["permissions", "preset"])?
            .stringValue

        return permissionPresetLabel(rawPreset)
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

        if isAuthenticationFailure(error) {
            reconnectTask?.cancel()
            reconnectTask = nil
            connectionStatus = .disconnected
            return
        }

        if allowReconnect {
            connectionStatus = .reconnecting
            scheduleReconnectIfNeeded()
        } else {
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
                let delaySeconds = min(pow(2.0, Double(self.reconnectAttempt)), 30)
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
                    self.reconnectAttempt += 1
                    self.connectionError = self.connectionMessage(for: error)

                    if self.isAuthenticationFailure(error) || self.reconnectAttempt >= 5 {
                        self.connectionStatus = .disconnected
                        self.reconnectTask = nil
                        return
                    }

                    self.connectionStatus = .reconnecting
                }
            }
        }
    }

    private func shouldReconnect(for error: Error) -> Bool {
        isAuthenticationFailure(error) || isConnectivityFailure(error)
    }

    private func isAuthenticationFailure(_ error: Error) -> Bool {
        if case APIError.httpStatus(let code, _) = error, code == 401 {
            return true
        }
        return false
    }

    private func isConnectivityFailure(_ error: Error) -> Bool {
        if let urlError = error as? URLError {
            switch urlError.code {
            case .timedOut,
                    .cannotFindHost,
                    .cannotConnectToHost,
                    .networkConnectionLost,
                    .dnsLookupFailed,
                    .notConnectedToInternet:
                return true
            default:
                return false
            }
        }

        if case APIError.invalidResponse = error {
            return true
        }

        if case APIError.httpStatus(let code, _) = error, code == 408 || (500 ... 599).contains(code) {
            return true
        }

        return false
    }

    private func connectionMessage(for error: Error) -> String {
        if isAuthenticationFailure(error) {
            return "Authentication failed. Check your pairing in Settings."
        }

        if isConnectivityFailure(error) {
            return "Fawx server at \(serverURLString) is offline or unreachable."
        }

        return "Fawx server at \(serverURLString) returned an unexpected response."
    }

    private func showToast(message: String, style: AppToastStyle) {
        toastDismissTask?.cancel()
        toast = AppToast(message: message, style: style)

        toastDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(3))
            self?.toast = nil
        }
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
    }
}
