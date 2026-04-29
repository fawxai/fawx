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
    await persistence.setAccentColor(AppAccentColor(red: 0.12, green: 0.34, blue: 0.56))
    await persistence.setConnectionMode(.local)
    await persistence.setSetupComplete(true)
    await persistence.setFavoriteModelIDs([
      "openai/gpt-5.4",
      "  ",
      "anthropic/claude-sonnet-4-6",
    ])

    XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.theme), AppTheme.dark.rawValue)
    XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.fontSize), AppFontSize.large.rawValue)
    XCTAssertEqual(defaults.string(forKey: AppStateStorageKey.accentColor), "#1F578F")
    XCTAssertEqual(
      defaults.string(forKey: AppStateStorageKey.connectionMode), AppConnectionMode.local.rawValue)
    XCTAssertTrue(defaults.bool(forKey: AppStateStorageKey.setupComplete))
    XCTAssertEqual(
      defaults.stringArray(forKey: AppStateStorageKey.favoriteModelIDs),
      ["anthropic/claude-sonnet-4-6", "openai/gpt-5.4"]
    )
  }

  func testLoadLaunchSnapshotReadsPersistedAccentColor() async {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
    defaults.set("#3366CC", forKey: AppStateStorageKey.accentColor)
    let persistence = AppStatePersistence(
      defaultsSuiteName: defaultsSuiteName,
      localInstallLoader: { nil }
    )

    let snapshot = await persistence.loadLaunchSnapshot(resetState: false)

    XCTAssertEqual(snapshot.accentColor.hexString, "#3366CC")
  }

  func testFawxAccentDisplayVariantPreservesRawSavedColorWhileAdaptingContrast() {
    let whiteAccent = AppAccentColor(red: 1, green: 1, blue: 1)
    let blackAccent = AppAccentColor(red: 0, green: 0, blue: 0)

    let whiteOnLight = whiteAccent.resolvedForFawxChrome(in: .light)
    let whiteOnDark = whiteAccent.resolvedForFawxChrome(in: .dark)
    let blackOnDark = blackAccent.resolvedForFawxChrome(in: .dark)
    let blackOnLight = blackAccent.resolvedForFawxChrome(in: .light)

    XCTAssertEqual(whiteAccent.hexString, "#FFFFFF")
    XCTAssertNotEqual(whiteOnLight.hexString, whiteAccent.hexString)
    XCTAssertEqual(whiteOnDark.hexString, whiteAccent.hexString)
    XCTAssertGreaterThanOrEqual(whiteOnLight.contrastRatio(against: FawxAccentContrastScheme.light.backgroundLuminance), 3)

    XCTAssertEqual(blackAccent.hexString, "#000000")
    XCTAssertNotEqual(blackOnDark.hexString, blackAccent.hexString)
    XCTAssertEqual(blackOnLight.hexString, blackAccent.hexString)
    XCTAssertGreaterThanOrEqual(blackOnDark.contrastRatio(against: FawxAccentContrastScheme.dark.backgroundLuminance), 3)
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
    defaults.set(
      ["openai/gpt-5.4", " ", "anthropic/claude-sonnet-4-6"],
      forKey: AppStateStorageKey.favoriteModelIDs
    )
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
      XCTAssertEqual(sut.favoriteModelIDs, ["anthropic/claude-sonnet-4-6", "openai/gpt-5.4"])
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

    func testAwaitPersistedStateLoadPrefersDetectedLocalInstallOverStaleSavedLocalURL() async throws
    {
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
      try KeychainHelper.saveToken(
        "stale-token", forServer: staleServerURL, service: keychainService)
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
    defaults.set(["openai/gpt-5.4"], forKey: AppStateStorageKey.favoriteModelIDs)
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
    XCTAssertEqual(snapshot.favoriteModelIDs, [])
    XCTAssertNil(snapshot.authToken)
    XCTAssertNil(snapshot.localInstallConfiguration)
    XCTAssertNil(defaults.string(forKey: AppStateStorageKey.serverURL))
    XCTAssertNil(defaults.string(forKey: AppStateStorageKey.pairedDeviceName))
    XCTAssertNil(defaults.string(forKey: AppStateStorageKey.theme))
    XCTAssertNil(defaults.string(forKey: AppStateStorageKey.fontSize))
    XCTAssertFalse(defaults.bool(forKey: AppStateStorageKey.setupComplete))
    XCTAssertNil(defaults.string(forKey: AppStateStorageKey.connectionMode))
    XCTAssertNil(defaults.stringArray(forKey: AppStateStorageKey.favoriteModelIDs))
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

  func testAwaitPersistedStateLoadDoesNotOverwriteStartupFavoriteModelChanges() async {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
    let loader = BlockingLocalInstallLoader()

    defaults.set(["anthropic/claude-sonnet-4-6"], forKey: AppStateStorageKey.favoriteModelIDs)

    let persistence = AppStatePersistence(
      defaultsSuiteName: defaultsSuiteName,
      localInstallLoader: { await loader.load() }
    )
    let sut = await MainActor.run {
      AppState(persistence: persistence)
    }

    await loader.waitUntilStarted()

    await MainActor.run {
      sut.toggleFavoriteModel("openai/gpt-5.4")
    }

    await loader.release()
    await sut.awaitPersistedStateLoad()

    await MainActor.run {
      XCTAssertEqual(sut.favoriteModelIDs, ["openai/gpt-5.4"])
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

  func testResolveLaunchConnectionModeUsesEffectiveServerURLWhenStoredModeMissing() async {
    #if os(macOS)
      let connectionMode = await MainActor.run {
        AppState.resolveLaunchConnectionMode(
          storedConnectionModeRawValue: nil,
          storedServerURL: "",
          effectiveServerURLString: "http://127.0.0.1:8418"
        )
      }

      XCTAssertEqual(connectionMode, .remote)
      await MainActor.run {
        XCTAssertEqual(
          AppState.resolveInitialDestination(
            isConfigured: true,
            setupComplete: false,
            connectionMode: connectionMode,
            hasStoredServerURL: false,
            hasLocalInstall: false
          ),
          .main
        )
      }
    #else
      let connectionMode = await MainActor.run {
        AppState.resolveLaunchConnectionMode(
          storedConnectionModeRawValue: nil,
          storedServerURL: "",
          effectiveServerURLString: "http://127.0.0.1:8418"
        )
      }

      XCTAssertEqual(connectionMode, .remote)
    #endif
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

    MockAppStateURLProtocol.setResponder { request in
      switch request.url?.path {
      case "/v1/models":
        Thread.sleep(forTimeInterval: 0.15)
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

    let requests = MockAppStateURLProtocol.recordedRequests()
    MockAppStateURLProtocol.reset()

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

    MockAppStateURLProtocol.setResponder { request in
      switch request.url?.path {
      case "/v1/ripcord/status":
        Thread.sleep(forTimeInterval: 0.15)
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

    let requests = MockAppStateURLProtocol.recordedRequests()
    MockAppStateURLProtocol.reset()

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
              <string>/Users/fawx/fawx/target/release/fawx</string>
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
              <string>/Users/fawx/fawx/target/release/fawx</string>
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
    XCTAssertEqual(context.appState.sidebarSelection, .thread(.activeSessionID(session.id)))
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

  @MainActor
  func testSessionViewModelRefreshMigratesStoredSessionSelectionToThreadSelection() async throws {
    let context = try await makeSessionViewModelContext(
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "ws-general",
              "name": "General",
              "path": "/Users/fawx/.fawx/general",
              "kind": "general",
              "repo": null,
              "last_opened_at": 1710000000
            },
            {
              "id": "ws-repo",
              "name": "Repository",
              "path": "/Users/fawx/fawx",
              "kind": "repository",
              "repo": {
                "root": "/Users/fawx/fawx",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": """
        {
          "sessions": [
            {
              "key": "session-1",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "Fix thread state",
              "preview": "Latest preview",
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000100,
              "message_count": 3
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-general/threads": #"{"threads":[],"total":0}"#,
        "/v1/workspaces/ws-general/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo/threads": """
        {
          "threads": [
            {
              "id": "thread-1",
              "title": "Fix thread state",
              "kind": "coding",
              "workspace_id": "ws-repo",
              "worktree_id": null,
              "active_session_id": "session-1",
              "status": "active",
              "preview": "Latest preview",
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.appState.sidebarSelection = SidebarSelection(rawValue: "session:session-1")

    await context.sessionViewModel.refresh()

    XCTAssertEqual(context.sessionViewModel.selectedWorkspaceID, "ws-repo")
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-1")
    XCTAssertEqual(context.sessionViewModel.selectedSessionID, "session-1")
    XCTAssertEqual(context.appState.sidebarSelection, .thread(.threadID("thread-1")))

    let requests = MockAppStateURLProtocol.recordedRequests()
    XCTAssertTrue(requests.contains { $0.url?.path == "/v1/workspaces/ws-repo/threads" })
    XCTAssertTrue(requests.contains { $0.url?.path == "/v1/workspaces/ws-repo/worktrees" })
  }

  @MainActor
  func testSessionViewModelRefreshFallsBackToDefaultThreadWhenStoredSessionSelectionIsMissing()
    async throws
  {
    let context = try await makeSessionViewModelContext(
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "ws-empty",
              "name": "Empty",
              "path": "/tmp/empty",
              "kind": "general",
              "repo": null,
              "last_opened_at": 1710000000
            },
            {
              "id": "ws-repo",
              "name": "Repository",
              "path": "/Users/fawx/fawx",
              "kind": "repository",
              "repo": {
                "root": "/Users/fawx/fawx",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": #"{"sessions":[],"total":0}"#,
        "/v1/workspaces/ws-empty/threads": #"{"threads":[],"total":0}"#,
        "/v1/workspaces/ws-empty/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo/threads": """
        {
          "threads": [
            {
              "id": "thread-fallback",
              "title": "Fallback thread",
              "kind": "coding",
              "workspace_id": "ws-repo",
              "worktree_id": null,
              "active_session_id": "session-fallback",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.appState.sidebarSelection = SidebarSelection(rawValue: "session:missing-session")

    await context.sessionViewModel.refresh()

    XCTAssertEqual(context.sessionViewModel.selectedWorkspaceID, "ws-repo")
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-fallback")
    XCTAssertEqual(context.sessionViewModel.selectedSessionID, "session-fallback")
    XCTAssertEqual(context.appState.sidebarSelection, .thread(.threadID("thread-fallback")))
  }

  @MainActor
  func testSessionViewModelRemembersThreadSelectionPerWorkspaceAcrossReloads() {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let defaults = makeUserDefaults(suiteName: defaultsSuiteName)
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState, userDefaults: defaults)
    let workspaces = [
      makeWorkspace(id: "ws-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a"),
      makeWorkspace(id: "ws-b", name: "Repo B", kind: .repository, path: "/tmp/repo-b"),
    ]
    let threadsByWorkspaceID = [
      "ws-a": [
        makeThread(
          id: "thread-a1", title: "A1", workspaceID: "ws-a", activeSessionID: "session-a1",
          updatedAt: 30),
        makeThread(
          id: "thread-a2", title: "A2", workspaceID: "ws-a", activeSessionID: "session-a2",
          updatedAt: 20),
      ],
      "ws-b": [
        makeThread(
          id: "thread-b1", title: "B1", workspaceID: "ws-b", activeSessionID: "session-b1",
          updatedAt: 30),
        makeThread(
          id: "thread-b2", title: "B2", workspaceID: "ws-b", activeSessionID: "session-b2",
          updatedAt: 20),
      ],
    ]

    sessionViewModel.setSidebarDataForTesting(
      workspaces: workspaces,
      threadsByWorkspaceID: threadsByWorkspaceID,
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-a"
    )

    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-a")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-a1")

    sessionViewModel.select("session-b2")
    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-b")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-b2")

    sessionViewModel.select("session-a2")
    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-a")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-a2")

    sessionViewModel.selectWorkspace("ws-b")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-b2")

    sessionViewModel.selectWorkspace("ws-a")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-a2")

    let reloaded = SessionViewModel(
      appState: AppState(startLoadingPersistedState: false),
      userDefaults: defaults
    )
    reloaded.setSidebarDataForTesting(
      workspaces: workspaces,
      threadsByWorkspaceID: threadsByWorkspaceID,
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-b"
    )

    XCTAssertEqual(reloaded.selectedWorkspaceID, "ws-b")
    XCTAssertEqual(reloaded.selectedThreadID, "thread-b2")

    reloaded.selectWorkspace("ws-a")
    XCTAssertEqual(reloaded.selectedThreadID, "thread-a2")
  }

  @MainActor
  func
    testSessionViewModelSelectedSessionUsesThreadCompatibilityAdapterWhenSessionListHasNotLoadedIt()
    async throws
  {
    let context = try await makeSessionViewModelContext(
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "ws-repo",
              "name": "Repository",
              "path": "/Users/fawx/fawx",
              "kind": "repository",
              "repo": {
                "root": "/Users/fawx/fawx",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 1
        }
        """,
        "/v1/sessions": #"{"sessions":[],"total":0}"#,
        "/v1/workspaces/ws-repo/threads": """
        {
          "threads": [
            {
              "id": "thread-compat",
              "title": "Compatibility thread",
              "kind": "coding",
              "workspace_id": "ws-repo",
              "worktree_id": null,
              "active_session_id": "session-compat",
              "status": "active",
              "preview": "Bridged through active session id",
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.appState.sidebarSelection = .workspace("ws-repo")

    await context.sessionViewModel.refresh()

    let selectedSession = try XCTUnwrap(context.sessionViewModel.selectedSession)
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-compat")
    XCTAssertEqual(context.sessionViewModel.selectedSessionID, "session-compat")
    XCTAssertEqual(selectedSession.id, "session-compat")
    XCTAssertEqual(selectedSession.title, "Compatibility thread")
    XCTAssertEqual(selectedSession.preview, "Bridged through active session id")
  }

  @MainActor
  func testSessionViewModelWorkspaceSelectionRemainsDistinctFromSelectedThread() {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-a", name: "Workspace A", kind: .general, path: "/tmp/ws-a"),
        makeWorkspace(id: "ws-b", name: "Workspace B", kind: .repository, path: "/tmp/ws-b"),
      ],
      threadsByWorkspaceID: [
        "ws-a": [
          makeThread(
            id: "thread-a1", title: "A1", workspaceID: "ws-a", activeSessionID: "session-a1",
            updatedAt: 20)
        ],
        "ws-b": [
          makeThread(
            id: "thread-b1", title: "B1", workspaceID: "ws-b", activeSessionID: "session-b1",
            updatedAt: 10)
        ],
      ],
      worktreesByWorkspaceID: [:],
      selectedThreadID: "thread-b1"
    )

    sessionViewModel.selectWorkspace("ws-a")

    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-a")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-a1")
    XCTAssertEqual(appState.sidebarSelection, .workspace("ws-a"))
  }

  @MainActor
  func testSessionViewModelWorkspaceThreadGroupsShowEmptyWorkspaceStartRowAndTrackExpansionState() {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-empty", name: "Empty", kind: .general, path: "/tmp/empty"),
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo"),
      ],
      threadsByWorkspaceID: [
        "ws-empty": [],
        "ws-repo": [
          makeThread(
            id: "thread-repo", title: "Repo thread", workspaceID: "ws-repo",
            activeSessionID: "session-repo", updatedAt: 30)
        ],
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-empty",
      expandedWorkspaceIDs: ["ws-empty"]
    )

    let groups = sessionViewModel.workspaceThreadGroups
    guard let emptyGroup = groups.first(where: { $0.workspace.id == "ws-empty" }) else {
      XCTFail("Expected empty workspace group.")
      return
    }
    guard let repoGroup = groups.first(where: { $0.workspace.id == "ws-repo" }) else {
      XCTFail("Expected repository workspace group.")
      return
    }

    XCTAssertEqual(groups.map(\.id), ["ws-empty", "ws-repo"])
    XCTAssertEqual(emptyGroup.workspace.id, "ws-empty")
    XCTAssertTrue(emptyGroup.isExpanded)
    XCTAssertTrue(emptyGroup.showsStartThreadRow)
    XCTAssertFalse(repoGroup.isExpanded)

    sessionViewModel.expandAllWorkspaces()
    XCTAssertTrue(sessionViewModel.areAllWorkspacesExpanded)
    XCTAssertTrue(sessionViewModel.isWorkspaceExpanded("ws-empty"))
    XCTAssertTrue(sessionViewModel.isWorkspaceExpanded("ws-repo"))

    sessionViewModel.collapseAllWorkspaces()
    XCTAssertFalse(sessionViewModel.isWorkspaceExpanded("ws-empty"))
    XCTAssertFalse(sessionViewModel.isWorkspaceExpanded("ws-repo"))
  }

  @MainActor
  func testSessionViewModelOpenWorkspaceAppendsBackendWorkspaceWithoutReintroducingGeneralShellRow()
    async throws
  {
    let context = try await makeSessionViewModelContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/workspaces/open"):
        return .json(
          """
          {
            "id": "ws-repo-b",
            "name": "Repo B",
            "path": "/tmp/repo-b",
            "kind": "repository",
            "repo": null,
            "last_opened_at": 1710000300
          }
          """
        )
      default:
        return .json("{}", statusCode: 404)
      }
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a")
      ],
      threadsByWorkspaceID: [
        "ws-repo-a": [
          makeThread(
            id: "thread-a1", title: "A1", workspaceID: "ws-repo-a", activeSessionID: "session-a1",
            updatedAt: 30)
        ]
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo-a"
    )

    let addedWorkspace = await context.sessionViewModel.openWorkspace(path: "/tmp/repo-b")

    XCTAssertEqual(addedWorkspace?.id, "ws-repo-b")
    XCTAssertFalse(context.sessionViewModel.workspaces.contains(where: \.isGeneral))
    XCTAssertEqual(context.sessionViewModel.workspaces.map(\.id), ["ws-repo-a", "ws-repo-b"])
    XCTAssertEqual(context.sessionViewModel.selectedWorkspaceID, "ws-repo-b")
    XCTAssertTrue(context.sessionViewModel.isWorkspaceExpanded("ws-repo-b"))
  }

  @MainActor
  func testSessionViewModelCreateNewThreadUsesTypedThreadRouteAndPreservesWorkspaceContext()
    async throws
  {
    let context = try await makeSessionViewModelContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/threads"):
        return .json(
          """
          {
            "id": "thread-created",
            "title": "Created thread",
            "kind": "coding",
            "workspace_id": "ws-repo",
            "worktree_id": null,
            "active_session_id": "session-created",
            "status": "idle",
            "preview": null,
            "model": "gpt-5.4",
            "created_at": 1710000400,
            "updated_at": 1710000400
          }
          """
        )
      default:
        return .json("{}", statusCode: 404)
      }
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [:],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo"
    )

    let createdSessionID = await context.sessionViewModel.createNewThread(in: "ws-repo")

    XCTAssertEqual(createdSessionID, "session-created")
    XCTAssertEqual(context.sessionViewModel.selectedWorkspaceID, "ws-repo")
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-created")
    XCTAssertEqual(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo"]?.map(\.id),
      ["thread-created"]
    )
  }

  @MainActor
  func testSessionViewModelArchiveThreadsArchivesEveryVisibleThreadInWorkspace() async throws {
    let archivedSessionA = makeSessionPayload(
      id: "session-a",
      title: "Thread A",
      updatedAt: 40,
      archived: true,
      archivedAt: 45
    )
    let archivedSessionB = makeSessionPayload(
      id: "session-b",
      title: "Thread B",
      updatedAt: 30,
      archived: true,
      archivedAt: 46
    )
    let context = try await makeSessionViewModelContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/archive"):
        return .json(archivedSessionA)
      case ("POST", "/v1/sessions/session-b/archive"):
        return .json(archivedSessionB)
      default:
        return .json("{}", statusCode: 404)
      }
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [
          makeThread(
            id: "thread-a", title: "Thread A", workspaceID: "ws-repo", activeSessionID: "session-a",
            updatedAt: 40),
          makeThread(
            id: "thread-b", title: "Thread B", workspaceID: "ws-repo", activeSessionID: "session-b",
            updatedAt: 30),
        ]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(id: "session-a", title: "Thread A", updatedAt: 40),
        makeDetailedSession(id: "session-b", title: "Thread B", updatedAt: 30),
      ],
      selectedThreadID: "thread-a"
    )

    let archivedCount = await context.sessionViewModel.archiveThreads(in: "ws-repo")

    XCTAssertEqual(archivedCount, 2)
    XCTAssertEqual(context.sessionViewModel.threadsByWorkspaceID["ws-repo"], [])
    let requests = MockAppStateURLProtocol.recordedRequests()
    MockAppStateURLProtocol.reset()
    XCTAssertEqual(
      requests.compactMap(\.url?.path).sorted(),
      ["/v1/sessions/session-a/archive", "/v1/sessions/session-b/archive"]
    )
  }

  @MainActor
  func testSessionViewModelCreateWorktreeThreadSurfacesTypedWorktreeMetadata() async throws {
    let context = try await makeSessionViewModelContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/worktrees"):
        return .json(
          """
          {
            "id": "wt-new",
            "workspace_id": "ws-repo",
            "label": "feature/thread-lifecycle",
            "path": "/tmp/repo-feature-thread-lifecycle",
            "branch": "feature/thread-lifecycle",
            "base_ref": "origin/dev",
            "status": "available",
            "clean": true,
            "ahead_count": 0,
            "behind_count": 0
          }
          """
        )
      case ("POST", "/v1/threads"):
        return .json(
          """
          {
            "id": "thread-worktree",
            "title": "Worktree lane",
            "kind": "coding",
            "workspace_id": "ws-repo",
            "worktree_id": "wt-new",
            "active_session_id": "session-worktree",
            "status": "idle",
            "preview": null,
            "model": "gpt-5.4",
            "created_at": 1710000500,
            "updated_at": 1710000500
          }
          """
        )
      default:
        return .json("{}", statusCode: 404)
      }
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [:],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo"
    )

    let created = await context.sessionViewModel.createWorktreeThread(
      in: "ws-repo",
      title: "Worktree lane",
      branch: "feature/thread-lifecycle",
      baseRef: "origin/dev"
    )

    XCTAssertEqual(created?.thread.id, "thread-worktree")
    XCTAssertEqual(created?.worktree.id, "wt-new")
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-worktree")
    XCTAssertEqual(
      context.sessionViewModel.worktreesByWorkspaceID["ws-repo"]?.map(\.id),
      ["wt-new"]
    )
    let thread = try XCTUnwrap(context.sessionViewModel.thread("thread-worktree"))
    XCTAssertEqual(thread.worktreeID, "wt-new")
    XCTAssertEqual(
      context.sessionViewModel.threadContextLabel(thread, includeWorkspace: true),
      "Repo · feature/thread-lifecycle · Clean"
    )
  }

  @MainActor
  func testSessionViewModelActivateWorkspaceRowCollapsesExpandedWorkspaceFromRowTap() {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [
          makeThread(
            id: "thread-1", title: "Thread", workspaceID: "ws-repo", activeSessionID: "session-1",
            updatedAt: 20)
        ]
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo",
      expandedWorkspaceIDs: ["ws-repo"]
    )

    let shouldSwitchContext = sessionViewModel.activateWorkspaceRow("ws-repo")

    XCTAssertFalse(shouldSwitchContext)
    XCTAssertFalse(sessionViewModel.isWorkspaceExpanded("ws-repo"))
    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-repo")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-1")
  }

  @MainActor
  func testSessionViewModelMoveWorkspacesAndThreadsUpdatesVisibleOrder() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState, userDefaults: defaults)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a"),
        makeWorkspace(id: "ws-b", name: "Repo B", kind: .repository, path: "/tmp/repo-b"),
      ],
      threadsByWorkspaceID: [
        "ws-a": [
          makeThread(
            id: "thread-a1", title: "A1", workspaceID: "ws-a", activeSessionID: "session-a1",
            updatedAt: 30),
          makeThread(
            id: "thread-a2", title: "A2", workspaceID: "ws-a", activeSessionID: "session-a2",
            updatedAt: 20),
        ],
        "ws-b": [
          makeThread(
            id: "thread-b1", title: "B1", workspaceID: "ws-b", activeSessionID: "session-b1",
            updatedAt: 10)
        ],
      ],
      worktreesByWorkspaceID: [:]
    )

    sessionViewModel.moveWorkspaces(fromOffsets: IndexSet(integer: 1), toOffset: 0)
    sessionViewModel.moveThreads(in: "ws-a", fromOffsets: IndexSet(integer: 1), toOffset: 0)

    XCTAssertEqual(sessionViewModel.workspaces.map(\.id), ["ws-b", "ws-a"])
    XCTAssertEqual(
      sessionViewModel.workspaceThreadGroups.first(where: { $0.workspace.id == "ws-a" })?.threads
        .map(\.id),
      ["thread-a2", "thread-a1"]
    )
  }

  @MainActor
  func testWorkspaceShellStateKeepsRemovedWorkspaceHiddenAcrossRefresh() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)
    let workspaceA = makeWorkspace(
      id: "ws-a",
      name: "Repo A",
      kind: .repository,
      path: "/tmp/repo-a"
    )
    let workspaceB = makeWorkspace(
      id: "ws-b",
      name: "Repo B",
      kind: .repository,
      path: "/tmp/repo-b"
    )

    XCTAssertEqual(
      shellState.visibleWorkspaces(merging: [workspaceA, workspaceB]).map(\.id),
      ["ws-a", "ws-b"]
    )

    shellState.removeWorkspace(workspaceA)

    XCTAssertEqual(
      shellState.visibleWorkspaces(merging: [workspaceA, workspaceB]).map(\.id),
      ["ws-b"]
    )
    XCTAssertTrue(shellState.isWorkspaceHidden(id: "ws-a"))
  }

  @MainActor
  func testWorkspaceShellStateSanitizeManualThreadOrderRemovesUnknownThreadsAndWorkspaces() throws {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    let seededOrder = [
      "ws-a": ["thread-1", "stale-thread", "thread-2"],
      "ws-stale": ["thread-3"],
    ]
    defaults.set(
      try JSONEncoder().encode(seededOrder),
      forKey: "workspace_shell_manual_thread_order"
    )
    var shellState = WorkspaceShellState(userDefaults: defaults)

    shellState.sanitizeManualThreadOrder(using: [
      "ws-a": [
        makeThread(
          id: "thread-1", title: "Thread 1", workspaceID: "ws-a", activeSessionID: "session-1",
          updatedAt: 20),
        makeThread(
          id: "thread-2", title: "Thread 2", workspaceID: "ws-a", activeSessionID: "session-2",
          updatedAt: 10),
      ]
    ])

    XCTAssertEqual(shellState.manualThreadOrderByWorkspaceID, ["ws-a": ["thread-1", "thread-2"]])
  }

  @MainActor
  func testWorkspaceShellStateOrderedThreadsHonorsManualOrderAndFallsBackForUnorderedThreads() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)

    shellState.moveThreads(
      ["thread-1", "thread-2"],
      in: "ws-a",
      fromOffsets: IndexSet(integer: 1),
      toOffset: 0
    )

    let ordered = shellState.orderedThreads(
      [
        makeThread(
          id: "thread-1", title: "Thread 1", workspaceID: "ws-a", activeSessionID: "session-1",
          updatedAt: 20),
        makeThread(
          id: "thread-2", title: "Thread 2", workspaceID: "ws-a", activeSessionID: "session-2",
          updatedAt: 10),
        makeThread(
          id: "thread-3", title: "Thread 3", workspaceID: "ws-a", activeSessionID: "session-3",
          updatedAt: 30),
      ],
      in: "ws-a",
      fallbackSort: { $0 < $1 }
    )

    XCTAssertEqual(ordered.map(\.id), ["thread-2", "thread-1", "thread-3"])
  }

  @MainActor
  func testWorkspaceShellStateForgetDeletedSessionClearsPersistedOwnershipTitleAndOrder() throws {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    let deletedSessionID = "session-delete"
    let deletedThreadID = stableEntityID(prefix: "thread", value: deletedSessionID)
    defaults.set(
      try JSONEncoder().encode(["ws-a": [deletedThreadID, "thread-keep"]]),
      forKey: "workspace_shell_manual_thread_order"
    )
    var shellState = WorkspaceShellState(userDefaults: defaults)
    shellState.rememberWorkspaceOwner("ws-a", for: deletedSessionID)
    shellState.renameThread(sessionID: deletedSessionID, title: "Delete me")

    shellState.forgetDeletedSession(deletedSessionID)

    XCTAssertNil(shellState.workspaceOwner(for: deletedSessionID))
    XCTAssertNil(shellState.customThreadTitle(for: deletedSessionID))
    XCTAssertEqual(shellState.manualThreadOrderByWorkspaceID, ["ws-a": ["thread-keep"]])
  }

  @MainActor
  func testWorkspaceShellStatePersistsRoundTripAcrossDefaults() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)
    let visibleWorkspace = shellState.addWorkspace(path: "/tmp/repo-a")
    let hiddenWorkspace = makeWorkspace(
      id: "ws-hidden",
      name: "Hidden Repo",
      kind: .repository,
      path: "/tmp/repo-hidden"
    )
    let firstThreadID = stableEntityID(prefix: "thread", value: "session-1")
    let secondThreadID = stableEntityID(prefix: "thread", value: "session-2")

    shellState.rememberWorkspaceOwner(visibleWorkspace.id, for: "session-1")
    shellState.renameThread(sessionID: "session-1", title: "Pinned thread")
    shellState.moveThreads(
      [firstThreadID, secondThreadID],
      in: visibleWorkspace.id,
      fromOffsets: IndexSet(integer: 1),
      toOffset: 0
    )
    shellState.removeWorkspace(hiddenWorkspace)

    let roundTrip = WorkspaceShellState(userDefaults: defaults)

    XCTAssertEqual(roundTrip.pinnedWorkspaces, [visibleWorkspace])
    XCTAssertEqual(roundTrip.workspaceOwner(for: "session-1"), visibleWorkspace.id)
    XCTAssertEqual(roundTrip.customThreadTitle(for: "session-1"), "Pinned thread")
    XCTAssertEqual(
      roundTrip.manualThreadOrderByWorkspaceID[visibleWorkspace.id],
      [secondThreadID, firstThreadID])
    XCTAssertTrue(roundTrip.isWorkspaceHidden(id: hiddenWorkspace.id))
  }

  @MainActor
  func
    testWorkspaceShellStateVisibleWorkspacesKeepRepositoryRowsAndSuppressBackendGeneralWorkspace()
  {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)
    let localWorkspace = shellState.addWorkspace(path: "/tmp/repo-a")
    let generalWorkspace = makeWorkspace(
      id: "ws-general", name: "General", kind: .general, path: "")
    let updatedWorkspace = WorkspaceSummary(
      id: localWorkspace.id,
      name: "Repo A Updated",
      path: localWorkspace.path,
      kind: .repository,
      repo: RepositorySummary(
        root: localWorkspace.path,
        vcs: "git",
        currentBranch: "main",
        defaultBranch: "main",
        origin: "git@example.com/repo-a.git",
        clean: true
      ),
      lastOpenedAt: 1_710_000_100
    )
    let workspaceB = makeWorkspace(
      id: "ws-b",
      name: "Repo B",
      kind: .repository,
      path: "/tmp/repo-b"
    )

    XCTAssertEqual(
      shellState.visibleWorkspaces(merging: [
        generalWorkspace, updatedWorkspace, workspaceB, updatedWorkspace,
      ]),
      [updatedWorkspace, workspaceB]
    )

    XCTAssertEqual(
      shellState.syncVisibleWorkspaces(with: [generalWorkspace, updatedWorkspace, workspaceB]).map(
        \.id),
      [updatedWorkspace.id, workspaceB.id]
    )
    XCTAssertTrue(shellState.isWorkspaceSuppressedFromShell(id: generalWorkspace.id))
  }

  @MainActor
  func testWorkspaceShellStateRemovesDuplicatePersistedWorkspaces() throws {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    let workspaceA = makeWorkspace(
      id: "ws-a",
      name: "Repo A",
      kind: .repository,
      path: "/tmp/repo-a"
    )
    let duplicateWorkspaceA = makeWorkspace(
      id: "ws-a-duplicate",
      name: "Repo A Duplicate",
      kind: .repository,
      path: "/tmp/repo-a"
    )
    let workspaceB = makeWorkspace(
      id: "ws-b",
      name: "Repo B",
      kind: .repository,
      path: "/tmp/repo-b"
    )
    defaults.set(
      try JSONEncoder().encode([workspaceA, duplicateWorkspaceA, workspaceB]),
      forKey: "workspace_shell_pinned_workspaces"
    )

    let shellState = WorkspaceShellState(userDefaults: defaults)

    XCTAssertEqual(shellState.pinnedWorkspaces.map(\.id), ["ws-a", "ws-b"])
    XCTAssertEqual(shellState.visibleWorkspaces(merging: []).map(\.id), ["ws-a", "ws-b"])
  }

  @MainActor
  func testWorkspaceShellStateNeverPromotesGeneralWorkspaceIntoVisibleShellRows() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)
    let generalWorkspace = makeWorkspace(
      id: "ws-general", name: "General", kind: .general, path: "")
    let repositoryWorkspace = makeWorkspace(
      id: "ws-repo",
      name: "Repo",
      kind: .repository,
      path: "/tmp/repo"
    )

    XCTAssertEqual(
      shellState.visibleWorkspaces(merging: [generalWorkspace, repositoryWorkspace]).map(\.id),
      ["ws-repo"]
    )

    shellState.removeWorkspace(generalWorkspace)

    XCTAssertEqual(
      shellState.visibleWorkspaces(merging: [generalWorkspace, repositoryWorkspace]).map(\.id),
      ["ws-repo"]
    )
    XCTAssertFalse(shellState.isWorkspaceHidden(id: "ws-general"))
  }

  @MainActor
  func
    testSessionViewModelRefreshSuppressesGeneralOwnedThreadsInsteadOfReassigningThemToRepositoryRows()
    async throws
  {
    let context = try await makeSessionViewModelContext(
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "workspace-general",
              "name": "General",
              "path": "",
              "kind": "general",
              "repo": null,
              "last_opened_at": 0
            },
            {
              "id": "ws-repo",
              "name": "Repo",
              "path": "/tmp/repo",
              "kind": "repository",
              "repo": {
                "root": "/tmp/repo",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": """
        {
          "sessions": [
            {
              "key": "session-general",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "General thread",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200,
              "message_count": 1
            },
            {
              "key": "session-repo",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "Repo thread",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300,
              "message_count": 1
            }
          ],
          "total": 2
        }
        """,
        "/v1/workspaces/workspace-general/threads": """
        {
          "threads": [
            {
              "id": "thread-general",
              "title": "General thread",
              "kind": "general",
              "workspace_id": "workspace-general",
              "worktree_id": null,
              "active_session_id": "session-general",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/workspace-general/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo/threads": """
        {
          "threads": [
            {
              "id": "thread-repo",
              "title": "Repo thread",
              "kind": "coding",
              "workspace_id": "ws-repo",
              "worktree_id": null,
              "active_session_id": "session-repo",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "workspace-general", name: "General", kind: .general, path: ""),
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo"),
      ],
      threadsByWorkspaceID: [
        "workspace-general": [
          ThreadSummary(
            id: "thread-general",
            title: "General thread",
            kind: .general,
            workspaceID: "workspace-general",
            worktreeID: nil,
            activeSessionID: "session-general",
            status: .idle,
            preview: nil,
            model: "gpt-5.4",
            createdAt: 1_710_000_000,
            updatedAt: 1_710_000_200
          )
        ],
        "ws-repo": [
          makeThread(
            id: "thread-repo",
            title: "Repo thread",
            workspaceID: "ws-repo",
            activeSessionID: "session-repo",
            updatedAt: 1_710_000_300
          )
        ],
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo"
    )

    await context.sessionViewModel.refresh()

    XCTAssertFalse(context.sessionViewModel.workspaces.contains(where: \.isGeneral))
    XCTAssertEqual(context.sessionViewModel.workspaces.map(\.id), ["ws-repo"])
    XCTAssertEqual(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo"]?.map(\.id),
      ["thread-repo"]
    )
    XCTAssertFalse(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo"]?.contains(where: {
        $0.activeSessionID == "session-general"
      }) == true
    )
    XCTAssertNil(context.sessionViewModel.thread("thread-general"))
  }

  @MainActor
  func
    testSessionViewModelRefreshRewritesStoredGeneralWorkspaceSelectionToVisibleRepositoryWorkspace()
    async throws
  {
    let context = try await makeSessionViewModelContext(
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "workspace-general",
              "name": "General",
              "path": "",
              "kind": "general",
              "repo": null,
              "last_opened_at": 0
            },
            {
              "id": "ws-repo",
              "name": "Repo",
              "path": "/tmp/repo",
              "kind": "repository",
              "repo": {
                "root": "/tmp/repo",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": #"{"sessions":[],"total":0}"#,
        "/v1/workspaces/workspace-general/threads": #"{"threads":[],"total":0}"#,
        "/v1/workspaces/workspace-general/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo/threads": #"{"threads":[],"total":0}"#,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.appState.sidebarSelection = .workspace("workspace-general")

    await context.sessionViewModel.refresh()

    XCTAssertFalse(context.sessionViewModel.workspaces.contains(where: \.isGeneral))
    XCTAssertEqual(context.sessionViewModel.selectedWorkspaceID, "ws-repo")
    XCTAssertEqual(context.appState.sidebarSelection, .workspace("ws-repo"))
  }

  @MainActor
  func testWorkspaceShellStateAddRemoveAndRevealWorkspaceLifecycle() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    var shellState = WorkspaceShellState(userDefaults: defaults)
    let workspace = shellState.addWorkspace(path: "/tmp/repo-reveal")

    shellState.removeWorkspace(workspace)
    XCTAssertTrue(shellState.isWorkspaceHidden(id: workspace.id))
    XCTAssertEqual(shellState.visibleWorkspaces(merging: [workspace]), [])

    shellState.revealWorkspace(workspace)

    XCTAssertFalse(shellState.isWorkspaceHidden(id: workspace.id))
    XCTAssertEqual(shellState.visibleWorkspaces(merging: [workspace]), [workspace])
  }

  @MainActor
  func testSessionViewModelRemoveWorkspaceRestoresSelectionToRemainingWorkspace() {
    let defaults = makeUserDefaults(suiteName: uniqueDefaultsSuiteName())
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState, userDefaults: defaults)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a"),
        makeWorkspace(id: "ws-b", name: "Repo B", kind: .repository, path: "/tmp/repo-b"),
      ],
      threadsByWorkspaceID: [
        "ws-a": [
          makeThread(
            id: "thread-a1", title: "A1", workspaceID: "ws-a", activeSessionID: "session-a1",
            updatedAt: 30)
        ],
        "ws-b": [
          makeThread(
            id: "thread-b1", title: "B1", workspaceID: "ws-b", activeSessionID: "session-b1",
            updatedAt: 20)
        ],
      ],
      worktreesByWorkspaceID: [:],
      selectedThreadID: "thread-a1"
    )

    sessionViewModel.removeWorkspace(id: "ws-a")

    XCTAssertEqual(sessionViewModel.workspaces.map(\.id), ["ws-b"])
    XCTAssertEqual(sessionViewModel.selectedWorkspaceID, "ws-b")
    XCTAssertEqual(sessionViewModel.selectedThreadID, "thread-b1")
    XCTAssertEqual(sessionViewModel.workspaceThreadGroups.map(\.workspace.id), ["ws-b"])
  }

  @MainActor
  func testSessionViewModelSortsThreadsRecentFirstByDefaultAndCanSwitchToCreatedOrder() {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [
          makeThread(
            id: "updated-first", title: "Updated first", workspaceID: "ws-repo",
            activeSessionID: "session-1", createdAt: 10, updatedAt: 30),
          makeThread(
            id: "created-first", title: "Created first", workspaceID: "ws-repo",
            activeSessionID: "session-2", createdAt: 40, updatedAt: 20),
          makeThread(
            id: "oldest", title: "Oldest", workspaceID: "ws-repo", activeSessionID: "session-3",
            createdAt: 5, updatedAt: 5),
        ]
      ],
      worktreesByWorkspaceID: [:]
    )

    XCTAssertEqual(
      sessionViewModel.workspaceThreadGroups.first?.threads.map(\.id),
      ["updated-first", "created-first", "oldest"]
    )
    XCTAssertEqual(
      sessionViewModel.chronologicalThreadEntries.map(\.thread.id),
      ["updated-first", "created-first", "oldest"]
    )

    sessionViewModel.sortMode = .created

    XCTAssertEqual(
      sessionViewModel.workspaceThreadGroups.first?.threads.map(\.id),
      ["created-first", "updated-first", "oldest"]
    )
    XCTAssertEqual(
      sessionViewModel.chronologicalThreadEntries.map(\.thread.id),
      ["created-first", "updated-first", "oldest"]
    )
  }

  @MainActor
  func testSessionViewModelRefreshKeepsThreadOwnershipWhenBackendCurrentWorkspaceChanges()
    async throws
  {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let context = try await makeSessionViewModelContext(
      defaultsSuiteName: defaultsSuiteName,
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "workspace-general",
              "name": "General",
              "path": "",
              "kind": "general",
              "repo": null,
              "last_opened_at": 0
            },
            {
              "id": "ws-repo-b",
              "name": "Repo B",
              "path": "/tmp/repo-b",
              "kind": "repository",
              "repo": {
                "root": "/tmp/repo-b",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": """
        {
          "sessions": [
            {
              "key": "session-a1",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "A1",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300,
              "message_count": 1
            },
            {
              "key": "session-b1",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "B1",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200,
              "message_count": 1
            }
          ],
          "total": 2
        }
        """,
        "/v1/workspaces/workspace-general/threads": #"{"threads":[],"total":0}"#,
        "/v1/workspaces/workspace-general/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo-b/threads": """
        {
          "threads": [
            {
              "id": "thread-a1",
              "title": "A1",
              "kind": "coding",
              "workspace_id": "ws-repo-b",
              "worktree_id": null,
              "active_session_id": "session-a1",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300
            },
            {
              "id": "thread-b1",
              "title": "B1",
              "kind": "coding",
              "workspace_id": "ws-repo-b",
              "worktree_id": null,
              "active_session_id": "session-b1",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000200
            }
          ],
          "total": 2
        }
        """,
        "/v1/workspaces/ws-repo-b/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a"),
        makeWorkspace(id: "ws-repo-b", name: "Repo B", kind: .repository, path: "/tmp/repo-b"),
      ],
      threadsByWorkspaceID: [
        "ws-repo-a": [
          makeThread(
            id: "thread-a1", title: "A1", workspaceID: "ws-repo-a", activeSessionID: "session-a1",
            updatedAt: 30)
        ],
        "ws-repo-b": [
          makeThread(
            id: "thread-b1", title: "B1", workspaceID: "ws-repo-b", activeSessionID: "session-b1",
            updatedAt: 20)
        ],
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo-b"
    )

    await context.sessionViewModel.refresh()

    XCTAssertFalse(context.sessionViewModel.workspaces.contains(where: \.isGeneral))
    XCTAssertEqual(context.sessionViewModel.workspaces.map(\.id), ["ws-repo-a", "ws-repo-b"])
    XCTAssertEqual(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo-a"]?.map(\.activeSessionID),
      ["session-a1"]
    )
    XCTAssertEqual(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo-b"]?.map(\.activeSessionID),
      ["session-b1"]
    )
  }

  @MainActor
  func testSessionViewModelRefreshDeduplicatesLegacyThreadSnapshotsAcrossWorkspaces()
    async throws
  {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let context = try await makeSessionViewModelContext(
      defaultsSuiteName: defaultsSuiteName,
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "workspace-general",
              "name": "General",
              "path": "",
              "kind": "general",
              "repo": null,
              "last_opened_at": 0
            },
            {
              "id": "ws-repo-b",
              "name": "Repo B",
              "path": "/tmp/repo-b",
              "kind": "repository",
              "repo": {
                "root": "/tmp/repo-b",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 2
        }
        """,
        "/v1/sessions": """
        {
          "sessions": [
            {
              "key": "session-legacy",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "Legacy thread",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300,
              "message_count": 1
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo-a/threads": """
        {
          "threads": [
            {
              "id": "thread-legacy-a",
              "title": "Legacy thread",
              "kind": "coding",
              "workspace_id": "ws-repo-a",
              "worktree_id": null,
              "active_session_id": "session-legacy",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo-a/worktrees": #"{"worktrees":[],"total":0}"#,
        "/v1/workspaces/ws-repo-b/threads": """
        {
          "threads": [
            {
              "id": "thread-legacy-b",
              "title": "Legacy thread",
              "kind": "coding",
              "workspace_id": "ws-repo-b",
              "worktree_id": null,
              "active_session_id": "session-legacy",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000300
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo-b/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo-a", name: "Repo A", kind: .repository, path: "/tmp/repo-a"),
        makeWorkspace(id: "ws-repo-b", name: "Repo B", kind: .repository, path: "/tmp/repo-b"),
      ],
      threadsByWorkspaceID: [
        "ws-repo-a": [
          makeThread(
            id: "thread-legacy-a",
            title: "Legacy thread",
            workspaceID: "ws-repo-a",
            activeSessionID: "session-legacy",
            updatedAt: 1_710_000_300
          )
        ]
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-repo-a"
    )

    await context.sessionViewModel.refresh()

    XCTAssertEqual(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo-a"]?.map(\.activeSessionID),
      ["session-legacy"]
    )
    XCTAssertFalse(
      context.sessionViewModel.threadsByWorkspaceID["ws-repo-b"]?.contains(where: {
        $0.activeSessionID == "session-legacy"
      }) == true
    )
  }

  @MainActor
  func testSessionViewModelRenameThreadPersistsAcrossRefreshes() async throws {
    let defaultsSuiteName = uniqueDefaultsSuiteName()
    let context = try await makeSessionViewModelContext(
      defaultsSuiteName: defaultsSuiteName,
      responses: [
        "/v1/workspaces": """
        {
          "workspaces": [
            {
              "id": "ws-repo",
              "name": "Repo",
              "path": "/tmp/repo",
              "kind": "repository",
              "repo": {
                "root": "/tmp/repo",
                "vcs": "git",
                "current_branch": "dev",
                "default_branch": "main",
                "origin": null,
                "clean": true
              },
              "last_opened_at": 1710000100
            }
          ],
          "total": 1
        }
        """,
        "/v1/sessions": """
        {
          "sessions": [
            {
              "key": "session-1",
              "kind": "main",
              "status": "idle",
              "label": null,
              "title": "Original",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000100,
              "message_count": 1
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/threads": """
        {
          "threads": [
            {
              "id": "thread-1",
              "title": "Original",
              "kind": "coding",
              "workspace_id": "ws-repo",
              "worktree_id": null,
              "active_session_id": "session-1",
              "status": "idle",
              "preview": null,
              "model": "gpt-5.4",
              "created_at": 1710000000,
              "updated_at": 1710000100
            }
          ],
          "total": 1
        }
        """,
        "/v1/workspaces/ws-repo/worktrees": #"{"worktrees":[],"total":0}"#,
      ]
    )
    defer {
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    await context.sessionViewModel.refresh()
    context.sessionViewModel.renameThread(id: "thread-1", title: "Renamed thread")

    let reloaded = SessionViewModel(appState: context.appState, userDefaults: context.defaults)
    await reloaded.refresh()

    let reloadedThread = try XCTUnwrap(reloaded.thread("thread-1"))
    XCTAssertEqual(reloaded.threadDisplayTitle(reloadedThread), "Renamed thread")
  }

  @MainActor
  func testSessionViewModelArchiveAndUnarchiveRestoreThreadVisibility() async throws {
    let archivedPayload = makeSessionPayload(
      id: "session-1",
      title: "Archived thread",
      updatedAt: 1_710_000_300,
      archived: true,
      archivedAt: 1_710_000_350
    )
    let restoredPayload = makeSessionPayload(
      id: "session-1",
      title: "Restored thread",
      updatedAt: 1_710_000_360,
      archived: false,
      archivedAt: nil
    )
    let context = try await makeSessionViewModelContext { request in
      guard let path = request.url?.path else {
        return .json("{}", statusCode: 400)
      }

      switch (request.httpMethod, path) {
      case ("POST", "/v1/sessions/session-1/archive"):
        return .json(archivedPayload)
      case ("DELETE", "/v1/sessions/session-1/archive"):
        return .json(restoredPayload)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockAppStateURLProtocol.reset()
      try? KeychainHelper.deleteToken(
        forServer: context.serverURL,
        service: context.keychainService
      )
    }

    context.sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [
          makeThread(
            id: "thread-1",
            title: "Archived thread",
            workspaceID: "ws-repo",
            activeSessionID: "session-1",
            updatedAt: 1_710_000_200
          )
        ]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(
          id: "session-1",
          title: "Archived thread",
          updatedAt: 1_710_000_200
        )
      ],
      selectedThreadID: "thread-1"
    )

    XCTAssertEqual(
      context.sessionViewModel.workspaceThreadGroups.first?.threads.map(\.id), ["thread-1"])

    let didArchive = await context.sessionViewModel.archiveThread(id: "thread-1")

    XCTAssertTrue(didArchive)
    XCTAssertNil(context.sessionViewModel.selectedThreadID)
    XCTAssertTrue(context.sessionViewModel.workspaceThreadGroups.first?.threads.isEmpty == true)
    XCTAssertTrue(context.sessionViewModel.workspaceThreadGroups.first?.showsStartThreadRow == true)
    XCTAssertEqual(context.sessionViewModel.archivedSessions.map(\.id), ["session-1"])

    let didRestore = await context.sessionViewModel.unarchiveSession(id: "session-1")

    XCTAssertTrue(didRestore)
    XCTAssertEqual(context.sessionViewModel.selectedThreadID, "thread-1")
    XCTAssertEqual(context.sessionViewModel.selectedSessionID, "session-1")
    XCTAssertEqual(
      context.sessionViewModel.workspaceThreadGroups.first?.threads.map(\.id), ["thread-1"])
    XCTAssertTrue(context.sessionViewModel.archivedSessions.isEmpty)

    let requests = MockAppStateURLProtocol.recordedRequests()
    XCTAssertTrue(
      requests.contains {
        $0.httpMethod == "POST" && $0.url?.path == "/v1/sessions/session-1/archive"
      }
    )
    XCTAssertTrue(
      requests.contains {
        $0.httpMethod == "DELETE" && $0.url?.path == "/v1/sessions/session-1/archive"
      }
    )
  }

  @MainActor
  func testSessionViewModelThreadDisplayTitlePrefersWorktreeLabelForGenericThreadNames() {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let thread = makeThread(
      id: "thread-worktree",
      title: "New Thread",
      workspaceID: "ws-repo",
      worktreeID: "wt-main",
      activeSessionID: "session-worktree",
      updatedAt: 40
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [thread]
      ],
      worktreesByWorkspaceID: [
        "ws-repo": [
          WorktreeSummary(
            id: "wt-main",
            workspaceID: "ws-repo",
            label: "main",
            path: "/tmp/repo/.worktrees/main",
            branch: "main",
            baseRef: "origin/main",
            status: .active,
            clean: true,
            aheadCount: 0,
            behindCount: 0
          )
        ]
      ]
    )

    XCTAssertEqual(sessionViewModel.threadDisplayTitle(thread), "main")
    XCTAssertEqual(
      sessionViewModel.threadContextLabel(thread, includeWorkspace: true),
      "Repo · Clean"
    )
  }

  @MainActor
  func testSelectedThreadContextSnapshotIncludesBoundWorktreeIdentity() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let thread = makeThread(
      id: "thread-worktree",
      title: "New Thread",
      workspaceID: "ws-repo",
      worktreeID: "wt-feature",
      activeSessionID: "session-worktree",
      updatedAt: 60
    )
    let session = Session(
      key: "session-worktree",
      kind: .main,
      status: .idle,
      label: nil,
      title: "Thread session",
      preview: "Latest reply",
      model: "gpt-5.4",
      createdAt: 50,
      updatedAt: 60,
      messageCount: 7
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        WorkspaceSummary(
          id: "ws-repo",
          name: "Repo",
          path: "/tmp/repo",
          kind: .repository,
          repo: RepositorySummary(
            root: "/tmp/repo",
            vcs: "git",
            currentBranch: "dev",
            defaultBranch: "main",
            origin: "git@github.com:example/fawx.git",
            clean: true
          ),
          lastOpenedAt: 1_710_000_000
        )
      ],
      threadsByWorkspaceID: [
        "ws-repo": [thread]
      ],
      worktreesByWorkspaceID: [
        "ws-repo": [
          WorktreeSummary(
            id: "wt-feature",
            workspaceID: "ws-repo",
            label: "feature-lane",
            path: "/tmp/repo/.worktrees/feature-lane",
            branch: "feature/thread-context",
            baseRef: "origin/dev",
            status: .active,
            clean: false,
            aheadCount: 2,
            behindCount: 1
          )
        ]
      ],
      sessions: [session],
      selectedWorkspaceID: "ws-repo",
      selectedThreadID: "thread-worktree"
    )

    let snapshot = try XCTUnwrap(sessionViewModel.selectedThreadContextSnapshot)

    XCTAssertEqual(snapshot.binding, .worktree)
    XCTAssertEqual(snapshot.displayTitle, "feature-lane")
    XCTAssertEqual(snapshot.workspaceName, "Repo")
    XCTAssertEqual(snapshot.workspacePath, "/tmp/repo")
    XCTAssertEqual(snapshot.worktreeLabel, "feature-lane")
    XCTAssertEqual(snapshot.worktreePath, "/tmp/repo/.worktrees/feature-lane")
    XCTAssertEqual(snapshot.branchName, "feature/thread-context")
    XCTAssertEqual(snapshot.baseRef, "origin/dev")
    XCTAssertEqual(snapshot.repositoryOrigin, "git@github.com:example/fawx.git")
    XCTAssertEqual(snapshot.model, "gpt-5.4")
    XCTAssertEqual(snapshot.messageCount, 7)
    XCTAssertEqual(snapshot.isClean, false)
    XCTAssertEqual(snapshot.divergenceLabel, "↑2 ↓1")
    XCTAssertEqual(snapshot.worktreeStatusLabel, "Active")
    XCTAssertTrue(snapshot.hasRepositoryContext)
    XCTAssertTrue(snapshot.showsHeaderIdentity)
    XCTAssertEqual(
      sessionViewModel.threadContextLabel(thread, includeWorkspace: true),
      "Repo · feature/thread-context · Dirty ↑2 ↓1"
    )
  }

  @MainActor
  func testSelectedThreadContextSnapshotDoesNotInventGitIdentityForGeneralThread() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let thread = makeThread(
      id: "thread-general",
      title: "General chat",
      workspaceID: "ws-general",
      activeSessionID: "session-general",
      updatedAt: 12
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        WorkspaceSummary(
          id: "ws-general",
          name: "General",
          path: "/tmp/general",
          kind: .general,
          repo: nil,
          lastOpenedAt: 1_710_000_000
        )
      ],
      threadsByWorkspaceID: [
        "ws-general": [thread]
      ],
      worktreesByWorkspaceID: [:],
      selectedWorkspaceID: "ws-general",
      selectedThreadID: "thread-general"
    )

    let snapshot = try XCTUnwrap(sessionViewModel.selectedThreadContextSnapshot)

    XCTAssertEqual(snapshot.binding, .general)
    XCTAssertEqual(snapshot.workspaceName, "General")
    XCTAssertNil(snapshot.branchName)
    XCTAssertNil(snapshot.worktreeLabel)
    XCTAssertNil(snapshot.baseRef)
    XCTAssertFalse(snapshot.hasRepositoryContext)
    XCTAssertFalse(snapshot.showsHeaderIdentity)
    XCTAssertNil(sessionViewModel.threadContextLabel(thread, includeWorkspace: true))
  }

  @MainActor
  func testThreadActivitySnapshotSurfacesBackgroundWorkByThread() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let threadA = makeThread(
      id: "thread-a",
      title: "Thread A",
      workspaceID: "ws-repo",
      activeSessionID: "session-a",
      updatedAt: 20
    )
    let threadB = makeThread(
      id: "thread-b",
      title: "Thread B",
      workspaceID: "ws-repo",
      activeSessionID: "session-b",
      updatedAt: 40
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [threadB, threadA]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(id: "session-a", title: "Thread A", updatedAt: 20),
        makeDetailedSession(id: "session-b", title: "Thread B", updatedAt: 40),
      ],
      selectedWorkspaceID: "ws-repo",
      selectedThreadID: "thread-a",
      viewedThreadUpdateAtByID: ["thread-b": 10]
    )

    sessionViewModel.syncRuntimeActivity([
      "session-b": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 2,
        runningToolCallCount: 1,
        progressLabel: "Implementing"
      )
    ])

    let selectedActivity = try XCTUnwrap(sessionViewModel.selectedThreadActivitySnapshot)
    XCTAssertEqual(selectedActivity.threadID, "thread-a")
    XCTAssertFalse(selectedActivity.isRunning)

    let backgroundActivity = sessionViewModel.threadActivitySnapshot(for: threadB)
    XCTAssertTrue(backgroundActivity.isRunning)
    XCTAssertEqual(backgroundActivity.badgeLabel, "Implementing")
    XCTAssertFalse(backgroundActivity.showsUnreadIndicator)

    let notice = try XCTUnwrap(sessionViewModel.selectedBackgroundActivityNotice)
    XCTAssertEqual(notice.message, "Running in another thread: Thread B")
    XCTAssertEqual(notice.detail, "Thread B · Implementing")
  }

  @MainActor
  func testBackgroundActivityNoticeFollowsSelectedThreadContext() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let threadA = makeThread(
      id: "thread-a",
      title: "Thread A",
      workspaceID: "ws-repo",
      activeSessionID: "session-a",
      updatedAt: 20
    )
    let threadB = makeThread(
      id: "thread-b",
      title: "Thread B",
      workspaceID: "ws-repo",
      activeSessionID: "session-b",
      updatedAt: 40
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [threadB, threadA]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(id: "session-a", title: "Thread A", updatedAt: 20),
        makeDetailedSession(id: "session-b", title: "Thread B", updatedAt: 40),
      ],
      selectedWorkspaceID: "ws-repo",
      selectedThreadID: "thread-a"
    )

    sessionViewModel.syncRuntimeActivity([
      "session-b": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Running"
      )
    ])

    XCTAssertEqual(sessionViewModel.selectedBackgroundActivityNotice?.primaryThreadID, "thread-b")

    sessionViewModel.selectThread(id: "thread-b")
    XCTAssertEqual(sessionViewModel.selectedThreadActivitySnapshot?.threadID, "thread-b")
    XCTAssertNil(sessionViewModel.selectedBackgroundActivityNotice)

    sessionViewModel.syncRuntimeActivity([
      "session-a": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Implementing"
      ),
      "session-b": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Running"
      ),
    ])

    XCTAssertNil(sessionViewModel.selectedBackgroundActivityNotice)
  }

  @MainActor
  func testBackgroundActivityOverviewNoticeDoesNotRequireSelectedThread() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let threadA = makeThread(
      id: "thread-a",
      title: "Thread A",
      workspaceID: "ws-repo",
      activeSessionID: "session-a",
      updatedAt: 20
    )
    let threadB = makeThread(
      id: "thread-b",
      title: "Thread B",
      workspaceID: "ws-repo",
      activeSessionID: "session-b",
      updatedAt: 40
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [threadB, threadA]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(id: "session-a", title: "Thread A", updatedAt: 20),
        makeDetailedSession(id: "session-b", title: "Thread B", updatedAt: 40),
      ],
      selectedWorkspaceID: "ws-repo"
    )

    sessionViewModel.syncRuntimeActivity([
      "session-b": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Implementing"
      )
    ])

    XCTAssertNil(sessionViewModel.selectedBackgroundActivityNotice)

    let notice = try XCTUnwrap(sessionViewModel.backgroundActivityOverviewNotice)
    XCTAssertEqual(notice.primaryThreadID, "thread-b")
    XCTAssertEqual(notice.overviewMessage, "Thread B is running")
    XCTAssertEqual(notice.detail, "Thread B · Implementing")
  }

  @MainActor
  func testBackgroundActivityNoticeCountsMultipleThreadsAndSubagents() throws {
    let appState = AppState(startLoadingPersistedState: false)
    let sessionViewModel = SessionViewModel(appState: appState)
    let threadA = makeThread(
      id: "thread-a",
      title: "Thread A",
      workspaceID: "ws-repo",
      activeSessionID: "session-a",
      updatedAt: 20
    )
    let threadB = makeThread(
      id: "thread-b",
      title: "Thread B",
      workspaceID: "ws-repo",
      activeSessionID: "session-b",
      updatedAt: 60
    )
    let threadC = makeThread(
      id: "thread-c",
      title: "Thread C",
      workspaceID: "ws-repo",
      activeSessionID: "session-c",
      kind: .subagent,
      updatedAt: 50
    )
    let threadD = makeThread(
      id: "thread-d",
      title: "Thread D",
      workspaceID: "ws-repo",
      activeSessionID: "session-d",
      updatedAt: 40
    )

    sessionViewModel.setSidebarDataForTesting(
      workspaces: [
        makeWorkspace(id: "ws-repo", name: "Repo", kind: .repository, path: "/tmp/repo")
      ],
      threadsByWorkspaceID: [
        "ws-repo": [threadA, threadB, threadC, threadD]
      ],
      worktreesByWorkspaceID: [:],
      sessions: [
        makeDetailedSession(id: "session-a", title: "Thread A", updatedAt: 20),
        makeDetailedSession(id: "session-b", title: "Thread B", updatedAt: 60),
        makeDetailedSession(id: "session-c", title: "Thread C", updatedAt: 50),
        makeDetailedSession(id: "session-d", title: "Thread D", updatedAt: 40),
      ],
      selectedWorkspaceID: "ws-repo",
      selectedThreadID: "thread-a"
    )

    sessionViewModel.syncRuntimeActivity([
      "session-b": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Planning"
      ),
      "session-c": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Delegating"
      ),
      "session-d": ThreadRuntimeActivity(
        isStreaming: true,
        liveToolCallCount: 1,
        runningToolCallCount: 1,
        progressLabel: "Implementing"
      ),
    ])

    let selectedNotice = try XCTUnwrap(sessionViewModel.selectedBackgroundActivityNotice)
    XCTAssertEqual(selectedNotice.primaryThreadID, "thread-b")
    XCTAssertEqual(selectedNotice.activeThreadCount, 3)
    XCTAssertEqual(selectedNotice.subagentThreadCount, 1)
    XCTAssertEqual(selectedNotice.message, "3 other threads running")
    XCTAssertEqual(selectedNotice.overviewMessage, "3 threads running")
    XCTAssertEqual(selectedNotice.detail, "Thread B · Planning · +2 more · 1 subagent")
    XCTAssertEqual(selectedNotice.compactLabel, "3 running")

    let overviewNotice = try XCTUnwrap(sessionViewModel.backgroundActivityOverviewNotice)
    XCTAssertEqual(overviewNotice.activeThreadCount, 3)
    XCTAssertEqual(overviewNotice.subagentThreadCount, 1)
    XCTAssertEqual(overviewNotice.overviewMessage, "3 threads running")
  }

  @MainActor
  func testGitViewModelClearsScopedStateWhenBindingChangesToGeneralThread() {
    let appState = AppState(startLoadingPersistedState: false)
    let gitViewModel = GitViewModel(appState: appState)

    gitViewModel.bindThreadContext(
      ThreadContextSnapshot(
        thread: .init(
          id: "thread-repo",
          sessionID: "session-repo",
          displayTitle: "Repo thread",
          kind: .coding,
          status: .active,
          model: "gpt-5.4",
          messageCount: 3
        ),
        workspace: .init(
          name: "Repo",
          path: "/tmp/repo",
          kind: .repository
        ),
        repository: .init(
          branchName: "feature/thread-context",
          worktreeLabel: "feature-lane",
          worktreePath: "/tmp/repo/.worktrees/feature-lane",
          baseRef: "origin/dev",
          origin: "git@github.com:example/fawx.git",
          isClean: false,
          divergenceLabel: "↑1",
          worktreeStatusLabel: "Active"
        ),
        binding: .worktree,
      )
    )
    gitViewModel.status = GitStatusResponse(
      branch: "feature/thread-context",
      files: [GitFileEntry(path: "README.md", status: .modified, staged: false)],
      clean: false
    )
    gitViewModel.diff = GitDiffResponse(
      diff: "diff --git a/README.md b/README.md",
      filesChanged: 1,
      insertions: 1,
      deletions: 0
    )
    gitViewModel.commits = [
      GitCommitEntry(
        hash: "abc123",
        shortHash: "abc123",
        message: "Update README",
        author: "Joseph",
        timestamp: "2026-04-09T00:00:00Z"
      )
    ]
    gitViewModel.errorMessage = "stale"
    gitViewModel.selectedFilePath = "README.md"
    gitViewModel.commitMessage = "WIP"
    gitViewModel.lastActionSummary = "Pushed branch."
    gitViewModel.pendingConfirmation = GitConfirmationRequest(action: .push)

    let generalContext = ThreadContextSnapshot(
      thread: .init(
        id: "thread-general",
        sessionID: "session-general",
        displayTitle: "General thread",
        kind: .general,
        status: .idle,
        model: "gpt-5.4-mini",
        messageCount: 1
      ),
      workspace: .init(
        name: nil,
        path: nil,
        kind: .general
      ),
      repository: .init(
        branchName: nil,
        worktreeLabel: nil,
        worktreePath: nil,
        baseRef: nil,
        origin: nil,
        isClean: nil,
        divergenceLabel: nil,
        worktreeStatusLabel: nil
      ),
      binding: .general,
    )

    gitViewModel.bindThreadContext(generalContext)

    XCTAssertEqual(gitViewModel.threadContext, generalContext)
    let expectedRefreshTaskID = [
      "thread",
      "thread:thread-general",
      "session-general",
      "",
      "",
      "",
      "",
    ].joined(separator: "|")
    XCTAssertEqual(gitViewModel.refreshTaskID, expectedRefreshTaskID)
    XCTAssertNil(gitViewModel.status)
    XCTAssertNil(gitViewModel.diff)
    XCTAssertTrue(gitViewModel.commits.isEmpty)
    XCTAssertNil(gitViewModel.errorMessage)
    XCTAssertNil(gitViewModel.selectedFilePath)
    XCTAssertEqual(gitViewModel.commitMessage, "")
    XCTAssertNil(gitViewModel.lastActionSummary)
    XCTAssertNil(gitViewModel.pendingConfirmation)
  }

  @MainActor
  func testGitViewModelRestoresCommitDraftPerThreadContext() {
    let appState = AppState(startLoadingPersistedState: false)
    let gitViewModel = GitViewModel(appState: appState)

    let firstContext = ThreadContextSnapshot(
      thread: .init(
        id: "thread-one",
        sessionID: "session-one",
        displayTitle: "Thread one",
        kind: .coding,
        status: .active,
        model: "gpt-5.4",
        messageCount: 3
      ),
      workspace: .init(
        name: "Repo",
        path: "/tmp/repo",
        kind: .repository
      ),
      repository: .init(
        branchName: "feature/one",
        worktreeLabel: "lane-one",
        worktreePath: "/tmp/repo/.worktrees/lane-one",
        baseRef: "origin/dev",
        origin: "git@github.com:example/fawx.git",
        isClean: false,
        divergenceLabel: "↑1",
        worktreeStatusLabel: "Active"
      ),
      binding: .worktree,
    )
    let secondContext = ThreadContextSnapshot(
      thread: .init(
        id: "thread-two",
        sessionID: "session-two",
        displayTitle: "Thread two",
        kind: .coding,
        status: .active,
        model: "gpt-5.4",
        messageCount: 4
      ),
      workspace: .init(
        name: "Repo",
        path: "/tmp/repo",
        kind: .repository
      ),
      repository: .init(
        branchName: "feature/two",
        worktreeLabel: "lane-two",
        worktreePath: "/tmp/repo/.worktrees/lane-two",
        baseRef: "origin/dev",
        origin: "git@github.com:example/fawx.git",
        isClean: true,
        divergenceLabel: nil,
        worktreeStatusLabel: "Active"
      ),
      binding: .worktree,
    )

    gitViewModel.bindThreadContext(firstContext)
    gitViewModel.commitMessage = "Draft for thread one"

    gitViewModel.bindThreadContext(secondContext)

    XCTAssertEqual(gitViewModel.commitMessage, "")

    gitViewModel.commitMessage = "Draft for thread two"
    gitViewModel.bindThreadContext(firstContext)

    XCTAssertEqual(gitViewModel.commitMessage, "Draft for thread one")

    gitViewModel.bindThreadContext(secondContext)

    XCTAssertEqual(gitViewModel.commitMessage, "Draft for thread two")
  }

  @MainActor
  func testThreadContextContextLineSuppressesDuplicateWorkspaceAndWorktreeLabels() {
    let snapshot = ThreadContextSnapshot(
      thread: .init(
        id: "thread-dup",
        sessionID: "session-dup",
        displayTitle: "Thread Dup",
        kind: .coding,
        status: .active,
        model: "gpt-5.4",
        messageCount: 2
      ),
      workspace: .init(
        name: "Repo",
        path: "/tmp/repo",
        kind: .repository
      ),
      repository: .init(
        branchName: "feature/repo",
        worktreeLabel: "Repo",
        worktreePath: "/tmp/repo/.worktrees/Repo",
        baseRef: "origin/dev",
        origin: "git@github.com:example/fawx.git",
        isClean: true,
        divergenceLabel: nil,
        worktreeStatusLabel: "Active"
      ),
      binding: .worktree
    )

    XCTAssertEqual(snapshot.contextLine(includeWorkspace: true), "Repo · /tmp/repo/.worktrees/Repo")
    XCTAssertFalse(snapshot.contextLine(includeWorkspace: true)?.contains("Repo · Repo") ?? false)
  }

  @MainActor
  func testGitViewModelBranchTitleFallsBackFromStatusToThreadContextAndDefault() {
    let appState = AppState(startLoadingPersistedState: false)
    let gitViewModel = GitViewModel(appState: appState)

    XCTAssertEqual(gitViewModel.branchTitle, "Git")

    gitViewModel.bindThreadContext(
      makeGitThreadContext(
        threadID: "thread-header",
        sessionID: "session-header",
        displayTitle: "Header thread",
        branchName: "feature/header",
        worktreeLabel: "lane-header"
      )
    )

    XCTAssertEqual(gitViewModel.branchTitle, "feature/header")

    gitViewModel.bindThreadContext(
      makeGitThreadContext(
        threadID: "thread-worktree",
        sessionID: "session-worktree",
        displayTitle: "Worktree thread",
        branchName: nil,
        worktreeLabel: "lane-only"
      )
    )

    XCTAssertEqual(gitViewModel.branchTitle, "lane-only")

    gitViewModel.status = GitStatusResponse(
      branch: "status-branch",
      files: [],
      clean: true
    )

    XCTAssertEqual(gitViewModel.branchTitle, "status-branch")
  }

  @MainActor
  func testRefreshContextIgnoresStaleSessionResponsesAfterSelectionChanges() async {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [ContextRaceURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let sut = AppState(
      client: client,
      startLoadingPersistedState: false
    )
    let firstContextStarted = expectation(description: "first context request started")
    let secondContextStarted = expectation(description: "second context request started")
    let releaseFirstContext = DispatchSemaphore(value: 0)

    defer {
      releaseFirstContext.signal()
      ContextRaceURLProtocol.reset()
    }

    ContextRaceURLProtocol.configure(
      releaseFirstContext: releaseFirstContext,
      firstContextStarted: { firstContextStarted.fulfill() },
      secondContextStarted: { secondContextStarted.fulfill() }
    )

    let firstRefreshTask = Task {
      await sut.refreshContext(for: "session-a")
    }
    await fulfillment(of: [firstContextStarted], timeout: 1)

    let secondRefreshTask = Task {
      await sut.refreshContext(for: "session-b")
    }
    await fulfillment(of: [secondContextStarted], timeout: 1)
    await secondRefreshTask.value

    XCTAssertEqual(sut.currentContext?.usedTokens, 80)

    releaseFirstContext.signal()
    await firstRefreshTask.value

    XCTAssertEqual(sut.currentContext?.usedTokens, 80)
    XCTAssertEqual(sut.currentContext?.normalizedPercentage, 80)
  }

  @MainActor
  func testGitViewModelRefreshReplaysNewestThreadContextAfterInFlightRefresh() async throws {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [MockAppStateURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let appState = AppState(
      client: client,
      startLoadingPersistedState: false
    )
    try await appState.savePairing(
      serverURLString: "http://localhost:8400",
      token: "test-token",
      deviceName: "Desk Mac",
      connectionMode: .remote
    )

    let gitViewModel = GitViewModel(appState: appState)
    let firstContext = makeGitThreadContext(
      threadID: "thread-one",
      sessionID: "session-one",
      displayTitle: "Thread one",
      branchName: "feature/one",
      worktreeLabel: "lane-one"
    )
    let secondContext = makeGitThreadContext(
      threadID: "thread-two",
      sessionID: "session-two",
      displayTitle: "Thread two",
      branchName: "feature/two",
      worktreeLabel: "lane-two"
    )
    let firstStatusStarted = expectation(description: "first status request started")
    let secondStatusStarted = expectation(description: "second status request started")
    let releaseFirstStatus = DispatchSemaphore(value: 0)
    let requestCounter = LockedIntCounter()
    let queuedRefreshDidComplete = LockedBoolFlag()

    defer {
      releaseFirstStatus.signal()
      MockAppStateURLProtocol.reset()
    }

    MockAppStateURLProtocol.setResponder { request in
      switch request.url?.path {
      case "/v1/git/status":
        let requestNumber = requestCounter.incrementAndRead()

        if requestNumber == 1 {
          firstStatusStarted.fulfill()
          _ = releaseFirstStatus.wait(timeout: .now() + 2)
          return .json(
            #"{"branch":"feature/one","files":[{"path":"README.md","status":"modified","staged":false}],"clean":false}"#
          )
        }

        secondStatusStarted.fulfill()
        return .json(
          #"{"branch":"feature/two","files":[],"clean":true}"#
        )
      case "/v1/git/diff":
        return .json(
          #"{"diff":"diff --git a/README.md b/README.md","files_changed":1,"insertions":1,"deletions":0}"#
        )
      case "/v1/git/log":
        return .json(#"{"commits":[]}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }

    gitViewModel.bindThreadContext(firstContext)
    let initialRefreshTask = Task {
      await gitViewModel.refresh()
    }

    await fulfillment(of: [firstStatusStarted], timeout: 1)

    gitViewModel.bindThreadContext(secondContext)
    let queuedRefreshTask = Task {
      await gitViewModel.refresh()
      queuedRefreshDidComplete.setTrue()
    }

    try await Task.sleep(nanoseconds: 50_000_000)
    XCTAssertFalse(queuedRefreshDidComplete.value)

    releaseFirstStatus.signal()
    await fulfillment(of: [secondStatusStarted], timeout: 1)

    _ = await queuedRefreshTask.value
    _ = await initialRefreshTask.value

    XCTAssertEqual(gitViewModel.threadContext, secondContext)
    XCTAssertEqual(gitViewModel.status?.branch, "feature/two")
    XCTAssertEqual(gitViewModel.branchTitle, "feature/two")
    XCTAssertNil(gitViewModel.errorMessage)
    XCTAssertFalse(gitViewModel.isLoading)
    XCTAssertEqual(
      MockAppStateURLProtocol.recordedRequests().filter { $0.url?.path == "/v1/git/status" }.count,
      2
    )
  }

  func testClearWorkspaceRootClearsWorkspaceAndWorkingDirectory() async throws {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [MockAppStateURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let sut = await MainActor.run {
      AppState(
        client: client,
        startLoadingPersistedState: false
      )
    }

    MockAppStateURLProtocol.setResponder { request in
      switch request.url?.path {
      case "/v1/config":
        return .json(
          """
          {
            "updated": true,
            "restart_required": false,
            "changed_keys": ["workspace.root", "tools.working_dir"]
          }
          """
        )
      case "/v1/models":
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
      case "/v1/status":
        return .json(
          """
          {
            "status": "ok",
            "model": "gpt-5.4",
            "skills": [],
            "memory_entries": 0,
            "tailscale_ip": null,
            "config": {
              "permission_mode": "workspace-write"
            }
          }
          """
        )
      case "/v1/permissions":
        return .json(
          """
          {
            "preset": "workspace-write",
            "mode": "prompt",
            "permissions": [],
            "available_presets": ["workspace-write"]
          }
          """
        )
      case "/v1/thinking":
        return .json(
          """
          {
            "level": "medium",
            "valid_levels": ["low", "medium", "high"]
          }
          """
        )
      case "/v1/auth":
        return .json(#"{"providers":[]}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }

    let response = try await sut.clearWorkspaceRoot(showToast: false)
    let requests = MockAppStateURLProtocol.recordedRequests()
    MockAppStateURLProtocol.reset()

    XCTAssertTrue(response.updated)
    let patchRequest = try XCTUnwrap(
      requests.first(where: { $0.httpMethod == "PATCH" && $0.url?.path == "/v1/config" })
    )
    let patchBody = try XCTUnwrap(patchRequest.bodyDataForTesting())
    let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: patchBody) as? [String: Any])
    let changes = try XCTUnwrap(payload["changes"] as? [String: Any])
    let workspace = try XCTUnwrap(changes["workspace"] as? [String: Any])
    let tools = try XCTUnwrap(changes["tools"] as? [String: Any])

    XCTAssertTrue(workspace["root"] is NSNull)
    XCTAssertTrue(tools["working_dir"] is NSNull)
  }

  func testUpdateWorkspaceRootDefersNormalizationToServer() async throws {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [MockAppStateURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let sut = await MainActor.run {
      AppState(
        client: client,
        startLoadingPersistedState: false
      )
    }

    MockAppStateURLProtocol.setResponder { request in
      switch request.url?.path {
      case "/v1/config":
        return .json(
          """
          {
            "updated": true,
            "restart_required": false,
            "changed_keys": ["workspace.root", "tools.working_dir"]
          }
          """
        )
      case "/v1/models":
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
      case "/v1/status":
        return .json(
          """
          {
            "status": "ok",
            "model": "gpt-5.4",
            "skills": [],
            "memory_entries": 0,
            "tailscale_ip": null,
            "config": {
              "permission_mode": "workspace-write"
            }
          }
          """
        )
      case "/v1/permissions":
        return .json(
          """
          {
            "preset": "workspace-write",
            "mode": "prompt",
            "permissions": [],
            "available_presets": ["workspace-write"]
          }
          """
        )
      case "/v1/thinking":
        return .json(
          """
          {
            "level": "medium",
            "valid_levels": ["low", "medium", "high"]
          }
          """
        )
      case "/v1/auth":
        return .json(#"{"providers":[]}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }

    let rawPath = "  /tmp/fawx workspace  "
    let response = try await sut.updateWorkspaceRoot(rawPath, showToast: false)
    let requests = MockAppStateURLProtocol.recordedRequests()
    MockAppStateURLProtocol.reset()

    XCTAssertTrue(response.updated)
    let patchRequest = try XCTUnwrap(
      requests.first(where: { $0.httpMethod == "PATCH" && $0.url?.path == "/v1/config" })
    )
    let patchBody = try XCTUnwrap(patchRequest.bodyDataForTesting())
    let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: patchBody) as? [String: Any])
    let changes = try XCTUnwrap(payload["changes"] as? [String: Any])
    let workspace = try XCTUnwrap(changes["workspace"] as? [String: Any])
    let tools = try XCTUnwrap(changes["tools"] as? [String: Any])

    XCTAssertEqual(workspace["root"] as? String, rawPath)
    XCTAssertEqual(tools["working_dir"] as? String, rawPath)
  }

  func testWorkspaceNavigationStateResolveSelectionMigratesLegacySessionSelectionToThreadSelection()
  {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .thread(.activeSessionID("session-1")),
      currentWorkspaceID: nil,
      currentThreadID: nil,
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: [:],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-a")
    XCTAssertEqual(resolution.threadID, "thread-a1")
    XCTAssertNil(resolution.pendingSessionSelectionID)
    XCTAssertTrue(resolution.shouldRewriteSidebarSelection)
    XCTAssertEqual(resolution.rewrittenSidebarSelection, .thread(.threadID("thread-a1")))
  }

  func testWorkspaceNavigationStateResolveSelectionFallsBackToRememberedThreadWithinWorkspace() {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .workspace("ws-a"),
      currentWorkspaceID: nil,
      currentThreadID: nil,
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: ["ws-a": "thread-a2"],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-a")
    XCTAssertEqual(resolution.threadID, "thread-a2")
    XCTAssertFalse(resolution.shouldRewriteSidebarSelection)
    XCTAssertNil(resolution.rewrittenSidebarSelection)
  }

  func testWorkspaceNavigationStateResolveSelectionPreservesWorkspaceDraftDuringRefresh() {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .workspace("ws-a"),
      currentWorkspaceID: "ws-a",
      currentThreadID: nil,
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: ["ws-a": "thread-a2"],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-a")
    XCTAssertNil(resolution.threadID)
    XCTAssertNil(resolution.pendingSessionSelectionID)
    XCTAssertFalse(resolution.shouldRewriteSidebarSelection)
    XCTAssertNil(resolution.rewrittenSidebarSelection)
  }

  func testWorkspaceNavigationStateResolveSelectionFallsBackWhenCurrentThreadWasDeleted() {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .workspace("ws-a"),
      currentWorkspaceID: "ws-a",
      currentThreadID: "thread-deleted",
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: ["ws-a": "thread-a2"],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-a")
    XCTAssertEqual(resolution.threadID, "thread-a2")
    XCTAssertNil(resolution.pendingSessionSelectionID)
    XCTAssertFalse(resolution.shouldRewriteSidebarSelection)
    XCTAssertNil(resolution.rewrittenSidebarSelection)
  }

  func testWorkspaceNavigationStateResolveSelectionFallsBackWithinCrossWorkspaceSelection() {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .workspace("ws-b"),
      currentWorkspaceID: "ws-a",
      currentThreadID: nil,
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: [:],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-b")
    XCTAssertEqual(resolution.threadID, "thread-b1")
    XCTAssertNil(resolution.pendingSessionSelectionID)
    XCTAssertFalse(resolution.shouldRewriteSidebarSelection)
    XCTAssertNil(resolution.rewrittenSidebarSelection)
  }

  func testWorkspaceNavigationStateResolveSelectionPreservesCurrentWorkspaceThreadDuringRefresh() {
    let catalog = makeNavigationCatalog()

    let resolution = WorkspaceNavigationState.resolveSelection(
      from: .workspace("ws-a"),
      currentWorkspaceID: "ws-a",
      currentThreadID: "thread-a1",
      pendingSessionSelectionID: nil,
      rememberedThreadIDByWorkspaceID: ["ws-a": "thread-a2"],
      in: catalog
    )

    XCTAssertEqual(resolution.workspaceID, "ws-a")
    XCTAssertEqual(resolution.threadID, "thread-a1")
    XCTAssertNil(resolution.pendingSessionSelectionID)
    XCTAssertFalse(resolution.shouldRewriteSidebarSelection)
    XCTAssertNil(resolution.rewrittenSidebarSelection)
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
  private func makeSessionViewModelContext(
    defaultsSuiteName: String? = nil,
    responses: [String: String]
  ) async throws -> (
    appState: AppState,
    sessionViewModel: SessionViewModel,
    defaults: UserDefaults,
    keychainService: String,
    serverURL: String
  ) {
    try await makeSessionViewModelContext(
      defaultsSuiteName: defaultsSuiteName,
      responder: makeJSONResponder(responses)
    )
  }

  @MainActor
  private func makeSessionViewModelContext(
    defaultsSuiteName: String? = nil,
    responder: @escaping MockAppStateURLProtocolStore.Responder
  ) async throws -> (
    appState: AppState,
    sessionViewModel: SessionViewModel,
    defaults: UserDefaults,
    keychainService: String,
    serverURL: String
  ) {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [MockAppStateURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let suiteName = defaultsSuiteName ?? uniqueDefaultsSuiteName()
    let defaults = makeUserDefaults(suiteName: suiteName)
    let keychainService = uniqueKeychainService()
    let serverURL = "http://localhost:8400"

    MockAppStateURLProtocol.setResponder(responder)

    let persistence = AppStatePersistence(
      defaultsSuiteName: suiteName,
      keychainService: keychainService,
      localInstallLoader: { nil }
    )
    let appState = AppState(
      persistence: persistence,
      client: client,
      startLoadingPersistedState: false
    )

    try await appState.savePairing(
      serverURLString: serverURL,
      token: "test-token",
      deviceName: "Desk Mac",
      connectionMode: .remote
    )

    let sessionViewModel = SessionViewModel(appState: appState, userDefaults: defaults)
    return (appState, sessionViewModel, defaults, keychainService, serverURL)
  }

  private func makeJSONResponder(
    _ responses: [String: String]
  ) -> MockAppStateURLProtocolStore.Responder {
    { request in
      guard let path = request.url?.path else {
        return .json("{}", statusCode: 400)
      }

      if let body = responses[path] {
        return .json(body)
      }

      return .json("{}", statusCode: 404)
    }
  }

  private func makeGitThreadContext(
    threadID: String = "thread-1",
    sessionID: String = "session-1",
    displayTitle: String = "Thread 1",
    branchName: String? = "feature/thread-context",
    worktreeLabel: String? = "thread-context",
    workspaceName: String? = "Repo",
    workspacePath: String? = "/tmp/repo",
    workspaceKind: WorkspaceKind = .repository,
    binding: ThreadContextSnapshot.Binding = .worktree,
    repositoryOrigin: String? = "git@github.com:example/fawx.git"
  ) -> ThreadContextSnapshot {
    ThreadContextSnapshot(
      thread: .init(
        id: threadID,
        sessionID: sessionID,
        displayTitle: displayTitle,
        kind: .coding,
        status: .active,
        model: "gpt-5.4",
        messageCount: 3
      ),
      workspace: .init(
        name: workspaceName,
        path: workspacePath,
        kind: workspaceKind
      ),
      repository: .init(
        branchName: branchName,
        worktreeLabel: worktreeLabel,
        worktreePath: worktreeLabel.map { "\(workspacePath ?? "/tmp/repo")/.worktrees/\($0)" },
        baseRef: "origin/dev",
        origin: repositoryOrigin,
        isClean: true,
        divergenceLabel: nil,
        worktreeStatusLabel: "Active"
      ),
      binding: binding,
    )
  }

  private func makeNavigationCatalog() -> WorkspaceNavigationCatalog {
    WorkspaceNavigationCatalog(
      workspaces: [
        WorkspaceSummary(
          id: "ws-a",
          name: "Workspace A",
          path: "/tmp/ws-a",
          kind: .general,
          repo: nil,
          lastOpenedAt: 1_710_000_000
        ),
        WorkspaceSummary(
          id: "ws-b",
          name: "Workspace B",
          path: "/tmp/ws-b",
          kind: .repository,
          repo: nil,
          lastOpenedAt: 1_710_000_100
        ),
      ],
      threadsByWorkspaceID: [
        "ws-a": [
          ThreadSummary(
            id: "thread-a1",
            title: "A1",
            kind: .general,
            workspaceID: "ws-a",
            worktreeID: nil,
            activeSessionID: "session-1",
            status: .active,
            preview: nil,
            model: "gpt-5.4",
            createdAt: 1_710_000_000,
            updatedAt: 1_710_000_200
          ),
          ThreadSummary(
            id: "thread-a2",
            title: "A2",
            kind: .general,
            workspaceID: "ws-a",
            worktreeID: nil,
            activeSessionID: "session-2",
            status: .idle,
            preview: nil,
            model: "gpt-5.4",
            createdAt: 1_710_000_000,
            updatedAt: 1_710_000_100
          ),
        ],
        "ws-b": [
          ThreadSummary(
            id: "thread-b1",
            title: "B1",
            kind: .coding,
            workspaceID: "ws-b",
            worktreeID: nil,
            activeSessionID: "session-3",
            status: .idle,
            preview: nil,
            model: "gpt-5.4",
            createdAt: 1_710_000_000,
            updatedAt: 1_710_000_050
          )
        ],
      ],
      allSessionsByID: [
        "session-1": makeSession(id: "session-1", updatedAt: 10),
        "session-2": makeSession(id: "session-2", updatedAt: 9),
        "session-3": makeSession(id: "session-3", updatedAt: 8),
      ]
    )
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

  private func makeDetailedSession(
    id: String,
    title: String,
    updatedAt: Int,
    archived: Bool = false,
    archivedAt: Int? = nil
  ) -> Session {
    Session(
      key: id,
      kind: .main,
      status: .idle,
      label: nil,
      title: title,
      preview: "Preview for \(title)",
      model: "gpt-5.4",
      createdAt: updatedAt - 10,
      updatedAt: updatedAt,
      messageCount: 2,
      archived: archived,
      archivedAt: archivedAt
    )
  }

  private func makeWorkspace(
    id: String,
    name: String,
    kind: WorkspaceKind,
    path: String
  ) -> WorkspaceSummary {
    WorkspaceSummary(
      id: id,
      name: name,
      path: path,
      kind: kind,
      repo: kind == .repository
        ? RepositorySummary(
          root: path,
          vcs: "git",
          currentBranch: "dev",
          defaultBranch: "main",
          origin: nil,
          clean: true
        )
        : nil,
      lastOpenedAt: 1_710_000_000
    )
  }

  private func makeThread(
    id: String,
    title: String,
    workspaceID: String,
    worktreeID: String? = nil,
    activeSessionID: String,
    kind: ThreadKind? = nil,
    createdAt: Int = 1_710_000_000,
    updatedAt: Int
  ) -> ThreadSummary {
    ThreadSummary(
      id: id,
      title: title,
      kind: kind ?? (workspaceID == "ws-a" || workspaceID == "ws-empty" ? .general : .coding),
      workspaceID: workspaceID,
      worktreeID: worktreeID,
      activeSessionID: activeSessionID,
      status: .idle,
      preview: "Preview for \(title)",
      model: "gpt-5.4",
      createdAt: createdAt,
      updatedAt: updatedAt
    )
  }

  private func makeSessionPayload(
    id: String,
    title: String,
    updatedAt: Int,
    archived: Bool,
    archivedAt: Int?
  ) -> String {
    let archivedAtValue = archivedAt.map(String.init) ?? "null"
    return """
      {
        "key": "\(id)",
        "kind": "main",
        "status": "idle",
        "label": null,
        "title": "\(title)",
        "preview": "Preview for \(title)",
        "model": "gpt-5.4",
        "created_at": \(updatedAt - 10),
        "updated_at": \(updatedAt),
        "message_count": 2,
        "archived": \(archived ? "true" : "false"),
        "archived_at": \(archivedAtValue)
      }
      """
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

  override class func canInit(with request: URLRequest) -> Bool {
    true
  }

  override class func canonicalRequest(for request: URLRequest) -> URLRequest {
    request
  }

  override func startLoading() {
    do {
      let (response, data) = try Self.store.response(for: request)
      client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
      client?.urlProtocol(self, didLoad: data)
      client?.urlProtocolDidFinishLoading(self)
    } catch {
      client?.urlProtocol(self, didFailWithError: error)
    }
  }

  override func stopLoading() {}

  static func setResponder(_ responder: @escaping MockAppStateURLProtocolStore.Responder) {
    store.setResponder(responder)
  }

  static func recordedRequests() -> [URLRequest] {
    store.recordedRequests()
  }

  static func reset() {
    store.reset()
  }
}

private final class ContextRaceURLProtocol: URLProtocol, @unchecked Sendable {
  fileprivate struct Configuration {
    let releaseFirstContext: DispatchSemaphore
    let firstContextStarted: @Sendable () -> Void
    let secondContextStarted: @Sendable () -> Void
  }

  private static let store = ContextRaceURLProtocolStore()

  override class func canInit(with request: URLRequest) -> Bool {
    true
  }

  override class func canonicalRequest(for request: URLRequest) -> URLRequest {
    request
  }

  override func startLoading() {
    guard let configuration = Self.store.currentConfiguration else {
      fail(with: MockAppStateProtocolError.missingResponder)
      return
    }

    switch request.url?.path {
    case "/v1/sessions/session-a/context":
      configuration.firstContextStarted()
      DispatchQueue.global(qos: .userInitiated).async { [self] in
        _ = configuration.releaseFirstContext.wait(timeout: .now() + 2)
        sendJSON(
          #"{"used_tokens":10,"max_tokens":100,"percentage":10,"compaction_threshold":80}"#
        )
      }
    case "/v1/sessions/session-b/context":
      configuration.secondContextStarted()
      sendJSON(
        #"{"used_tokens":80,"max_tokens":100,"percentage":80,"compaction_threshold":80}"#
      )
    default:
      sendJSON("{}", statusCode: 404)
    }
  }

  override func stopLoading() {}

  static func configure(
    releaseFirstContext: DispatchSemaphore,
    firstContextStarted: @escaping @Sendable () -> Void,
    secondContextStarted: @escaping @Sendable () -> Void
  ) {
    store.configure(
      Configuration(
        releaseFirstContext: releaseFirstContext,
        firstContextStarted: firstContextStarted,
        secondContextStarted: secondContextStarted
      )
    )
  }

  static func reset() {
    store.reset()
  }

  private func sendJSON(_ body: String, statusCode: Int = 200) {
    guard let url = request.url,
      let response = HTTPURLResponse(
        url: url,
        statusCode: statusCode,
        httpVersion: nil,
        headerFields: ["Content-Type": "application/json"]
      )
    else {
      fail(with: MockAppStateProtocolError.invalidResponse)
      return
    }

    client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
    client?.urlProtocol(self, didLoad: Data(body.utf8))
    client?.urlProtocolDidFinishLoading(self)
  }

  private func fail(with error: Error) {
    client?.urlProtocol(self, didFailWithError: error)
  }
}

private final class ContextRaceURLProtocolStore: @unchecked Sendable {
  private let lock = NSLock()
  private var configuration: ContextRaceURLProtocol.Configuration?

  // URLProtocol entry points are synchronous, so the test store stays lock-backed instead of actor-backed.
  var currentConfiguration: ContextRaceURLProtocol.Configuration? {
    lock.lock()
    defer { lock.unlock() }
    return configuration
  }

  func configure(_ configuration: ContextRaceURLProtocol.Configuration) {
    lock.lock()
    defer { lock.unlock() }
    self.configuration = configuration
  }

  func reset() {
    lock.lock()
    defer { lock.unlock() }
    configuration = nil
  }
}

private final class MockAppStateURLProtocolStore: @unchecked Sendable {
  typealias Responder = @Sendable (URLRequest) throws -> MockAppStateResponse

  private let lock = NSLock()

  private var responder: Responder?
  private var requests: [URLRequest] = []

  func setResponder(_ responder: @escaping Responder) {
    lock.lock()
    defer { lock.unlock() }
    self.responder = responder
    requests = []
  }

  func response(for request: URLRequest) throws -> (HTTPURLResponse, Data) {
    let configuredResponder: Responder

    lock.lock()
    requests.append(request)

    guard let responder else {
      lock.unlock()
      throw MockAppStateProtocolError.missingResponder
    }
    configuredResponder = responder
    lock.unlock()

    let response = try configuredResponder(request)
    guard let url = request.url else {
      throw MockAppStateProtocolError.missingURL
    }
    guard
      let httpResponse = HTTPURLResponse(
        url: url,
        statusCode: response.statusCode,
        httpVersion: nil,
        headerFields: ["Content-Type": "application/json"]
      )
    else {
      throw MockAppStateProtocolError.invalidResponse
    }

    return (httpResponse, response.body)
  }

  func recordedRequests() -> [URLRequest] {
    lock.lock()
    defer { lock.unlock() }
    return requests
  }

  func reset() {
    lock.lock()
    defer { lock.unlock() }
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
  case unreadableRequestBody
}

private final class LockedIntCounter: @unchecked Sendable {
  private let lock = NSLock()
  private var value = 0

  func incrementAndRead() -> Int {
    lock.lock()
    defer { lock.unlock() }
    value += 1
    return value
  }
}

private final class LockedBoolFlag: @unchecked Sendable {
  private let lock = NSLock()
  private var storedValue = false

  var value: Bool {
    lock.lock()
    defer { lock.unlock() }
    return storedValue
  }

  func setTrue() {
    lock.lock()
    defer { lock.unlock() }
    storedValue = true
  }
}

extension URLRequest {
  fileprivate func bodyDataForTesting() throws -> Data? {
    if let httpBody {
      return httpBody
    }

    guard let httpBodyStream else {
      return nil
    }

    return try Data(readingRequestBodyFrom: httpBodyStream)
  }
}

extension Data {
  fileprivate init(readingRequestBodyFrom stream: InputStream) throws {
    stream.open()
    defer { stream.close() }

    self.init()

    let bufferSize = 4096
    let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
    defer { buffer.deallocate() }

    while stream.hasBytesAvailable {
      let bytesRead = stream.read(buffer, maxLength: bufferSize)
      if bytesRead < 0 {
        throw stream.streamError ?? MockAppStateProtocolError.unreadableRequestBody
      }
      if bytesRead == 0 {
        break
      }

      append(buffer, count: bytesRead)
    }
  }
}
