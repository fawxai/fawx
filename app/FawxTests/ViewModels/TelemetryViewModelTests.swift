import XCTest
@testable import Fawx

@MainActor
final class TelemetryViewModelTests: XCTestCase {
    func testSetCategoryEnabledIgnoresSecondMutationWhileFirstIsPending() async throws {
        let started = expectation(description: "First telemetry mutation started")
        let patchState = TelemetryPatchState()
        let appState = try await makeConfiguredAppState()

        let response = makeTelemetryResponse(
            enabled: true,
            categories: [
                "errors": true,
                "performance": false,
            ]
        )

        let sut = TelemetryViewModel(
            appState: appState,
            fetchConsent: { response },
            patchConsent: { request in
                let requestCount = patchState.append(request)

                if requestCount == 1 {
                    started.fulfill()
                    return try await withCheckedThrowingContinuation { next in
                        patchState.store(next)
                    }
                }

                XCTFail("A second telemetry mutation should not start while the first is still pending.")
                return response
            }
        )

        sut.isEnabled = true
        sut.categories = [
            TelemetryCategory(name: "errors", enabled: false, description: "Error signal category"),
            TelemetryCategory(name: "performance", enabled: false, description: "Performance signal category"),
        ]

        let firstTask = Task {
            await sut.setCategoryEnabled("errors", enabled: true)
        }

        await fulfillment(of: [started], timeout: 1)

        let secondTask = Task {
            await sut.setCategoryEnabled("performance", enabled: true)
        }

        await Task.yield()

        XCTAssertEqual(patchState.requestCount, 1)
        XCTAssertEqual(sut.pendingCategories, Set(["errors"]))

        patchState.resume(returning: response)

        await firstTask.value
        await secondTask.value

        XCTAssertEqual(patchState.requestCount, 1)
        XCTAssertFalse(sut.pendingCategories.contains("performance"))
    }

    func testSetEnabledReturnsEarlyWhileCategoryMutationIsPending() async throws {
        let started = expectation(description: "Category mutation started")
        let patchState = TelemetryPatchState()
        let appState = try await makeConfiguredAppState()

        let response = makeTelemetryResponse(
            enabled: true,
            categories: [
                "errors": true,
                "performance": false,
            ]
        )

        let sut = TelemetryViewModel(
            appState: appState,
            fetchConsent: { response },
            patchConsent: { request in
                _ = patchState.append(request)
                started.fulfill()
                return try await withCheckedThrowingContinuation { next in
                    patchState.store(next)
                }
            }
        )

        sut.isEnabled = true
        sut.categories = [
            TelemetryCategory(name: "errors", enabled: false, description: "Error signal category"),
            TelemetryCategory(name: "performance", enabled: false, description: "Performance signal category"),
        ]

        let categoryTask = Task {
            await sut.setCategoryEnabled("errors", enabled: true)
        }

        await fulfillment(of: [started], timeout: 1)
        await sut.setEnabled(false)

        XCTAssertEqual(patchState.requestCount, 1)
        XCTAssertTrue(sut.isEnabled)

        patchState.resume(returning: response)
        await categoryTask.value
    }

    private func makeTelemetryResponse(
        enabled: Bool,
        categories: [String: Bool]
    ) -> TelemetryConsentResponse {
        TelemetryConsentResponse(
            enabled: enabled,
            categories: Dictionary(
                uniqueKeysWithValues: categories.map { entry in
                    (
                        entry.key,
                        TelemetryCategoryInfo(
                            enabled: entry.value,
                            description: "\(entry.key) description"
                        )
                    )
                }
            ),
            updatedAt: "2026-03-16T00:00:00Z"
        )
    }

    private func makeConfiguredAppState() async throws -> AppState {
        let defaultsSuiteName = "TelemetryViewModelTests.\(UUID().uuidString)"
        let keychainService = "ai.fawx.app.tests.\(UUID().uuidString)"
        let appState = AppState(
            persistence: AppStatePersistence(
                defaultsSuiteName: defaultsSuiteName,
                keychainService: keychainService,
                localInstallLoader: { nil }
            ),
            startLoadingPersistedState: false
        )

        try await appState.savePairing(
            serverURLString: "https://telemetry.example.com:8400",
            token: "telemetry-token",
            deviceName: "Telemetry Test Device",
            connectionMode: .remote
        )

        return appState
    }
}

private final class TelemetryPatchState: @unchecked Sendable {
    private let lock = NSLock()
    private var patchRequests: [TelemetryConsentPatchRequest] = []
    private var continuation: CheckedContinuation<TelemetryConsentResponse, Error>?

    func append(_ request: TelemetryConsentPatchRequest) -> Int {
        lock.lock()
        defer { lock.unlock() }
        patchRequests.append(request)
        return patchRequests.count
    }

    func store(_ continuation: CheckedContinuation<TelemetryConsentResponse, Error>) {
        lock.lock()
        defer { lock.unlock() }
        self.continuation = continuation
    }

    func resume(returning response: TelemetryConsentResponse) {
        lock.lock()
        let continuation = self.continuation
        self.continuation = nil
        lock.unlock()
        continuation?.resume(returning: response)
    }

    var requestCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return patchRequests.count
    }
}
