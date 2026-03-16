import XCTest
@testable import Fawx

@MainActor
final class TelemetryViewModelTests: XCTestCase {
    func testSetCategoryEnabledIgnoresSecondMutationWhileFirstIsPending() async {
        let started = expectation(description: "First telemetry mutation started")
        var continuation: CheckedContinuation<TelemetryConsentResponse, Error>?
        var patchRequests: [TelemetryConsentPatchRequest] = []

        let response = makeTelemetryResponse(
            enabled: true,
            categories: [
                "errors": true,
                "performance": false,
            ]
        )

        let sut = TelemetryViewModel(
            appState: AppState(),
            fetchConsent: { response },
            patchConsent: { request in
                patchRequests.append(request)

                if patchRequests.count == 1 {
                    started.fulfill()
                    return try await withCheckedThrowingContinuation { next in
                        continuation = next
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

        XCTAssertEqual(patchRequests.count, 1)
        XCTAssertEqual(sut.pendingCategories, Set(["errors"]))

        continuation?.resume(returning: response)

        await firstTask.value
        await secondTask.value

        XCTAssertEqual(patchRequests.count, 1)
        XCTAssertFalse(sut.pendingCategories.contains("performance"))
    }

    func testSetEnabledReturnsEarlyWhileCategoryMutationIsPending() async {
        let started = expectation(description: "Category mutation started")
        var continuation: CheckedContinuation<TelemetryConsentResponse, Error>?
        var patchRequests: [TelemetryConsentPatchRequest] = []

        let response = makeTelemetryResponse(
            enabled: true,
            categories: [
                "errors": true,
                "performance": false,
            ]
        )

        let sut = TelemetryViewModel(
            appState: AppState(),
            fetchConsent: { response },
            patchConsent: { request in
                patchRequests.append(request)
                started.fulfill()
                return try await withCheckedThrowingContinuation { next in
                    continuation = next
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

        XCTAssertEqual(patchRequests.count, 1)
        XCTAssertTrue(sut.isEnabled)

        continuation?.resume(returning: response)
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
}
