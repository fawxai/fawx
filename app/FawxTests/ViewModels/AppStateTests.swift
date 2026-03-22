import SwiftUI
import XCTest
@testable import Fawx

final class AppStatePersistenceTests: XCTestCase {
    func testLoadLaunchSnapshotReadsDefaultsOffMainThread() async {
        let probe = OffMainExecutionProbe()
        let persistence = AppStatePersistence(
            defaultsSuiteName: uniqueDefaultsSuiteName(),
            localInstallLoader: { nil },
            offMainExecutionProbe: { probe.record() }
        )

        _ = await persistence.loadLaunchSnapshot(resetState: false)

        probe.assertObservedOffMain()
    }

    func testSavePairingWritesOffMainThread() async throws {
        let probe = OffMainExecutionProbe()
        let keychainService = uniqueKeychainService()
        let serverURL = "https://remote.example.com:8400"
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        let persistence = AppStatePersistence(
            defaultsSuiteName: uniqueDefaultsSuiteName(),
            keychainService: keychainService,
            localInstallLoader: { nil },
            offMainExecutionProbe: { probe.record() }
        )

        try await persistence.savePairing(
            serverURLString: serverURL,
            token: "new-token",
            deviceName: "Remote Mac",
            connectionMode: .remote
        )

        probe.assertObservedOffMain()
    }

    func testClearPairingRemovesPersistedPairingAndToken() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let serverURL = "https://remote.example.com:8400"
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        defaults.set(serverURL, forKey: AppStateStorageKey.serverURL)
        defaults.set("Remote Mac", forKey: AppStateStorageKey.pairedDeviceName)
        try KeychainHelper.saveToken("stored-token", forServer: serverURL, service: keychainService)

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )

        await persistence.clearPairing(serverURLString: serverURL)

        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.serverURL))
        XCTAssertNil(defaults.string(forKey: AppStateStorageKey.pairedDeviceName))
        XCTAssertNil(try KeychainHelper.token(forServer: serverURL, service: keychainService))
    }

    func testPersistenceSettersStillWriteExpectedDefaults() async {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            localInstallLoader: { nil }
        )

        await persistence.setTheme(.dark)
        await persistence.setFontSize(.large)
        await persistence.setConnectionMode(.local)
        await persistence.setSetupComplete(true)

        XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.theme), AppTheme.dark.rawValue)
        XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.fontSize), AppFontSize.large.rawValue)
        XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.connectionMode), AppConnectionMode.local.rawValue)
        XCTAssertTrue(defaults.bool(forKey: AppStateStorageKey.setupComplete))
    }

#if os(macOS)
    func testLoadLaunchSnapshotStartsDefaultsAndLocalInstallLoadsConcurrently() async {
        let defaultsReadStarted = expectation(description: "defaults read started")
        let probe = OffMainExecutionProbe(blocking: true, startedExpectation: defaultsReadStarted)
        let loader = BlockingLocalInstallLoader()
        let persistence = AppStatePersistence(
            defaultsSuiteName: uniqueDefaultsSuiteName(),
            localInstallLoader: { await loader.load() },
            offMainExecutionProbe: { probe.record() }
        )

        let task = Task {
            await persistence.loadLaunchSnapshot(resetState: false)
        }

        await fulfillment(of: [defaultsReadStarted], timeout: 1)
        await loader.waitUntilStarted()
        probe.release()
        await loader.release()

        let snapshot = await task.value
        XCTAssertNil(snapshot.localInstallConfiguration)
        probe.assertObservedOffMain()
    }
#endif

    private func makeUserDefaults(suiteName: String) -> UserDefaults {
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            fatalError("Couldn't create AppStatePersistence test defaults.")
        }
        return defaults
    }

    private func uniqueDefaultsSuiteName() -> String {
        "AppStatePersistenceTests.\(UUID().uuidString)"
    }

    private func uniqueKeychainService() -> String {
        "ai.fawx.app.persistence.tests.\(UUID().uuidString)"
    }
}

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

    func testAwaitPersistedStateLoadPrefersDetectedLocalInstallOverStaleSavedLocalURL() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
        let keychainService = uniqueKeychainService()
        let staleServerURL = "http://127.0.0.1:18400"
        let localInstall = LocalInstallConfiguration(
            host: "127.0.0.1",
            port: 8400,
            bearerToken: "local-token",
            dataDirectoryURL: URL(fileURLWithPath: "/tmp/\(UUID().uuidString)", isDirectory: true)
        )

        defaults.set(staleServerURL, forKey: AppStateStorageKey.serverURL)
        defaults.set(AppConnectionMode.local.rawValue, forKey: AppStateStorageKey.connectionMode)
        defaults.set("Desk Mac", forKey: AppStateStorageKey.pairedDeviceName)
        try KeychainHelper.saveToken("stale-token", forServer: staleServerURL, service: keychainService)
        defer { try? KeychainHelper.deleteToken(forServer: staleServerURL, service: keychainService) }

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
    }

    func testSynchronizeLocalConnectionIfNeededRepairsStaleRuntimeURL() async throws {
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let keychainService = uniqueKeychainService()
        let staleServerURL = "http://127.0.0.1:18400"
        let localInstall = LocalInstallConfiguration(
            host: "127.0.0.1",
            port: 8400,
            bearerToken: "local-token",
            dataDirectoryURL: URL(fileURLWithPath: "/tmp/\(UUID().uuidString)", isDirectory: true)
        )

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { localInstall }
        )
        let sut = await MainActor.run {
            AppState(persistence: persistence, startLoadingPersistedState: false)
        }

        try await sut.savePairing(
            serverURLString: staleServerURL,
            token: "stale-token",
            deviceName: "Desk Mac",
            connectionMode: .local
        )

        await sut.synchronizeLocalConnectionIfNeeded()

        await MainActor.run {
            XCTAssertEqual(sut.serverURLString, localInstall.baseURLString)
            XCTAssertEqual(sut.localInstallConfiguration, localInstall)
            XCTAssertTrue(sut.isConfigured)
        }
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

    func testRefreshServerStateCoalescesConcurrentCalls() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockAppStateURLProtocol.self]
        let session = URLSession(configuration: configuration)
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token",
            restSession: session,
            streamSession: session
        )
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let keychainService = uniqueKeychainService()
        let serverURL = "http://localhost:8400"
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        await MockAppStateURLProtocol.setResponder { request in
            switch request.url?.path {
            case "/v1/models":
                try await Task.sleep(for: .milliseconds(150))
                return .json(
                    """
                    {
                        "active_model": "gpt-5.4",
                        "models": [
                            {
                                "model_id": "gpt-5.4",
                                "provider": "openai",
                                "auth_method": "api_key"
                            }
                        ]
                    }
                    """
                )
            default:
                return .json("{}", statusCode: 404)
            }
        }

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )
        let sut = await MainActor.run {
            AppState(
                persistence: persistence,
                client: client,
                startLoadingPersistedState: false
            )
        }
        try await sut.savePairing(
            serverURLString: serverURL,
            token: "test-token",
            deviceName: "Desk Mac",
            connectionMode: .remote
        )

        async let firstRefresh: Void = sut.refreshServerState()
        async let secondRefresh: Void = sut.refreshServerState()
        try await firstRefresh
        try await secondRefresh

        let requests = await MockAppStateURLProtocol.recordedRequests()
        await MockAppStateURLProtocol.reset()

        XCTAssertEqual(
            requests.filter { $0.url?.path == "/v1/models" }.count,
            1
        )
    }

    func testRefreshRipcordStateCoalescesConcurrentCalls() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockAppStateURLProtocol.self]
        let session = URLSession(configuration: configuration)
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token",
            restSession: session,
            streamSession: session
        )
        let defaultsSuiteName = uniqueDefaultsSuiteName()
        let keychainService = uniqueKeychainService()
        let serverURL = "http://localhost:8400"
        defer { try? KeychainHelper.deleteToken(forServer: serverURL, service: keychainService) }

        await MockAppStateURLProtocol.setResponder { request in
            switch request.url?.path {
            case "/v1/ripcord/status":
                try await Task.sleep(for: .milliseconds(150))
                return .json(
                    """
                    {
                        "active": false,
                        "entry_count": 0
                    }
                    """
                )
            default:
                return .json("{}", statusCode: 404)
            }
        }

        let persistence = AppStatePersistence(
            defaultsSuiteName: defaultsSuiteName,
            keychainService: keychainService,
            localInstallLoader: { nil }
        )
        let sut = await MainActor.run {
            AppState(
                persistence: persistence,
                client: client,
                startLoadingPersistedState: false
            )
        }
        try await sut.savePairing(
            serverURLString: serverURL,
            token: "test-token",
            deviceName: "Desk Mac",
            connectionMode: .remote
        )

        async let firstRefresh: Void = sut.refreshRipcordState()
        async let secondRefresh: Void = sut.refreshRipcordState()
        await firstRefresh
        await secondRefresh

        let requests = await MockAppStateURLProtocol.recordedRequests()
        await MockAppStateURLProtocol.reset()

        XCTAssertEqual(
            requests.filter { $0.url?.path == "/v1/ripcord/status" }.count,
            1
        )
    }

    @MainActor
    func testDismissRipcordNotificationHidesCurrentActiveStatus() {
        let sut = AppState(startLoadingPersistedState: false)
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Tracked file mutation",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 3
        )

        sut.ripcordStatus = status
        XCTAssertEqual(sut.activeRipcordStatus, status)

        sut.dismissRipcordNotification()

        XCTAssertNil(sut.activeRipcordStatus)
    }

    @MainActor
    func testRipcordStatusWithoutJournaledActionsDoesNotSurfaceNotification() {
        let sut = AppState(startLoadingPersistedState: false)
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Writes outside project directory",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 0
        )

        sut.ripcordStatus = status

        XCTAssertNil(sut.activeRipcordStatus)
    }

    @MainActor
    func testRipcordNotificationAppearsWhenJournaledActionsBegin() {
        let sut = AppState(startLoadingPersistedState: false)
        let pendingStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Writes outside project directory",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 0
        )
        let activeStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Writes outside project directory",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 2
        )

        sut.ripcordStatus = pendingStatus
        XCTAssertNil(sut.activeRipcordStatus)

        sut.ripcordStatus = activeStatus

        XCTAssertEqual(sut.activeRipcordStatus, activeStatus)
    }

    func testLaunchAgentPlistWithoutHTTPFlagNeedsRepair() {
        let plist = """
        <plist version="1.0">
        <dict>
            <key>ProgramArguments</key>
            <array>
                <string>/Users/joseph/fawx/target/release/fawx</string>
                <string>serve</string>
                <string>--port</string>
                <string>8400</string>
            </array>
        </dict>
        </plist>
        """

        XCTAssertTrue(AppState.launchAgentNeedsRepair(plistContents: plist))
    }

    func testLaunchAgentPlistWithHTTPFlagDoesNotNeedRepair() {
        let plist = """
        <plist version="1.0">
        <dict>
            <key>ProgramArguments</key>
            <array>
                <string>/Users/joseph/fawx/target/release/fawx</string>
                <string>serve</string>
                <string>--http</string>
                <string>--port</string>
                <string>8400</string>
            </array>
        </dict>
        </plist>
        """

        XCTAssertFalse(AppState.launchAgentNeedsRepair(plistContents: plist))
    }

    @MainActor
    func testNewRipcordEventResetsDismissedNotification() {
        let sut = AppState(startLoadingPersistedState: false)
        let firstStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Tracked file mutation",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 3
        )
        let secondStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-2",
            tripwireDescription: "New policy trigger",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_100),
            entryCount: 1
        )

        sut.ripcordStatus = firstStatus
        sut.dismissRipcordNotification()
        XCTAssertNil(sut.activeRipcordStatus)

        sut.ripcordStatus = secondStatus

        XCTAssertEqual(sut.activeRipcordStatus, secondStatus)
    }

    @MainActor
    func testInactiveRipcordStatusClearsDismissedNotification() {
        let sut = AppState(startLoadingPersistedState: false)
        sut.ripcordStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Tracked file mutation",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_000),
            entryCount: 3
        )

        sut.dismissRipcordNotification()
        sut.ripcordStatus = .inactive

        XCTAssertNil(sut.activeRipcordStatus)

        sut.ripcordStatus = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-2",
            tripwireDescription: "New policy trigger",
            activatedAt: Date(timeIntervalSince1970: 1_710_000_100),
            entryCount: 1
        )

        XCTAssertNotNil(sut.activeRipcordStatus)
    }

    @MainActor
    func testGitPanelPresentationShowRestoresSidebarSelectionToActiveSession() {
        let context = makeGitPanelPresentationContext()
        let session = makeSession(id: "session-a", updatedAt: 10)
        let isShowingGitPanel = BoolBox(false)

        context.sessionViewModel.upsert(session)
        context.sessionViewModel.select(session.id)
        context.appState.sidebarSelection = .settings

        GitPanelPresentation.show(
            showGitPanel: gitPanelBinding(for: isShowingGitPanel),
            selectedSessionID: nil,
            appState: context.appState,
            sessionViewModel: context.sessionViewModel,
            chatViewModel: context.chatViewModel
        )

        XCTAssertTrue(isShowingGitPanel.value)
        XCTAssertEqual(context.appState.sidebarSelection, .session(session.id))
        XCTAssertEqual(context.sessionViewModel.selectedSessionID, session.id)
        XCTAssertTrue(context.chatViewModel.isLoadingHistory)
    }

    @MainActor
    func testGitPanelPresentationShowClearsSelectionWhenNoSessionIsAvailable() {
        let context = makeGitPanelPresentationContext()
        let isShowingGitPanel = BoolBox(false)

        context.appState.sidebarSelection = .skills
        context.sessionViewModel.select(nil)

        GitPanelPresentation.show(
            showGitPanel: gitPanelBinding(for: isShowingGitPanel),
            selectedSessionID: nil,
            appState: context.appState,
            sessionViewModel: context.sessionViewModel,
            chatViewModel: context.chatViewModel
        )

        XCTAssertTrue(isShowingGitPanel.value)
        XCTAssertNil(context.appState.sidebarSelection)
        XCTAssertNil(context.sessionViewModel.selectedSessionID)
        XCTAssertFalse(context.chatViewModel.isLoadingHistory)
        XCTAssertTrue(context.chatViewModel.transcriptItems.isEmpty)
    }

    @MainActor
    func testGitPanelPresentationToggleShowsThenHidesPanel() {
        let context = makeGitPanelPresentationContext()
        let session = makeSession(id: "session-a", updatedAt: 20)
        let isShowingGitPanel = BoolBox(false)

        context.sessionViewModel.upsert(session)
        context.sessionViewModel.select(session.id)

        GitPanelPresentation.toggle(
            showGitPanel: gitPanelBinding(for: isShowingGitPanel),
            selectedSessionID: session.id,
            appState: context.appState,
            sessionViewModel: context.sessionViewModel,
            chatViewModel: context.chatViewModel
        )
        XCTAssertTrue(isShowingGitPanel.value)

        GitPanelPresentation.toggle(
            showGitPanel: gitPanelBinding(for: isShowingGitPanel),
            selectedSessionID: session.id,
            appState: context.appState,
            sessionViewModel: context.sessionViewModel,
            chatViewModel: context.chatViewModel
        )
        XCTAssertFalse(isShowingGitPanel.value)
    }

    @MainActor
    func testGitPanelPresentationHideClearsBinding() {
        let isShowingGitPanel = BoolBox(true)

        GitPanelPresentation.hide(showGitPanel: gitPanelBinding(for: isShowingGitPanel))

        XCTAssertFalse(isShowingGitPanel.value)
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

    @MainActor
    private func makeGitPanelPresentationContext()
        -> (appState: AppState, sessionViewModel: SessionViewModel, chatViewModel: ChatViewModel)
    {
        let appState = AppState(startLoadingPersistedState: false)
        let sessionViewModel = SessionViewModel(appState: appState)
        let chatViewModel = ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
        return (appState, sessionViewModel, chatViewModel)
    }

    private func makeSession(id: String, updatedAt: Int) -> Session {
        Session(
            key: id,
            kind: .main,
            status: .idle,
            label: nil,
            title: "Session \(id)",
            preview: nil,
            model: "gpt-5.4",
            createdAt: updatedAt,
            updatedAt: updatedAt,
            messageCount: 0
        )
    }

    private func gitPanelBinding(for box: BoolBox) -> Binding<Bool> {
        Binding(
            get: { box.value },
            set: { box.value = $0 }
        )
    }
}

private final class BoolBox {
    var value: Bool

    init(_ value: Bool) {
        self.value = value
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

private final class OffMainExecutionProbe: @unchecked Sendable {
    private let lock = NSLock()
    private let startedExpectation: XCTestExpectation?
    private let releaseSemaphore: DispatchSemaphore?
    private var invocationCount = 0
    private var observedMainThread = false

    init(
        blocking: Bool = false,
        startedExpectation: XCTestExpectation? = nil
    ) {
        self.startedExpectation = startedExpectation
        self.releaseSemaphore = blocking ? DispatchSemaphore(value: 0) : nil
    }

    func record() {
        lock.lock()
        invocationCount += 1
        observedMainThread = observedMainThread || Thread.isMainThread
        lock.unlock()

        startedExpectation?.fulfill()
        releaseSemaphore?.wait()
    }

    func release() {
        releaseSemaphore?.signal()
    }

    func assertObservedOffMain(
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        lock.lock()
        let invocationCount = invocationCount
        let observedMainThread = observedMainThread
        lock.unlock()

        XCTAssertGreaterThan(invocationCount, 0, file: file, line: line)
        XCTAssertFalse(observedMainThread, file: file, line: line)
    }
}

private final class MockAppStateURLProtocol: URLProtocol, @unchecked Sendable {
    private static let store = MockAppStateURLProtocolStore()
    private var requestTask: Task<Void, Never>?

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        let request = self.request
        let client = client

        requestTask = Task {
            do {
                let (response, data) = try await Self.store.response(for: request)
                guard !Task.isCancelled else {
                    return
                }

                client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
                client?.urlProtocol(self, didLoad: data)
                client?.urlProtocolDidFinishLoading(self)
            } catch {
                guard !Task.isCancelled else {
                    return
                }

                client?.urlProtocol(self, didFailWithError: error)
            }
        }
    }

    override func stopLoading() {
        requestTask?.cancel()
        requestTask = nil
    }

    static func setResponder(_ responder: @escaping MockAppStateURLProtocolStore.Responder) async {
        await store.setResponder(responder)
    }

    static func recordedRequests() async -> [URLRequest] {
        await store.recordedRequests()
    }

    static func reset() async {
        await store.reset()
    }
}

private actor MockAppStateURLProtocolStore {
    typealias Responder = @Sendable (URLRequest) async throws -> MockAppStateResponse

    private var responder: Responder?
    private var requests: [URLRequest] = []

    func setResponder(_ responder: @escaping Responder) {
        self.responder = responder
        requests = []
    }

    func response(for request: URLRequest) async throws -> (HTTPURLResponse, Data) {
        requests.append(request)

        guard let responder else {
            throw MockAppStateProtocolError.missingResponder
        }

        let response = try await responder(request)
        guard let url = request.url else {
            throw MockAppStateProtocolError.missingURL
        }
        guard let httpResponse = HTTPURLResponse(
            url: url,
            statusCode: response.statusCode,
            httpVersion: nil,
            headerFields: ["Content-Type": "application/json"]
        ) else {
            throw MockAppStateProtocolError.invalidResponse
        }

        return (httpResponse, response.body)
    }

    func recordedRequests() -> [URLRequest] {
        requests
    }

    func reset() {
        responder = nil
        requests = []
    }
}

private struct MockAppStateResponse: Sendable {
    let statusCode: Int
    let body: Data

    init(statusCode: Int, body: Data = Data("{}".utf8)) {
        self.statusCode = statusCode
        self.body = body
    }

    static func json(_ body: String, statusCode: Int = 200) -> Self {
        Self(statusCode: statusCode, body: Data(body.utf8))
    }
}

private enum MockAppStateProtocolError: Error {
    case invalidResponse
    case missingResponder
    case missingURL
}
