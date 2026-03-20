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
    private struct DefaultsSnapshot: Sendable {
        let storedServerURL: String
        let pairedDeviceName: String?
        let theme: AppTheme
        let fontSize: AppFontSize
        let setupComplete: Bool
        let connectionModeRawValue: String?
        let authToken: String?
    }

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
        let defaultsSuiteName = self.defaultsSuiteName
        let keychainService = self.keychainService
        let defaultsSnapshot = await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)

            if resetState {
                Self.resetPersistedConfiguration(
                    userDefaults: userDefaults,
                    keychainService: keychainService
                )
            }

            let storedServerURL = userDefaults.string(forKey: AppStateStorageKey.serverURL) ?? ""
            let authToken = storedServerURL.isEmpty
                ? nil
                : (try? KeychainHelper.token(forServer: storedServerURL, service: keychainService))

            return DefaultsSnapshot(
                storedServerURL: storedServerURL,
                pairedDeviceName: userDefaults.string(forKey: AppStateStorageKey.pairedDeviceName),
                theme: Self.storedTheme(userDefaults: userDefaults),
                fontSize: Self.storedFontSize(userDefaults: userDefaults),
                setupComplete: userDefaults.bool(forKey: AppStateStorageKey.setupComplete),
                connectionModeRawValue: userDefaults.string(forKey: AppStateStorageKey.connectionMode),
                authToken: authToken
            )
        }.value

        return LaunchSnapshot(
            storedServerURL: defaultsSnapshot.storedServerURL,
            pairedDeviceName: defaultsSnapshot.pairedDeviceName,
            theme: defaultsSnapshot.theme,
            fontSize: defaultsSnapshot.fontSize,
            setupComplete: defaultsSnapshot.setupComplete,
            connectionModeRawValue: defaultsSnapshot.connectionModeRawValue,
            authToken: defaultsSnapshot.authToken,
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
    ) async throws {
        let defaultsSuiteName = self.defaultsSuiteName
        let keychainService = self.keychainService

        try await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            userDefaults.set(serverURLString, forKey: AppStateStorageKey.serverURL)
            userDefaults.set(deviceName, forKey: AppStateStorageKey.pairedDeviceName)
            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
            try KeychainHelper.saveToken(token, forServer: serverURLString, service: keychainService)
        }.value
    }

    func clearPairing(serverURLString: String) async {
        let defaultsSuiteName = self.defaultsSuiteName
        let keychainService = self.keychainService

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            if !serverURLString.isEmpty {
                try? KeychainHelper.deleteToken(forServer: serverURLString, service: keychainService)
            }

            userDefaults.removeObject(forKey: AppStateStorageKey.serverURL)
            userDefaults.removeObject(forKey: AppStateStorageKey.pairedDeviceName)
        }.value
    }

    func persistResolvedLaunchState(
        setupComplete: Bool,
        connectionMode: AppConnectionMode
    ) async {
        let defaultsSuiteName = self.defaultsSuiteName

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            if setupComplete {
                userDefaults.set(true, forKey: AppStateStorageKey.setupComplete)
            }

            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
        }.value
    }

    func setTheme(_ theme: AppTheme) async {
        let defaultsSuiteName = self.defaultsSuiteName

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            userDefaults.set(theme.rawValue, forKey: AppStateStorageKey.theme)
        }.value
    }

    func setFontSize(_ fontSize: AppFontSize) async {
        let defaultsSuiteName = self.defaultsSuiteName

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            userDefaults.set(fontSize.rawValue, forKey: AppStateStorageKey.fontSize)
        }.value
    }

    func setConnectionMode(_ connectionMode: AppConnectionMode) async {
        let defaultsSuiteName = self.defaultsSuiteName

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
        }.value
    }

    func setSetupComplete(_ setupComplete: Bool) async {
        let defaultsSuiteName = self.defaultsSuiteName

        await Task.detached(priority: .utility) {
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            userDefaults.set(setupComplete, forKey: AppStateStorageKey.setupComplete)
        }.value
    }

    private static func makeDefaults(suiteName defaultsSuiteName: String?) -> UserDefaults {
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

    private static func storedTheme(userDefaults: UserDefaults) -> AppTheme {
        AppTheme(
            rawValue: userDefaults.string(forKey: AppStateStorageKey.theme) ?? AppTheme.system.rawValue
        ) ?? .system
    }

    private static func storedFontSize(userDefaults: UserDefaults) -> AppFontSize {
        AppFontSize(
            rawValue: userDefaults.string(forKey: AppStateStorageKey.fontSize) ?? AppFontSize.medium.rawValue
        ) ?? .medium
    }

    private static func resetPersistedConfiguration(
        userDefaults: UserDefaults,
        keychainService: String
    ) {
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
