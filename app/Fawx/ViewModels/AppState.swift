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

@MainActor
@Observable
final class AppState {
    private enum StorageKey {
        static let serverURL = "server_url"
        static let pairedDeviceName = "paired_device_name"
        static let theme = "theme"
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
    var isUpdatingServerSettings = false

    let client: FawxClient
    private var authToken: String?

    init() {
        if UITestLaunchOptions.shouldResetState {
            Self.resetPersistedConfiguration()
        }

        let storedServerURL = UserDefaults.standard.string(forKey: StorageKey.serverURL) ?? ""
        let storedTheme = AppTheme(rawValue: UserDefaults.standard.string(forKey: StorageKey.theme) ?? AppTheme.system.rawValue) ?? .system
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

    func bootstrap() async {
        guard isConfigured else {
            connectionStatus = .disconnected
            return
        }

        connectionStatus = .connecting
        do {
            lastHealth = try await client.health()
            connectionStatus = .connected
            connectionError = nil
            try await refreshServerState()
        } catch {
            connectionStatus = .disconnected
            connectionError = error.localizedDescription
        }
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

        availableModels = models.models
        let activeModelID = status?.model ?? models.activeModel
        activeModel = models.models.first(where: { $0.modelID == activeModelID })
            ?? models.models.first
        permissionPresetName = resolvePermissionPreset(from: status?.config)
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

    private func resolvePermissionPreset(from config: JSONValue?) -> String {
        let rawPreset = config?
            .value(at: ["permissions", "preset"])?
            .stringValue

        return permissionPresetLabel(rawPreset)
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
    }
}
