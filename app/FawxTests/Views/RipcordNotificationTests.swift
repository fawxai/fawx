import XCTest
@testable import Fawx

final class RipcordNotificationTests: XCTestCase {
    func testSnapshotReflectsTripwireDescriptionAndCapabilityActions() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-1",
            tripwireDescription: "Review before approving",
            activatedAt: nil,
            entryCount: 3
        )

        let snapshot = makeNotificationSnapshot(
            status: status,
            isPerformingAction: false,
            resolutionActionKind: .dismiss
        )

        XCTAssertEqual(snapshot.title, "Ripcord Active")
        XCTAssertEqual(snapshot.description, "Review before approving")
        XCTAssertEqual(snapshot.entryCountLabel, "3 actions journaled")
        XCTAssertEqual(snapshot.resolutionActionKind, .dismiss)
        XCTAssertFalse(snapshot.showsProgress)
        XCTAssertFalse(snapshot.areActionsDisabled)
        XCTAssertEqual(snapshot.maxWidth, FawxSpacing.ripcordNotificationMaxWidth)
    }

    func testSnapshotUsesApproveActionInInteractiveMode() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-2",
            tripwireDescription: "Review before approving",
            activatedAt: nil,
            entryCount: 2
        )

        let snapshot = makeNotificationSnapshot(
            status: status,
            isPerformingAction: false,
            resolutionActionKind: .approve
        )

        XCTAssertEqual(snapshot.resolutionActionKind, .approve)
        XCTAssertFalse(snapshot.areActionsDisabled)
    }

    func testSnapshotFallsBackToDefaultDescriptionWhileActionIsInFlight() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: nil,
            tripwireDescription: "   ",
            activatedAt: nil,
            entryCount: 1
        )

        let snapshot = makeNotificationSnapshot(
            status: status,
            isPerformingAction: true,
            resolutionActionKind: .dismiss
        )

        XCTAssertEqual(snapshot.description, "Tripwire crossed")
        XCTAssertEqual(snapshot.entryCountLabel, "1 action journaled")
        XCTAssertTrue(snapshot.showsProgress)
        XCTAssertTrue(snapshot.areActionsDisabled)
    }

    func testResolutionActionKindUsesDismissInCapabilityMode() {
        XCTAssertEqual(RipcordResolutionActionKind.forPermissionMode(.capability), .dismiss)
    }

    func testResolutionActionKindUsesApproveInPromptMode() {
        XCTAssertEqual(RipcordResolutionActionKind.forPermissionMode(.prompt), .approve)
    }

    func testReviewTraySnapshotPreservesJournalState() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "tripwire-3",
            tripwireDescription: "Writes outside project directory",
            activatedAt: nil,
            entryCount: 4
        )
        let entry = JournalEntry(
            id: 1,
            timestamp: Date(timeIntervalSince1970: 1_710_000_000),
            toolName: "write_file",
            toolCallId: "call-1",
            action: JournalAction(type: "write_file", payload: .object([:])),
            reversible: true
        )

        let snapshot = RipcordReviewTraySnapshot(
            status: status,
            entries: [entry],
            isLoading: false,
            errorMessage: "recoverable",
            isPerformingAction: true,
            resolutionActionKind: .approve
        )

        XCTAssertEqual(snapshot.title, "Ripcord Review")
        XCTAssertEqual(snapshot.description, "Writes outside project directory")
        XCTAssertEqual(snapshot.entryCountLabel, "4 actions journaled")
        XCTAssertEqual(snapshot.entries, [entry])
        XCTAssertEqual(snapshot.errorMessage, "recoverable")
        XCTAssertTrue(snapshot.isPerformingAction)
        XCTAssertEqual(snapshot.resolutionActionKind, .approve)
        XCTAssertEqual(snapshot.maxWidth, FawxSpacing.ripcordReviewTrayMaxWidth)
    }

    private func makeNotificationSnapshot(
        status: RipcordStatusResponse,
        isPerformingAction: Bool,
        resolutionActionKind: RipcordResolutionActionKind
    ) -> RipcordNotificationSnapshot {
        RipcordNotificationSnapshot(
            status: status,
            isPerformingAction: isPerformingAction,
            resolutionActionKind: resolutionActionKind
        )
    }
}
