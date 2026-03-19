import Foundation

enum AppStateStorageKey {
    static let serverURL = "server_url"
    static let pairedDeviceName = "paired_device_name"
    static let theme = "theme"
    static let fontSize = "font_size"
    static let setupComplete = "setup_complete"
    static let connectionMode = "connection_mode"
}

actor AppStatePersistence {
    struct LaunchSnapshot: Sendable {
        let storedServerURL: String
        let pairedDeviceName: String?
        let theme: AppTheme
        let fontSize: AppFontSize
        let setupComplete: Bool
        let connectionModeRawValue: String?
        let authToken: String?
        let localInstallConfiguration: LocalInstallConfiguration?
    }

    private let defaultsSuiteName: String?
    private let keychainService: String
    private let localInstallLoader: @Sendable () async -> LocalInstallConfiguration?

    init(
        defaultsSuiteName: String? = nil,
        keychainService: String = KeychainHelper.defaultService,
        localInstallLoader: @escaping @Sendable () async -> LocalInstallConfiguration? = {
            await LocalInstallConfiguration.loadDefault()
        }
    ) {
        self.defaultsSuiteName = defaultsSuiteName
        self.keychainService = keychainService
        self.localInstallLoader = localInstallLoader
    }

    static func defaultStore() -> AppStatePersistence {
        if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil {
            return AppStatePersistence(
                defaultsSuiteName: AppStateTestIsolation.makeSuiteName(),
                keychainService: AppStateTestIsolation.makeKeychainService(),
                localInstallLoader: { nil }
            )
        }

        return AppStatePersistence()
    }

    func loadLaunchSnapshot(resetState: Bool) async -> LaunchSnapshot {
        let userDefaults = defaults

        if resetState {
            resetPersistedConfiguration(userDefaults: userDefaults)
        }

        let storedServerURL = userDefaults.string(forKey: AppStateStorageKey.serverURL) ?? ""
        let authToken = storedServerURL.isEmpty
            ? nil
            : (try? KeychainHelper.token(forServer: storedServerURL, service: keychainService))

        return LaunchSnapshot(
            storedServerURL: storedServerURL,
            pairedDeviceName: userDefaults.string(forKey: AppStateStorageKey.pairedDeviceName),
            theme: storedTheme(),
            fontSize: storedFontSize(),
            setupComplete: userDefaults.bool(forKey: AppStateStorageKey.setupComplete),
            connectionModeRawValue: userDefaults.string(forKey: AppStateStorageKey.connectionMode),
            authToken: authToken,
            localInstallConfiguration: await localInstallLoader()
        )
    }

    func loadLocalInstallConfiguration() async -> LocalInstallConfiguration? {
        await localInstallLoader()
    }

    func savePairing(
        serverURLString: String,
        token: String,
        deviceName: String,
        connectionMode: AppConnectionMode
    ) throws {
        let userDefaults = defaults
        userDefaults.set(serverURLString, forKey: AppStateStorageKey.serverURL)
        userDefaults.set(deviceName, forKey: AppStateStorageKey.pairedDeviceName)
        userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
        try KeychainHelper.saveToken(token, forServer: serverURLString, service: keychainService)
    }

    func clearPairing(serverURLString: String) {
        let userDefaults = defaults
        if !serverURLString.isEmpty {
            try? KeychainHelper.deleteToken(forServer: serverURLString, service: keychainService)
        }

        userDefaults.removeObject(forKey: AppStateStorageKey.serverURL)
        userDefaults.removeObject(forKey: AppStateStorageKey.pairedDeviceName)
    }

    func persistResolvedLaunchState(
        setupComplete: Bool,
        connectionMode: AppConnectionMode
    ) {
        let userDefaults = defaults
        if setupComplete {
            userDefaults.set(true, forKey: AppStateStorageKey.setupComplete)
        }

        userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
    }

    func setTheme(_ theme: AppTheme) {
        defaults.set(theme.rawValue, forKey: AppStateStorageKey.theme)
    }

    func setFontSize(_ fontSize: AppFontSize) {
        defaults.set(fontSize.rawValue, forKey: AppStateStorageKey.fontSize)
    }

    func setConnectionMode(_ connectionMode: AppConnectionMode) {
        defaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
    }

    func setSetupComplete(_ setupComplete: Bool) {
        defaults.set(setupComplete, forKey: AppStateStorageKey.setupComplete)
    }

    private var defaults: UserDefaults {
        if let defaultsSuiteName, let userDefaults = UserDefaults(suiteName: defaultsSuiteName) {
            return userDefaults
        }

        if let defaultsSuiteName {
            NSLog(
                "Couldn't open AppState defaults suite %@. Falling back to standard defaults.",
                defaultsSuiteName
            )
        }

        return .standard
    }

    private func storedTheme() -> AppTheme {
        AppTheme(
            rawValue: defaults.string(forKey: AppStateStorageKey.theme) ?? AppTheme.system.rawValue
        ) ?? .system
    }

    private func storedFontSize() -> AppFontSize {
        AppFontSize(
            rawValue: defaults.string(forKey: AppStateStorageKey.fontSize) ?? AppFontSize.medium.rawValue
        ) ?? .medium
    }

    private func resetPersistedConfiguration(userDefaults: UserDefaults) {
        let storedServerURL = userDefaults.string(forKey: AppStateStorageKey.serverURL) ?? ""

        if !storedServerURL.isEmpty {
            try? KeychainHelper.deleteToken(forServer: storedServerURL, service: keychainService)
        }

        userDefaults.removeObject(forKey: AppStateStorageKey.serverURL)
        userDefaults.removeObject(forKey: AppStateStorageKey.pairedDeviceName)
        userDefaults.removeObject(forKey: AppStateStorageKey.theme)
        userDefaults.removeObject(forKey: AppStateStorageKey.fontSize)
        userDefaults.removeObject(forKey: AppStateStorageKey.setupComplete)
        userDefaults.removeObject(forKey: AppStateStorageKey.connectionMode)
    }
}

private enum AppStateTestIsolation {
    static func makeSuiteName() -> String {
        "ai.fawx.app.tests.\(UUID().uuidString)"
    }

    static func makeKeychainService() -> String {
        "ai.fawx.app.tests.\(UUID().uuidString)"
    }
}
