import Foundation

enum AppStateStorageKey {
    static let serverURL = "server_url"
    static let pairedDeviceName = "paired_device_name"
    static let theme = "theme"
    static let fontSize = "font_size"
    static let accentColor = "accent_color"
    static let setupComplete = "setup_complete"
    static let connectionMode = "connection_mode"
    static let favoriteModelIDs = "favorite_model_ids"
}

actor AppStatePersistence {
    struct LaunchSnapshot: Sendable {
        struct PersistedState: Sendable {
            let storedServerURL: String
            let pairedDeviceName: String?
            let theme: AppTheme
            let fontSize: AppFontSize
            let accentColor: AppAccentColor
            let setupComplete: Bool
            let connectionModeRawValue: String?
            let authToken: String?
            let favoriteModelIDs: Set<String>
        }

        let persistedState: PersistedState
        let localInstallConfiguration: LocalInstallConfiguration?

        var storedServerURL: String { persistedState.storedServerURL }
        var pairedDeviceName: String? { persistedState.pairedDeviceName }
        var theme: AppTheme { persistedState.theme }
        var fontSize: AppFontSize { persistedState.fontSize }
        var accentColor: AppAccentColor { persistedState.accentColor }
        var setupComplete: Bool { persistedState.setupComplete }
        var connectionModeRawValue: String? { persistedState.connectionModeRawValue }
        var authToken: String? { persistedState.authToken }
        var favoriteModelIDs: Set<String> { persistedState.favoriteModelIDs }
    }

    private let defaultsSuiteName: String?
    private let keychainService: String
    private let localInstallLoader: @Sendable () async -> LocalInstallConfiguration?
    private let offMainExecutionProbe: (@Sendable () -> Void)?

    init(
        defaultsSuiteName: String? = nil,
        keychainService: String = KeychainHelper.defaultService,
        localInstallLoader: @escaping @Sendable () async -> LocalInstallConfiguration? = {
            await LocalInstallConfiguration.loadDefault()
        },
        offMainExecutionProbe: (@Sendable () -> Void)? = nil
    ) {
        self.defaultsSuiteName = defaultsSuiteName
        self.keychainService = keychainService
        self.localInstallLoader = localInstallLoader
        self.offMainExecutionProbe = offMainExecutionProbe
    }

    static func defaultStore() -> AppStatePersistence {
        if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil {
            return AppStatePersistence(
                defaultsSuiteName: AppStateTestIsolation.makeSuiteName(),
                keychainService: AppStateTestIsolation.makeKeychainService(),
                localInstallLoader: { nil }
            )
        }

        if UITestLaunchOptions.isUITesting,
           UITestLaunchOptions.defaultsSuiteOverride != nil || UITestLaunchOptions.keychainServiceOverride != nil
        {
            return AppStatePersistence(
                defaultsSuiteName: UITestLaunchOptions.defaultsSuiteOverride,
                keychainService: UITestLaunchOptions.keychainServiceOverride ?? KeychainHelper.defaultService
            )
        }

        return AppStatePersistence()
    }

    func loadLaunchSnapshot(resetState: Bool) async -> LaunchSnapshot {
        async let persistedState = readPersistedState(resetState: resetState)
        async let localInstallConfiguration = localInstallLoader()

        return LaunchSnapshot(
            persistedState: await persistedState,
            localInstallConfiguration: await localInstallConfiguration
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
        let keychainService = self.keychainService

        try await runOffMain { userDefaults in
            userDefaults.set(serverURLString, forKey: AppStateStorageKey.serverURL)
            userDefaults.set(deviceName, forKey: AppStateStorageKey.pairedDeviceName)
            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
            try KeychainHelper.saveToken(token, forServer: serverURLString, service: keychainService)
        }
    }

    func clearPairing(serverURLString: String) async {
        let keychainService = self.keychainService

        await runOffMain { userDefaults in
            if !serverURLString.isEmpty {
                try? KeychainHelper.deleteToken(forServer: serverURLString, service: keychainService)
            }

            userDefaults.removeObject(forKey: AppStateStorageKey.serverURL)
            userDefaults.removeObject(forKey: AppStateStorageKey.pairedDeviceName)
        }
    }

    func persistResolvedLaunchState(
        setupComplete: Bool,
        connectionMode: AppConnectionMode
    ) async {
        await runOffMain { userDefaults in
            if setupComplete {
                userDefaults.set(true, forKey: AppStateStorageKey.setupComplete)
            }

            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
        }
    }

    func setTheme(_ theme: AppTheme) async {
        await runOffMain { userDefaults in
            userDefaults.set(theme.rawValue, forKey: AppStateStorageKey.theme)
        }
    }

    func setFontSize(_ fontSize: AppFontSize) async {
        await runOffMain { userDefaults in
            userDefaults.set(fontSize.rawValue, forKey: AppStateStorageKey.fontSize)
        }
    }

    func setAccentColor(_ accentColor: AppAccentColor) async {
        await runOffMain { userDefaults in
            userDefaults.set(accentColor.hexString, forKey: AppStateStorageKey.accentColor)
        }
    }

    func setConnectionMode(_ connectionMode: AppConnectionMode) async {
        await runOffMain { userDefaults in
            userDefaults.set(connectionMode.rawValue, forKey: AppStateStorageKey.connectionMode)
        }
    }

    func setSetupComplete(_ setupComplete: Bool) async {
        await runOffMain { userDefaults in
            userDefaults.set(setupComplete, forKey: AppStateStorageKey.setupComplete)
        }
    }

    func setFavoriteModelIDs(_ modelIDs: Set<String>) async {
        let normalizedModelIDs = modelIDs
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .sorted()

        await runOffMain { userDefaults in
            userDefaults.set(normalizedModelIDs, forKey: AppStateStorageKey.favoriteModelIDs)
        }
    }

    private func readPersistedState(resetState: Bool) async -> LaunchSnapshot.PersistedState {
        let keychainService = self.keychainService

        return await runOffMain { userDefaults in
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

            return LaunchSnapshot.PersistedState(
                storedServerURL: storedServerURL,
                pairedDeviceName: userDefaults.string(forKey: AppStateStorageKey.pairedDeviceName),
                theme: Self.storedTheme(userDefaults: userDefaults),
                fontSize: Self.storedFontSize(userDefaults: userDefaults),
                accentColor: Self.storedAccentColor(userDefaults: userDefaults),
                setupComplete: userDefaults.bool(forKey: AppStateStorageKey.setupComplete),
                connectionModeRawValue: userDefaults.string(forKey: AppStateStorageKey.connectionMode),
                authToken: authToken,
                favoriteModelIDs: Self.storedFavoriteModelIDs(userDefaults: userDefaults)
            )
        }
    }

    private func runOffMain<T: Sendable>(
        _ operation: @escaping @Sendable (UserDefaults) -> T
    ) async -> T {
        let defaultsSuiteName = self.defaultsSuiteName
        let offMainExecutionProbe = self.offMainExecutionProbe

        return await Task.detached(priority: .utility) {
            offMainExecutionProbe?()
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            return operation(userDefaults)
        }.value
    }

    private func runOffMain<T: Sendable>(
        _ operation: @escaping @Sendable (UserDefaults) throws -> T
    ) async throws -> T {
        let defaultsSuiteName = self.defaultsSuiteName
        let offMainExecutionProbe = self.offMainExecutionProbe

        return try await Task.detached(priority: .utility) {
            offMainExecutionProbe?()
            let userDefaults = Self.makeDefaults(suiteName: defaultsSuiteName)
            return try operation(userDefaults)
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

    private static func storedAccentColor(userDefaults: UserDefaults) -> AppAccentColor {
        guard let rawValue = userDefaults.string(forKey: AppStateStorageKey.accentColor),
              let accentColor = AppAccentColor(hexString: rawValue)
        else {
            return .default
        }
        return accentColor
    }

    private static func storedFavoriteModelIDs(userDefaults: UserDefaults) -> Set<String> {
        Set(
            (userDefaults.stringArray(forKey: AppStateStorageKey.favoriteModelIDs) ?? [])
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        )
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
        userDefaults.removeObject(forKey: AppStateStorageKey.accentColor)
        userDefaults.removeObject(forKey: AppStateStorageKey.setupComplete)
        userDefaults.removeObject(forKey: AppStateStorageKey.connectionMode)
        userDefaults.removeObject(forKey: AppStateStorageKey.favoriteModelIDs)
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
