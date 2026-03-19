import XCTest
@testable import Fawx

final class AppStateTests: XCTestCase {
    func testAwaitPersistedStateLoadAppliesStoredPairingAndAppearance() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let serverURL = "https://example.com:8400"

        defaults.set(serverURL, forKey: AppStateStorageKey.serverURL)
        defaults.set("Desk Mac", forKey: AppStateStorageKey.pairedDeviceName)
        defaults.set(AppTheme.dark.rawValue, forKey: AppStateStorageKey.theme)
        defaults.set(AppFontSize.large.rawValue, forKey: AppStateStorageKey.fontSize)
        defaults.set(true, forKey: AppStateStorageKey.setupComplete)
        defaults.set(AppConnectionMode.remote.rawValue, forKey: AppStateStorageKey.connectionMode)
        try KeychainHelper.saveToken("stored-token", forServer: serverURL, service: keychainService)
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await sut.awaitPersistedStateLoad()

        await MainActor.run {
            XCTAssertEqual(sut.serverURLString, serverURL)
            XCTAssertEqual(sut.pairedDeviceName, "Desk Mac")
            XCTAssertEqual(sut.theme, .dark)
            XCTAssertEqual(sut.fontSize, .large)
            XCTAssertEqual(sut.connectionMode, .remote)
            XCTAssertTrue(sut.isConfigured)
            XCTAssertEqual(sut.rootDestination, .main)
        }
    }

#if os(macOS)
    func testAwaitPersistedStateLoadUsesDetectedLocalInstallAndPersistsBootstrapFlags() async {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let localInstall = LocalInstallConfiguration(
            host: "127.0.0.1",
            port: 9500,
            bearerToken: "local-token",
            dataDirectoryURL: URL(fileURLWithPath: "/tmp/\(UUID().uuidString)", isDirectory: true)
        )

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { localInstall }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await sut.awaitPersistedStateLoad()

        await MainActor.run {
            XCTAssertEqual(sut.serverURLString, localInstall.baseURLString)
            XCTAssertEqual(sut.connectionMode, .local)
            XCTAssertEqual(sut.localInstallConfiguration, localInstall)
            XCTAssertTrue(sut.isConfigured)
            XCTAssertEqual(sut.rootDestination, .main)
        }
        XCTAssertTrue(defaults.bool(forKey: AppStateStorageKey.setupComplete))
        XCTAssertEqual(
            defaults.string(forKey: AppStateStorageKey.connectionMode),
            AppConnectionMode.local.rawValue
        )
    }
#endif

    func testSavePairingPersistsDefaultsAndKeychain() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let serverURL = "https://remote.example.com:8400"
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence, startLoadingPersistedState: false)
        }

        try await sut.savePairing(
            serverURLString: serverURL,
            token: "new-token",
            deviceName: "Remote Mac",
            connectionMode: AppConnectionMode.remote
        )

        await MainActor.run {
            XCTAssertEqual(sut.serverURLString, serverURL)
            XCTAssertEqual(sut.pairedDeviceName, "Remote Mac")
            XCTAssertEqual(sut.connectionMode, .remote)
        }
        XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.serverURL), serverURL)
        XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.pairedDeviceName), "Remote Mac")
        XCTAssertEqual(
            defaults.string(forKey: AppStateStorageKey.connectionMode),
            AppConnectionMode.remote.rawValue
        )
        XCTAssertEqual(
            try KeychainHelper.token(forServer: serverURL, service: keychainService),
            "new-token"
        )
    }

    func testUnpairClearsPersistedPairingAndRoutesRemoteUsersToOnboarding() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let serverURL = "https://remote.example.com:8400"

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence, startLoadingPersistedState: false)
        }

        try await sut.savePairing(
            serverURLString: serverURL,
            token: "pairing-token",
            deviceName: "Remote Mac",
            connectionMode: .remote
        )

        try await sut.unpair()

        await MainActor.run {
            XCTAssertFalse(sut.isConfigured)
            XCTAssertNil(sut.pairedDeviceName)
            XCTAssertEqual(sut.connectionMode, .remote)
            XCTAssertEqual(sut.rootDestination, .remoteOnboarding)
        }
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.serverURL))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.pairedDeviceName))
        XCTAssertEqual(
            defaults.string(forKey: AppStateStorageKey.connectionMode),
            AppConnectionMode.remote.rawValue
        )
        XCTAssertNil(try KeychainHelper.token(forServer: serverURL, service: keychainService))
    }

#if os(macOS)
    func testUnpairKeepsLocalInstallUsersInMainExperience() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let keychainService = uniqueKeychainService()
        let localInstall = LocalInstallConfiguration(
            host: "127.0.0.1",
            port: 9500,
            bearerToken: "local-token",
            dataDirectoryURL: URL(fileURLWithPath: "/tmp/\(UUID().uuidString)", isDirectory: true)
        )

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { localInstall }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await sut.awaitPersistedStateLoad()
        try await sut.savePairing(
            serverURLString: localInstall.baseURLString,
            token: "updated-local-token",
            deviceName: "Local Mac",
            connectionMode: .local
        )

        try await sut.unpair()

        await MainActor.run {
            XCTAssertEqual(sut.connectionMode, .local)
            XCTAssertEqual(sut.rootDestination, .main)
        }
    }
#endif

    func testLoadLaunchSnapshotResetStateClearsPersistedConfiguration() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let serverURL = "https://example.com:8400"

        defaults.set(serverURL, forKey: AppStateStorageKey.serverURL)
        defaults.set("Desk Mac", forKey: AppStateStorageKey.pairedDeviceName)
        defaults.set(AppTheme.dark.rawValue, forKey: AppStateStorageKey.theme)
        defaults.set(AppFontSize.large.rawValue, forKey: AppStateStorageKey.fontSize)
        defaults.set(true, forKey: AppStateStorageKey.setupComplete)
        defaults.set(AppConnectionMode.remote.rawValue, forKey: AppStateStorageKey.connectionMode)
        try KeychainHelper.saveToken("stored-token", forServer: serverURL, service: keychainService)

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )

        let snapshot = await persistence.loadLaunchSnapshot(resetState: true)

        XCTAssertEqual(snapshot.storedServerURL, "")
        XCTAssertNil(snapshot.pairedDeviceName)
        XCTAssertEqual(snapshot.theme, .system)
        XCTAssertEqual(snapshot.fontSize, .medium)
        XCTAssertFalse(snapshot.setupComplete)
        XCTAssertNil(snapshot.connectionModeRawValue)
        XCTAssertNil(snapshot.authToken)
        XCTAssertNil(snapshot.localInstallConfiguration)
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.serverURL))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.pairedDeviceName))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.theme))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.fontSize))
        XCTAssertFalse(defaults.bool(forKey: AppStateStorageKey.setupComplete))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.connectionMode))
        XCTAssertNil(try KeychainHelper.token(forServer: serverURL, service: keychainService))
    }

    func testAwaitPersistedStateLoadDoesNotOverwriteStartupAppearanceChanges() async {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let loader = BlockingLocalInstallLoader()

        defaults.set(AppTheme.dark.rawValue, forKey: AppStateStorageKey.theme)
        defaults.set(AppFontSize.large.rawValue, forKey: AppStateStorageKey.fontSize)

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            localInstallLoader: { await loader.load() }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await loader.waitUntilStarted()

        await MainActor.run {
            sut.setTheme(.light)
            sut.setFontSize(.small)
        }

        await loader.release()
        await sut.awaitPersistedStateLoad()

        await MainActor.run {
            XCTAssertEqual(sut.theme, .light)
            XCTAssertEqual(sut.fontSize, .small)
        }
    }

    func testAwaitPersistedStateLoadDoesNotOverwriteBeginRemoteOnboarding() async {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let loader = BlockingLocalInstallLoader()

        defaults.set(AppConnectionMode.local.rawValue, forKey: AppStateStorageKey.connectionMode)

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            localInstallLoader: { await loader.load() }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await loader.waitUntilStarted()

        await MainActor.run {
            sut.beginRemoteOnboarding()
        }

        await loader.release()
        await sut.awaitPersistedStateLoad()

        await MainActor.run {
            XCTAssertEqual(sut.connectionMode, .remote)
            XCTAssertEqual(sut.rootDestination, .remoteOnboarding)
        }
    }

#if os(macOS)
    func testAwaitPersistedStateLoadDoesNotOverwriteReturnToLocalSetup() async {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let loader = BlockingLocalInstallLoader()

        defaults.set("https://remote.example.com:8400", forKey: AppStateStorageKey.serverURL)
        defaults.set(AppConnectionMode.remote.rawValue, forKey: AppStateStorageKey.connectionMode)

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            localInstallLoader: { await loader.load() }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence)
        }

        await loader.waitUntilStarted()

        await MainActor.run {
            sut.returnToLocalSetup()
        }

        await loader.release()
        await sut.awaitPersistedStateLoad()

        await MainActor.run {
            XCTAssertEqual(sut.connectionMode, .local)
            XCTAssertEqual(sut.rootDestination, .setupWizard)
        }
    }
#endif

    private func makeUserDefaults(suiteName: String) -> UserDefaults {
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            fatalError("Couldn't create AppState test defaults.")
        }
        return defaults
    }

    private func uniqueDefaultsSuiteName() -> String {
        "AppStateTests.\(UUID().uuidString)"
    }

    private func uniqueKeychainService() -> String {
        "ai.fawx.app.tests.\(UUID().uuidString)"
    }
}

private actor BlockingLocalInstallLoader {
    private var started = false
    private var startedContinuation: CheckedContinuation<Void, Never>?
    private var releaseContinuation: CheckedContinuation<Void, Never>?

    func load() async -> LocalInstallConfiguration? {
        started = true
        startedContinuation?.resume()
        startedContinuation = nil

        await withCheckedContinuation { continuation in
            releaseContinuation = continuation
        }

        return nil
    }

    func waitUntilStarted() async {
        guard !started else {
            return
        }

        await withCheckedContinuation { continuation in
            startedContinuation = continuation
        }
    }

    func release() {
        releaseContinuation?.resume()
        releaseContinuation = nil
    }
}
