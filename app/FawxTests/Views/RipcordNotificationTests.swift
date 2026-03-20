import XCTest
@testable import Fawx

final class RipcordNotificationTests: XCTestCase {
    func testSnapshotReflectsTripwireDescriptionAndEnabledActions() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Review before approving",
            activatedAt: nil,
            entryCount: 3
        )

        let snapshot = makeSUT(status: status, isPerformingAction: false).snapshot

        XCTAssertEqual(snapshot.title, "Ripcord Active")
        XCTAssertEqual(snapshot.description, "Review before approving")
        XCTAssertEqual(snapshot.entryCountLabel, "3 actions journaled")
        XCTAssertFalse(snapshot.showsProgress)
        XCTAssertFalse(snapshot.areActionsDisabled)
        XCTAssertFalse(snapshot.isDismissDisabled)
        XCTAssertEqual(snapshot.maxWidth, FawxSpacing.ripcordNotificationMaxWidth)
    }

    func testSnapshotFallsBackToDefaultDescriptionWhileActionIsInFlight() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: nil,
            tripwireDescription: "   ",
            activatedAt: nil,
            entryCount: 1
        )

        let snapshot = makeSUT(status: status, isPerformingAction: true).snapshot

        XCTAssertEqual(snapshot.description, "Tripwire crossed")
        XCTAssertEqual(snapshot.entryCountLabel, "1 action journaled")
        XCTAssertTrue(snapshot.showsProgress)
        XCTAssertTrue(snapshot.areActionsDisabled)
        XCTAssertTrue(snapshot.isDismissDisabled)
    }

    private func makeSUT(
        status: RipcordStatusResponse,
        isPerformingAction: Bool
    ) -> RipcordNotification {
        RipcordNotification(
            status: status,
            isPerformingAction: isPerformingAction,
            reviewAction: {},
            pullAction: {},
            approveAction: {},
            dismissAction: {}
        )
    }
}
