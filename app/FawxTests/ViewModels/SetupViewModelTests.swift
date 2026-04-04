import XCTest
@testable import Fawx

@MainActor
final class SetupViewModelTests: XCTestCase {
    func testPrepareCurrentStepBootstrapsServerBeforeRefreshingProviderState() async throws {
        let appState = makeAppState()
        var bootstrapCallCount = 0
        var refreshCallCount = 0

        let sut = SetupViewModel(
            appState: appState,
            refreshPhase4StateAction: {
                refreshCallCount += 1
                appState.setupStatus = Self.makeSetupStatus(providersConfigured: [])
            },
            providerBootstrapAction: { progress in
                bootstrapCallCount += 1
                progress("Starting Fawx server...")
                return BootstrapResult(
                    port: 8400,
                    host: "127.0.0.1",
                    bearerToken: "local-token",
                    dataDir: "/Users/test/.fawx",
                    configPath: "/Users/test/.fawx/config.toml",
                    created: true
                )
            }
        )
        sut.step = .provider
        sut.providerStatusKind = .failure
        sut.providerStatusMessage = "Old error"

        await sut.prepareCurrentStep()

        XCTAssertEqual(bootstrapCallCount, 1)
        XCTAssertEqual(refreshCallCount, 1)
        XCTAssertEqual(sut.providerStatusKind, .idle)
        XCTAssertNil(sut.providerStatusMessage)
        XCTAssertNil(sut.bootstrapProgress)
    }

    func testPrepareCurrentStepSkipsBootstrapWhenServerIsAlreadyConfigured() async throws {
        let appState = try await makeConfiguredAppState()
        var bootstrapCallCount = 0
        var refreshCallCount = 0

        let sut = SetupViewModel(
            appState: appState,
            completeLocalSetupAction: { _ in
                bootstrapCallCount += 1
            },
            refreshPhase4StateAction: {
                refreshCallCount += 1
                appState.setupStatus = Self.makeSetupStatus(providersConfigured: ["anthropic"])
            }
        )
        sut.step = .provider

        await sut.prepareCurrentStep()

        XCTAssertEqual(bootstrapCallCount, 0)
        XCTAssertEqual(refreshCallCount, 1)
        XCTAssertEqual(sut.providerStatusKind, .success)
        XCTAssertEqual(sut.providerStatusMessage, "Provider authentication is ready.")
    }

    func testPrepareCurrentStepShowsBootstrapFailureOnProviderStep() async {
        struct TestError: LocalizedError {
            var errorDescription: String? {
                "Network unavailable"
            }
        }

        let appState = makeAppState()
        var refreshCallCount = 0

        let sut = SetupViewModel(
            appState: appState,
            refreshPhase4StateAction: {
                refreshCallCount += 1
            },
            providerBootstrapAction: { _ in
                throw TestError()
            }
        )
        sut.step = .provider

        await sut.prepareCurrentStep()

        XCTAssertEqual(refreshCallCount, 0)
        XCTAssertEqual(sut.providerStatusKind, .failure)
        XCTAssertEqual(sut.providerStatusMessage, "Could not start the server: Network unavailable")
        XCTAssertNil(sut.bootstrapProgress)
    }

    func testSelectingFireworksForcesAPIKeyAuthMethod() {
        let sut = SetupViewModel(appState: makeAppState())

        sut.selectProvider(.fireworks)

        XCTAssertEqual(sut.selectedAuthMethod, .apiKey)
        XCTAssertEqual(sut.availableAuthMethods, [.apiKey])
        XCTAssertFalse(sut.supportsSubscriptionFlow)
        XCTAssertEqual(sut.providerActionTitle, "Save API Key")
        XCTAssertEqual(sut.providerFieldTitle, "API Key")
        XCTAssertEqual(sut.providerFieldPrompt, "Paste your Fireworks API key")
    }

    func testSelectingUnsupportedAuthMethodFallsBackToProviderDefault() {
        let sut = SetupViewModel(appState: makeAppState())
        sut.selectProvider(.fireworks)

        sut.selectAuthMethod(.subscription)

        XCTAssertEqual(sut.selectedAuthMethod, .apiKey)
    }

    private func makeAppState() -> AppState {
        AppState(
            persistence: AppStatePersistence(
                defaultsSuiteName: "SetupViewModelTests.\(UUID().uuidString)",
                keychainService: "ai.fawx.app.tests.\(UUID().uuidString)",
                localInstallLoader: { nil }
            ),
            startLoadingPersistedState: false
        )
    }

    private func makeConfiguredAppState() async throws -> AppState {
        let appState = makeAppState()
        try await appState.savePairing(
            serverURLString: "http://127.0.0.1:8400",
            token: "configured-token",
            deviceName: "Configured Test Mac",
            connectionMode: .local
        )
        return appState
    }

    private static func makeSetupStatus(providersConfigured: [String]) -> SetupStatusResponse {
        SetupStatusResponse(
            mode: "local",
            setupComplete: true,
            hasValidConfig: true,
            serverRunning: true,
            launchagent: SetupLaunchAgentStatus(
                installed: true,
                loaded: true,
                autoStartEnabled: true
            ),
            localServer: SetupLocalServerStatus(
                host: "127.0.0.1",
                port: 8400,
                httpsEnabled: false
            ),
            auth: SetupAuthStatus(
                bearerTokenPresent: true,
                providersConfigured: providersConfigured
            ),
            tailscale: SetupTailscaleStatus(
                installed: true,
                running: true,
                loggedIn: true,
                hostname: "setup-test",
                certReady: true
            )
        )
    }
}
