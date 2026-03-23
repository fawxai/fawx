import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import Fawx

@MainActor
final class ChatViewModelTests: XCTestCase {
    func testTranscriptScrollTrackerPublishesDistanceOnceMetricsBecomeValid() {
        let tracker = TranscriptScrollTracker()

        XCTAssertNil(tracker.update(viewportBottomY: 120))

        XCTAssertEqual(tracker.update(contentBottomY: 180), 60)
    }

    func testTranscriptScrollTrackerDeduplicatesRepeatedDistances() {
        let tracker = TranscriptScrollTracker()

        XCTAssertNil(tracker.update(viewportBottomY: 120))
        XCTAssertEqual(tracker.update(contentBottomY: 180), 60)
        XCTAssertNil(tracker.update(contentBottomY: 180))
        XCTAssertNil(tracker.update(viewportBottomY: 120))
    }

    func testTranscriptScrollTrackerIgnoresInvalidGeometryUpdates() {
        let tracker = TranscriptScrollTracker()

        XCTAssertNil(tracker.update(viewportBottomY: 120))
        XCTAssertEqual(tracker.update(contentBottomY: 180), 60)
        XCTAssertNil(tracker.update(contentBottomY: -10))

        XCTAssertEqual(tracker.update(contentBottomY: 200), 80)
    }

    func testTranscriptScrollTrackerResetClearsCachedDistance() {
        let tracker = TranscriptScrollTracker()

        XCTAssertNil(tracker.update(viewportBottomY: 120))
        XCTAssertEqual(tracker.update(contentBottomY: 180), 60)

        tracker.reset()

        XCTAssertNil(tracker.update(viewportBottomY: 120))
        XCTAssertEqual(tracker.update(contentBottomY: 180), 60)
    }

    func testTranscriptScrollCoordinatorDefaultsToBottomRestoreForNewSession() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")

        XCTAssertEqual(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: false
            ),
            .bottom
        )
        XCTAssertTrue(coordinator.shouldFollowLiveOutput)
    }

    func testTranscriptScrollCoordinatorDefersRestoreUntilHistoryFinishesLoading() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")

        XCTAssertNil(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: true
            )
        )

        XCTAssertEqual(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: false
            ),
            .bottom
        )
        XCTAssertTrue(coordinator.shouldFollowLiveOutput)
    }

    func testTranscriptScrollCoordinatorSeedPinnedStatePrimesDeduplication() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("test-session")

        let seed = coordinator.seedPinnedState(distanceFromBottom: 0)

        XCTAssertTrue(seed.isPinnedToBottom)
        XCTAssertNil(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 10, distanceFromBottom: 5),
                userDriven: false
            )
        )
    }

    func testTranscriptScrollInteractionTrackerDefaultsToNotUserDriven() {
        let tracker = TranscriptScrollInteractionTracker()

        XCTAssertFalse(tracker.isUserDrivenScroll(isPositionedByUser: false))
    }

    func testTranscriptScrollInteractionTrackerReportsUserDrivenWhenPositionedByUser() {
        let tracker = TranscriptScrollInteractionTracker()

        XCTAssertTrue(tracker.isUserDrivenScroll(isPositionedByUser: true))
    }

    @available(iOS 18.0, macOS 15.0, *)
    func testTranscriptScrollInteractionTrackerTracksInteractingPhase() {
        let tracker = TranscriptScrollInteractionTracker()

        tracker.updateScrollPhase(.tracking)
        XCTAssertTrue(tracker.isUserDrivenScroll(isPositionedByUser: false))

        tracker.updateScrollPhase(.idle)
        XCTAssertFalse(tracker.isUserDrivenScroll(isPositionedByUser: false))
    }

    func testTranscriptScrollCoordinatorPublishesPinnedTransitionsOnce() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")

        XCTAssertEqual(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 20, distanceFromBottom: 10),
                userDriven: false
            ),
            TranscriptPinnedStateUpdate(distanceFromBottom: 10, isPinnedToBottom: true)
        )
        XCTAssertNil(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 24, distanceFromBottom: 12),
                userDriven: false
            )
        )

        XCTAssertEqual(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 120, distanceFromBottom: 120),
                userDriven: true
            ),
            TranscriptPinnedStateUpdate(distanceFromBottom: 120, isPinnedToBottom: false)
        )
        XCTAssertNil(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 140, distanceFromBottom: 140),
                userDriven: true
            )
        )
    }

    @available(iOS 18.0, macOS 15.0, *)
    func testTranscriptScrollInteractionTrackerMarksInteractivePhasesAsUserDriven() {
        let tracker = TranscriptScrollInteractionTracker()

        tracker.updateScrollPhase(.tracking)

        XCTAssertTrue(tracker.isUserDrivenScroll(isPositionedByUser: false))
    }

    @available(iOS 18.0, macOS 15.0, *)
    func testTranscriptScrollInteractionTrackerClearsInteractionOnIdlePhase() {
        let tracker = TranscriptScrollInteractionTracker()

        tracker.updateScrollPhase(.tracking)
        tracker.updateScrollPhase(.idle)

        XCTAssertFalse(tracker.isUserDrivenScroll(isPositionedByUser: false))
    }

    @available(iOS 18.0, macOS 15.0, *)
    func testTranscriptScrollInteractionTrackerFallsBackToPositionedByUserWhenIdle() {
        let tracker = TranscriptScrollInteractionTracker()

        tracker.updateScrollPhase(.idle)

        XCTAssertTrue(tracker.isUserDrivenScroll(isPositionedByUser: true))
    }

    func testTranscriptScrollCoordinatorRestoresDetachedSessionAtSavedOffset() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")
        _ = coordinator.update(
            observation: TranscriptScrollObservation(contentOffsetY: 240, distanceFromBottom: 120),
            userDriven: true
        )

        coordinator.activateSession("session-b")
        coordinator.activateSession("session-a")

        XCTAssertEqual(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: false
            ),
            .point(240)
        )
        XCTAssertFalse(coordinator.shouldFollowLiveOutput)
    }

    func testTranscriptScrollCoordinatorRepinsNearBottom() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")
        _ = coordinator.update(
            observation: TranscriptScrollObservation(contentOffsetY: 200, distanceFromBottom: 120),
            userDriven: true
        )
        XCTAssertFalse(coordinator.shouldFollowLiveOutput)

        _ = coordinator.update(
            observation: TranscriptScrollObservation(contentOffsetY: 280, distanceFromBottom: 10),
            userDriven: true
        )

        XCTAssertTrue(coordinator.shouldFollowLiveOutput)
    }

    func testTranscriptScrollCoordinatorIgnoresProgrammaticDistanceGrowth() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")

        _ = coordinator.update(
            observation: TranscriptScrollObservation(contentOffsetY: 80, distanceFromBottom: 200),
            userDriven: false
        )

        XCTAssertTrue(coordinator.shouldFollowLiveOutput)
    }

    func testTranscriptScrollCoordinatorEvictsLeastRecentlyUsedSnapshots() {
        let coordinator = TranscriptScrollCoordinator()

        for index in 0..<64 {
            let sessionID = "session-\(index)"
            coordinator.activateSession(sessionID)
            _ = coordinator.update(
                observation: TranscriptScrollObservation(
                    contentOffsetY: CGFloat(index * 10),
                    distanceFromBottom: 120
                ),
                userDriven: true
            )
        }

        coordinator.activateSession("session-0")
        XCTAssertEqual(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: false
            ),
            .bottom
        )

        coordinator.activateSession("session-63")
        XCTAssertEqual(
            coordinator.restoreIntentIfNeeded(
                hasVisibleTranscriptContent: true,
                isLoadingHistory: false
            ),
            .point(630)
        )
        XCTAssertFalse(coordinator.shouldFollowLiveOutput)
    }

    func testStableDigestIsDeterministicForKnownInput() {
        XCTAssertEqual(ChatViewModel.stableDigest(for: "hello world"), "779a65e7023cd2e7")
        XCTAssertEqual(ChatViewModel.stableDigest(for: "hello world"), "779a65e7023cd2e7")
    }

    func testMakeTranscriptItemsProducesStableUniqueIDsForDuplicateMessages() {
        let sut = makeSUT()
        let duplicateA = SessionMessage(role: .assistant, content: "same", timestamp: 1)
        let duplicateB = SessionMessage(role: .assistant, content: "same", timestamp: 1)

        let firstPass = sut.makeTranscriptItems(from: [duplicateA, duplicateB])
        let secondPass = sut.makeTranscriptItems(from: [duplicateA, duplicateB])

        XCTAssertEqual(firstPass.map(\.id), secondPass.map(\.id))
        XCTAssertEqual(firstPass.map(\.id), [
            "message:assistant:1:97b5e18bf93ef5b",
            "message:assistant:1:97b5e18bf93ef5b#1",
        ])
    }

    func testAppendMessageUpdatesCacheAndVisibleTranscriptForCurrentSession() {
        let sut = makeSUT()
        let message = SessionMessage(role: .user, content: "hello", timestamp: 10)

        sut.prepareToDisplaySession("session-a")
        sut.appendMessageForTesting(message, sessionID: "session-a")

        XCTAssertEqual(sut.cachedMessages(for: "session-a"), [message])
        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage), [message])
    }

    func testPrepareToDisplaySessionUsesCachedMessagesImmediately() {
        let sut = makeSUT()
        let cachedMessage = SessionMessage(role: .assistant, content: "cached", timestamp: 42)

        sut.cacheMessages([cachedMessage], for: "session-a")
        sut.prepareToDisplaySession("session-a")

        XCTAssertFalse(sut.isLoadingHistory)
        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage), [cachedMessage])
    }

    func testApplyFetchedMessagesPreservesOptimisticLocalTailWhenResponseIsStale() {
        let sut = makeSUT()
        let history = SessionMessage(role: .assistant, content: "earlier", timestamp: 10)
        let localUserMessage = SessionMessage(role: .user, content: "send this", timestamp: 11)

        sut.cacheMessages([history, localUserMessage], for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting([history], sessionID: "session-a")

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.content),
            ["earlier", "send this"]
        )
        XCTAssertEqual(
            sut.transcriptItems.compactMap(\.sessionMessage).map(\.content),
            ["earlier", "send this"]
        )
    }

    func testApplyFetchedMessagesReplacesOptimisticTailWhenServerCatchesUp() {
        let sut = makeSUT()
        let history = SessionMessage(role: .assistant, content: "earlier", timestamp: 10)
        let localUserMessage = SessionMessage(role: .user, content: "send this", timestamp: 11)
        let fetchedUserMessage = SessionMessage(role: .user, content: "send this", timestamp: 22)
        let fetchedAssistantMessage = SessionMessage(role: .assistant, content: "done", timestamp: 23)

        sut.cacheMessages([history, localUserMessage], for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting(
            [history, fetchedUserMessage, fetchedAssistantMessage],
            sessionID: "session-a"
        )

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.timestamp),
            [10, 22, 23]
        )
        XCTAssertEqual(
            sut.transcriptItems.compactMap(\.sessionMessage).map(\.content),
            ["earlier", "send this", "done"]
        )
    }

    func testApplyFetchedMessagesPreservesOptimisticAssistantTailWhenToolHistoryArrivesFirst() {
        let sut = makeSUT()
        let localUserMessage = SessionMessage(role: .user, content: "Inspect the docs", timestamp: 11)
        let optimisticAssistantMessage = SessionMessage(
            role: .assistant,
            content: "Now I have a complete picture.",
            timestamp: 12
        )
        let fetchedUserMessage = SessionMessage(role: .user, content: "Inspect the docs", timestamp: 21)
        let fetchedAssistantToolMessage = SessionMessage(
            role: .assistant,
            contentBlocks: [
                .text("Let me check."),
                .toolUse(
                    id: "call_1",
                    name: "read_file",
                    input: .object(["path": .string("README.md")])
                ),
            ],
            timestamp: 22
        )
        let fetchedToolResultMessage = SessionMessage(
            role: .tool,
            contentBlocks: [
                .toolResult(toolUseId: "call_1", content: .string("docs"), isError: false)
            ],
            timestamp: 23
        )

        sut.cacheMessages([localUserMessage, optimisticAssistantMessage], for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting(
            [fetchedUserMessage, fetchedAssistantToolMessage, fetchedToolResultMessage],
            sessionID: "session-a"
        )

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.role),
            [.user, .assistant, .tool, .assistant]
        )
        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.last?.content,
            optimisticAssistantMessage.content
        )
        XCTAssertEqual(
            sut.transcriptItems.compactMap(\.sessionMessage).last?.content,
            optimisticAssistantMessage.content
        )

        let toolGroups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
            guard case .toolActivityGroup(let group) = item else {
                return nil
            }
            return group
        }

        XCTAssertEqual(toolGroups.count, 1)
        XCTAssertEqual(toolGroups[0].toolCalls[0].result, "docs")
    }

    func testApplyFetchedMessagesKeepsEarlierOptimisticAssistantBeforeLaterMatchedTurn() {
        let sut = makeSUT()
        let initialUserMessage = SessionMessage(role: .user, content: "Inspect the docs", timestamp: 11)
        let optimisticEarlierAssistant = SessionMessage(
            role: .assistant,
            content: "Here's the status on how the env vars flow.",
            timestamp: 12
        )
        let followUpUserMessage = SessionMessage(
            role: .user,
            content: "Create a full spec and submit a pull request.",
            timestamp: 13
        )
        let optimisticLatestAssistant = SessionMessage(
            role: .assistant,
            content: "Now I have a complete picture.",
            timestamp: 14
        )

        let fetchedInitialUserMessage = SessionMessage(
            role: .user,
            content: initialUserMessage.content,
            timestamp: 21
        )
        let fetchedFollowUpUserMessage = SessionMessage(
            role: .user,
            content: followUpUserMessage.content,
            timestamp: 22
        )
        let fetchedLatestAssistant = SessionMessage(
            role: .assistant,
            content: optimisticLatestAssistant.content,
            timestamp: 23
        )

        sut.cacheMessages(
            [
                initialUserMessage,
                optimisticEarlierAssistant,
                followUpUserMessage,
                optimisticLatestAssistant,
            ],
            for: "session-a"
        )
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting(
            [
                fetchedInitialUserMessage,
                fetchedFollowUpUserMessage,
                fetchedLatestAssistant,
            ],
            sessionID: "session-a"
        )

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.content),
            [
                initialUserMessage.content,
                optimisticEarlierAssistant.content,
                followUpUserMessage.content,
                optimisticLatestAssistant.content,
            ]
        )
        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.timestamp),
            [21, 12, 22, 23]
        )
        XCTAssertEqual(
            sut.transcriptItems.compactMap(\.sessionMessage).map(\.content),
            [
                initialUserMessage.content,
                optimisticEarlierAssistant.content,
                followUpUserMessage.content,
                optimisticLatestAssistant.content,
            ]
        )
    }

    func testPrepareToDisplaySessionShowsLoadingStateWhenCacheIsMissing() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-missing")

        XCTAssertTrue(sut.isLoadingHistory)
        XCTAssertTrue(sut.transcriptItems.isEmpty)
    }

    func testMakeTranscriptItemsGroupsStructuredToolHistoryIntoSingleItem() {
        let sut = makeSUT()
        let assistantToolMessage = SessionMessage(
            role: .assistant,
            contentBlocks: [
                .text("Let me check."),
                .toolUse(
                    id: "call_1",
                    name: "read_file",
                    input: .object(["path": .string("README.md")])
                ),
            ],
            timestamp: 2
        )
        let toolResultMessage = SessionMessage(
            role: .tool,
            contentBlocks: [
                .toolResult(toolUseId: "call_1", content: .string("file contents"), isError: false)
            ],
            timestamp: 3
        )
        let assistantReply = SessionMessage(role: .assistant, content: "Done.", timestamp: 4)

        let items = sut.makeTranscriptItems(from: [assistantToolMessage, toolResultMessage, assistantReply])

        XCTAssertEqual(items.count, 3)

        guard case .message(let leadingMessage) = items[0] else {
            return XCTFail("Expected first transcript item to be a message")
        }
        XCTAssertEqual(leadingMessage.displayText, "Let me check.")

        guard case .toolActivityGroup(let group) = items[1] else {
            return XCTFail("Expected second transcript item to be grouped tool activity")
        }
        XCTAssertEqual(group.toolCount, 1)
        XCTAssertEqual(group.toolCalls[0].name, "read_file")
        XCTAssertTrue(group.toolCalls[0].arguments.contains("README.md"))
        XCTAssertEqual(group.toolCalls[0].result, "file contents")
        XCTAssertFalse(group.toolCalls[0].isError)

        guard case .message(let trailingMessage) = items[2] else {
            return XCTFail("Expected third transcript item to be a message")
        }
        XCTAssertEqual(trailingMessage.displayText, "Done.")
    }

    func testHiddenSessionToolActivityRemainsVisibleWhenReturning() {
        let sut = makeSUT()
        let cachedMessage = SessionMessage(role: .assistant, content: "Queued work", timestamp: 1)

        sut.cacheMessages([cachedMessage], for: "session-a")
        sut.setStreamingSessionsForTesting(
            ["session-a": (text: "", phase: .act)],
            currentSessionID: "session-b"
        )
        sut.prepareToDisplaySession("session-b")

        sut.beginToolCallForTesting(sessionID: "session-a", id: "call_1", name: "read_file")
        sut.completeToolCallForTesting(
            sessionID: "session-a",
            id: "call_1",
            name: "read_file",
            arguments: "{\"path\":\"README.md\"}"
        )
        sut.finishToolCallForTesting(
            sessionID: "session-a",
            id: "call_1",
            output: "file contents",
            isError: false
        )

        sut.prepareToDisplaySession("session-a")

        let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
            guard case .toolActivityGroup(let group) = item else {
                return nil
            }
            return group
        }

        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage), [cachedMessage])
        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].toolCalls[0].name, "read_file")
        XCTAssertEqual(groups[0].toolCalls[0].result, "file contents")
    }

    func testLiveToolGroupPreservesCompletedToolsAcrossMultipleRoundsInOneTurn() {
        let sut = makeSUT()

        sut.setStreamingSessionsForTesting(
            ["session-a": (text: "", phase: .act)],
            currentSessionID: "session-a"
        )
        sut.prepareToDisplaySession("session-a")

        sut.beginToolCallForTesting(sessionID: "session-a", id: "call_1", name: "read_file")
        sut.finishToolCallForTesting(
            sessionID: "session-a",
            id: "call_1",
            output: "first result",
            isError: false
        )

        sut.beginToolCallForTesting(sessionID: "session-a", id: "call_2", name: "list_dir")

        var groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
            guard case .toolActivityGroup(let group) = item else {
                return nil
            }
            return group
        }

        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].toolCalls.map(\.id), ["call_1", "call_2"])
        XCTAssertEqual(groups[0].toolCalls[0].result, "first result")
        XCTAssertTrue(groups[0].toolCalls[1].isRunning)

        sut.finishToolCallForTesting(
            sessionID: "session-a",
            id: "call_2",
            output: "second result",
            isError: false
        )

        groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
            guard case .toolActivityGroup(let group) = item else {
                return nil
            }
            return group
        }

        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].toolCalls.map(\.result), ["first result", "second result"])
    }

    func testLiveToolActivityIncrementsTranscriptUpdateIDEvenWhenLastItemIDIsStable() {
        let sut = makeSUT()

        sut.setStreamingSessionsForTesting(
            ["session-a": (text: "", phase: .act)],
            currentSessionID: "session-a"
        )
        sut.prepareToDisplaySession("session-a")
        sut.beginToolCallForTesting(sessionID: "session-a", id: "call_1", name: "read_file")

        let lastItemID = sut.transcriptItems.last?.id
        let transcriptUpdateID = sut.transcriptUpdateID

        sut.completeToolCallForTesting(
            sessionID: "session-a",
            id: "call_1",
            name: "read_file",
            arguments: "{\"path\":\"README.md\"}"
        )

        XCTAssertEqual(sut.transcriptItems.last?.id, lastItemID)
        XCTAssertGreaterThan(sut.transcriptUpdateID, transcriptUpdateID)
    }

    func testLiveToolActivitySnapshotUsesStatusOnlyDetails() {
        let group = ToolActivityGroupRecord(
            id: "live-session-a",
            toolCalls: [
                ToolCallRecord(
                    id: "call_1",
                    name: "read_file",
                    arguments: "{\"path\":\"README.md\"}",
                    result: "contents",
                    isRunning: true,
                    isError: false
                )
            ],
            isLive: true
        )

        let snapshot = ToolActivityGroupCardSnapshot(group: group, isExpanded: true)

        XCTAssertEqual(snapshot.detailStyle, .liveStatusOnly)
        XCTAssertFalse(snapshot.showsPayloadDetails)
        XCTAssertEqual(snapshot.visibleToolCalls.map(\.id), ["call_1"])
        XCTAssertEqual(
            snapshot.accessibilityHint,
            "Collapse tool activity. Detailed arguments and output appear after the response finishes."
        )
    }

    func testToolActivitySnapshotDefaultsToCollapsedState() {
        let group = ToolActivityGroupRecord(
            id: "live-session-a",
            toolCalls: [
                ToolCallRecord(
                    id: "call_1",
                    name: "read_file",
                    arguments: "{\"path\":\"README.md\"}",
                    result: "contents",
                    isRunning: true,
                    isError: false
                )
            ],
            isLive: true
        )

        let snapshot = ToolActivityGroupCardSnapshot(group: group, isExpanded: false)

        XCTAssertEqual(snapshot.detailStyle, .collapsed)
        XCTAssertFalse(snapshot.isExpanded)
        XCTAssertEqual(snapshot.headerTitle, "read_file")
        XCTAssertEqual(snapshot.visibleToolCalls, [])
        XCTAssertEqual(snapshot.accessibilityHint, "Expand tool activity")
    }

    func testHistoricalToolActivitySnapshotUsesPayloadDetails() {
        let group = ToolActivityGroupRecord(
            id: "history-session-a",
            toolCalls: [
                ToolCallRecord(
                    id: "call_1",
                    name: "read_file",
                    arguments: "{\"path\":\"README.md\"}",
                    result: "contents",
                    isRunning: false,
                    isError: false
                )
            ],
            isLive: false
        )

        let snapshot = ToolActivityGroupCardSnapshot(group: group, isExpanded: true)

        XCTAssertEqual(snapshot.detailStyle, .historicalPayload)
        XCTAssertTrue(snapshot.showsPayloadDetails)
        XCTAssertEqual(snapshot.visibleToolCalls.map(\.arguments), ["{\"path\":\"README.md\"}"])
        XCTAssertEqual(snapshot.accessibilityHint, "Collapse tool activity")
    }

    func testFetchedHistoryReplacesMatchingLiveToolOverlayInsteadOfDuplicatingIt() {
        let sut = makeSUT()
        let optimisticAssistant = SessionMessage(role: .assistant, content: "Let me check.", timestamp: 1)
        let historicalAssistant = SessionMessage(
            role: .assistant,
            contentBlocks: [
                .text("Let me check."),
                .toolUse(
                    id: "call_1",
                    name: "read_file",
                    input: .object(["path": .string("README.md")])
                ),
            ],
            timestamp: 2
        )
        let historicalToolResult = SessionMessage(
            role: .tool,
            contentBlocks: [
                .toolResult(toolUseId: "call_1", content: .string("file contents"), isError: false)
            ],
            timestamp: 3
        )

        sut.cacheMessages([optimisticAssistant], for: "session-a")
        sut.prepareToDisplaySession("session-a")
        sut.beginToolCallForTesting(sessionID: "session-a", id: "call_1", name: "read_file")
        sut.finishToolCallForTesting(
            sessionID: "session-a",
            id: "call_1",
            output: "file contents",
            isError: false
        )

        sut.applyFetchedMessagesForTesting([historicalAssistant, historicalToolResult], sessionID: "session-a")

        let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
            guard case .toolActivityGroup(let group) = item else {
                return nil
            }
            return group
        }

        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups[0].toolCalls[0].id, "call_1")
        XCTAssertEqual(groups[0].toolCalls[0].result, "file contents")
    }

    func testDraftMessageIsScopedPerSession() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-a")
        sut.draftMessage = "draft-a"

        sut.prepareToDisplaySession("session-b")
        XCTAssertEqual(sut.draftMessage, "")

        sut.draftMessage = "draft-b"

        sut.prepareToDisplaySession("session-a")
        XCTAssertEqual(sut.draftMessage, "draft-a")

        sut.prepareToDisplaySession("session-b")
        XCTAssertEqual(sut.draftMessage, "draft-b")
    }

    func testDraftMessagePersistsForNilSessionSelection() {
        let sut = makeSUT()

        sut.prepareToDisplaySession(nil)
        sut.draftMessage = "new-session-draft"

        sut.prepareToDisplaySession("session-a")
        XCTAssertEqual(sut.draftMessage, "")

        sut.prepareToDisplaySession(nil)
        XCTAssertEqual(sut.draftMessage, "new-session-draft")
    }

    func testHandleStreamErrorOnlySurfacesForVisibleSession() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-a")
        sut.handleStreamErrorForTesting("hidden", sessionID: "session-b")
        XCTAssertNil(sut.errorMessage)

        sut.handleStreamErrorForTesting("visible", sessionID: "session-a")
        XCTAssertEqual(sut.errorMessage, "visible")
    }

    func testErrorMessagesAreScopedPerSession() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-a")
        sut.handleStreamErrorForTesting("error-a", sessionID: "session-a")
        XCTAssertEqual(sut.errorMessage, "error-a")

        sut.prepareToDisplaySession("session-b")
        XCTAssertNil(sut.errorMessage)

        sut.handleStreamErrorForTesting("error-b", sessionID: "session-b")
        XCTAssertEqual(sut.errorMessage, "error-b")

        sut.prepareToDisplaySession("session-a")
        XCTAssertEqual(sut.errorMessage, "error-a")
    }

    func testClearingOneSessionErrorPreservesOtherSessionErrors() {
        let sut = makeSUT()

        sut.handleStreamErrorForTesting("error-a", sessionID: "session-a")
        sut.handleStreamErrorForTesting("error-b", sessionID: "session-b")

        sut.clearErrorMessageForTesting(sessionID: "session-a")

        sut.prepareToDisplaySession("session-a")
        XCTAssertNil(sut.errorMessage)

        sut.prepareToDisplaySession("session-b")
        XCTAssertEqual(sut.errorMessage, "error-b")
    }

    func testStreamingComputedPropertiesTrackVisibleAndBackgroundSessions() {
        let sut = makeSUT()

        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a",
            streamingText: "partial",
            phase: .reason
        )

        XCTAssertEqual(sut.activeStreamSessionID, "session-a")
        XCTAssertTrue(sut.isCurrentSessionStreaming)
        XCTAssertFalse(sut.isStreamingInAnotherSession)
        XCTAssertEqual(sut.visibleStreamingText, "partial")
        XCTAssertEqual(sut.composerPhaseLabel, "Reason")

        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-b",
            streamingSessionID: "session-a",
            streamingText: "partial",
            phase: .reason
        )

        XCTAssertFalse(sut.isCurrentSessionStreaming)
        XCTAssertTrue(sut.isStreamingInAnotherSession)
        XCTAssertEqual(sut.visibleStreamingText, "")
        XCTAssertEqual(sut.composerPhaseLabel, "Streaming in another session...")
    }

    func testMultipleConcurrentStreamingSessionsAreTrackedIndependently() {
        let sut = makeSUT()

        sut.setStreamingSessionsForTesting(
            [
                "session-a": (text: "alpha", phase: .reason),
                "session-b": (text: "beta", phase: .act),
            ],
            currentSessionID: "session-b"
        )

        XCTAssertEqual(sut.activeStreamSessionIDs, Set(["session-a", "session-b"]))
        XCTAssertTrue(sut.isCurrentSessionStreaming)
        XCTAssertTrue(sut.isStreamingInAnotherSession)
        XCTAssertEqual(sut.visibleStreamingText, "beta")
        XCTAssertEqual(sut.visibleCurrentPhase, .act)
    }

    func testContextCompactedUpdatesVisibleSessionContextAndBanner() {
        let sut = makeSUT()
        sut.prepareToDisplaySession("session-a")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 68,
                maxTokens: 100,
                percentage: 68,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(
            sessionID: "session-a",
            tier: "slide",
            messagesRemoved: 12,
            tokensBefore: 68,
            tokensAfter: 42,
            usageRatio: 0.42
        )

        XCTAssertEqual(sut.currentContextForTesting?.usedTokens, 42)
        XCTAssertEqual(sut.currentContextForTesting?.maxTokens, 100)
        XCTAssertEqual(sut.currentContextForTesting?.normalizedPercentage, 42)
        XCTAssertEqual(
            sut.compactionBannerInfo,
            ChatViewModel.CompactionBannerInfo(
                message: "Context optimized: 12 messages compacted, 68% → 42%",
                isEmergency: false
            )
        )
    }

    func testContextCompactedIgnoresBackgroundSession() {
        let sut = makeSUT()
        sut.prepareToDisplaySession("session-b")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 55,
                maxTokens: 100,
                percentage: 55,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(
            sessionID: "session-a",
            tier: "slide",
            messagesRemoved: 12,
            tokensBefore: 68,
            tokensAfter: 42,
            usageRatio: 0.42
        )

        XCTAssertEqual(sut.currentContextForTesting?.usedTokens, 55)
        XCTAssertNil(sut.compactionBannerInfo)
    }

    func testContextCompactedDerivesMissingBudgetFromUsageRatio() {
        let sut = makeSUT()
        sut.prepareToDisplaySession("session-a")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 68,
                maxTokens: 0,
                percentage: 0,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(
            sessionID: "session-a",
            tier: "slide",
            messagesRemoved: 12,
            tokensBefore: 68,
            tokensAfter: 42,
            usageRatio: 0.42
        )

        XCTAssertEqual(sut.currentContextForTesting?.maxTokens, 100)
        XCTAssertEqual(sut.currentContextForTesting?.normalizedPercentage, 42)
        XCTAssertEqual(
            sut.compactionBannerInfo,
            ChatViewModel.CompactionBannerInfo(
                message: "Context optimized: 12 messages compacted, 68% → 42%",
                isEmergency: false
            )
        )
    }

    func testContextCompactedUsesSingularBannerCopy() {
        let sut = makeSUT()
        sut.prepareToDisplaySession("session-a")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 68,
                maxTokens: 100,
                percentage: 68,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(
            sessionID: "session-a",
            tier: "slide",
            messagesRemoved: 1,
            tokensBefore: 68,
            tokensAfter: 42,
            usageRatio: 0.42
        )

        XCTAssertEqual(
            sut.compactionBannerInfo,
            ChatViewModel.CompactionBannerInfo(
                message: "Context optimized: 1 message compacted, 68% → 42%",
                isEmergency: false
            )
        )
    }

    func testContextCompactedUsesEmergencyBannerCopy() {
        let sut = makeSUT()
        sut.prepareToDisplaySession("session-a")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 92,
                maxTokens: 100,
                percentage: 92,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(
            sessionID: "session-a",
            tier: "emergency",
            messagesRemoved: 12,
            tokensBefore: 92,
            tokensAfter: 48,
            usageRatio: 0.48
        )

        XCTAssertEqual(
            sut.compactionBannerInfo,
            ChatViewModel.CompactionBannerInfo(
                message: "Context urgently optimized: 12 messages compacted, 92% → 48%",
                isEmergency: true
            )
        )
    }

    func testCompactionBannerAutoDismissesAfterTimeout() async {
        let sleeper = ControlledSleeper()
        let sut = makeSUT { duration in
            try await sleeper.sleep(for: duration)
        }

        sut.prepareToDisplaySession("session-a")
        sut.setCurrentContextForTesting(
            ContextInfo(
                usedTokens: 68,
                maxTokens: 100,
                percentage: 68,
                compactionThreshold: 80
            )
        )

        sut.handleContextCompactedForTesting(sessionID: "session-a")

        XCTAssertNotNil(sut.compactionBannerInfo)
        await waitForSleepToBeScheduled(on: sleeper)
        await sleeper.resumeNextSleep()
        await waitForCompactionBannerToDismiss(on: sut)

        XCTAssertNil(sut.compactionBannerInfo)
    }

    func testStreamingDisplayControllerCoalescesRapidTokensIntoOneFlush() async {
        var flushedChunks: [String] = []
        let sleeper = ControlledSleeper()
        let controller = StreamingDisplayController(
            flushInterval: .milliseconds(30),
            sleepHandler: { duration in
                try await sleeper.sleep(for: duration)
            }
        ) { chunk in
            flushedChunks.append(chunk)
        }

        for _ in 0..<100 {
            controller.appendToken("a")
        }

        await waitForSleepToBeScheduled(on: sleeper)
        XCTAssertTrue(flushedChunks.isEmpty)
        let pendingSleepCount = await sleeper.pendingSleepCount
        XCTAssertEqual(pendingSleepCount, 1)

        await sleeper.resumeNextSleep()
        await Task.yield()

        XCTAssertEqual(flushedChunks, [String(repeating: "a", count: 100)])
    }

    func testStreamingDistanceDetachesAndRepinsAutoScroll() {
        let sut = makeSUT()

        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )

        XCTAssertTrue(sut.shouldAutoScrollStreamingUpdates)

        sut.updateStreamingDistanceFromBottomForTesting(StreamingDisplayController.bottomThreshold + 20)

        XCTAssertFalse(sut.isPinnedToBottomForTesting)
        XCTAssertFalse(sut.shouldAutoScrollStreamingUpdates)

        sut.updateStreamingDistanceFromBottomForTesting(StreamingDisplayController.bottomThreshold - 5)

        XCTAssertTrue(sut.isPinnedToBottomForTesting)
        XCTAssertTrue(sut.shouldAutoScrollStreamingUpdates)
    }

    func testStreamEndFlushesBufferedTokensImmediately() async throws {
        let sut = makeSUT()

        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )

        sut.appendStreamingTokenForTesting("tail")

        XCTAssertEqual(sut.streamingTextForTesting(sessionID: "session-a"), "")

        sut.flushStreamingDisplayForTesting()

        XCTAssertEqual(sut.streamingTextForTesting(sessionID: "session-a"), "tail")

        try await Task.sleep(for: .milliseconds(80))

        XCTAssertEqual(sut.streamingTextForTesting(sessionID: "session-a"), "tail")
    }

    func testStreamingDisplayControllerPreservesDetachedStateWhenStreamEnds() {
        let controller = StreamingDisplayController { _ in }

        controller.userDidScroll(distanceFromBottom: StreamingDisplayController.bottomThreshold + 20)
        controller.reset(repinToBottom: false)

        XCTAssertFalse(controller.isPinnedToBottom)

        controller.reset(repinToBottom: true)

        XCTAssertTrue(controller.isPinnedToBottom)
    }

    func testStreamingDisplayControllerAllowsLargeScrollAwayWhilePendingAutoScroll() {
        let controller = StreamingDisplayController { _ in }

        controller.appendToken("tail")
        controller.streamDidEnd()
        controller.userDidScroll(distanceFromBottom: StreamingDisplayController.bottomThreshold * 3)

        XCTAssertFalse(controller.isPinnedToBottom)
    }

    func testStreamingDisplayControllerKeepsPinnedStateForSmallContentGrowthJump() {
        let controller = StreamingDisplayController { _ in }

        controller.appendToken("tail")
        controller.streamDidEnd()
        controller.userDidScroll(distanceFromBottom: StreamingDisplayController.bottomThreshold + 5)

        XCTAssertTrue(controller.isPinnedToBottom)
    }

    func testStreamingDisplayControllerIgnoresProgrammaticScrollDistanceGrowth() {
        let controller = StreamingDisplayController { _ in }

        controller.userDidScroll(
            distanceFromBottom: StreamingDisplayController.bottomThreshold * 3,
            isUserInitiated: false
        )

        XCTAssertTrue(controller.isPinnedToBottom)
    }

    func testAppendingVisibleMessageWhileDetachedMidStreamPreservesScrollPosition() {
        let sut = makeSUT()
        let assistantMessage = SessionMessage(role: .assistant, content: "done", timestamp: 2)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a",
            streamingText: "partial",
            phase: .reason
        )
        sut.updateStreamingDistanceFromBottomForTesting(StreamingDisplayController.bottomThreshold + 20)

        sut.appendMessageForTesting(assistantMessage, sessionID: "session-a")

        XCTAssertEqual(sut.pendingTranscriptScrollBehavior, .preservePosition)
    }

    func testAppendingVisibleMessageWhilePinnedMidStreamScrollsAnimated() {
        let sut = makeSUT()
        let assistantMessage = SessionMessage(role: .assistant, content: "done", timestamp: 2)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a",
            streamingText: "partial",
            phase: .reason
        )

        sut.appendMessageForTesting(assistantMessage, sessionID: "session-a")

        XCTAssertEqual(sut.pendingTranscriptScrollBehavior, .animated)
    }

    func testAppendingVisibleMessageWhileDetachedNotStreamingScrollsAnimated() {
        let sut = makeSUT()
        let assistantMessage = SessionMessage(role: .assistant, content: "done", timestamp: 2)

        sut.prepareToDisplaySession("session-a")
        sut.updateStreamingDistanceFromBottomForTesting(StreamingDisplayController.bottomThreshold + 20)

        sut.appendMessageForTesting(assistantMessage, sessionID: "session-a")

        XCTAssertEqual(sut.pendingTranscriptScrollBehavior, .animated)
    }

    func testAppendingVisibleMessageWhileDetachedInDifferentStreamingSessionScrollsAnimated() {
        let sut = makeSUT()
        let assistantMessage = SessionMessage(role: .assistant, content: "done", timestamp: 2)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-b",
            streamingText: "partial",
            phase: .reason
        )
        sut.updateStreamingDistanceFromBottomForTesting(StreamingDisplayController.bottomThreshold + 20)

        sut.appendMessageForTesting(assistantMessage, sessionID: "session-a")

        XCTAssertEqual(sut.pendingTranscriptScrollBehavior, .animated)
    }

    func testShowEmptyStatePreservesBackgroundStream() {
        let sut = makeSUT()
        let message = SessionMessage(role: .assistant, content: "visible", timestamp: 1)

        sut.prepareToDisplaySession("session-a")
        sut.appendMessageForTesting(message, sessionID: "session-a")
        sut.handleStreamErrorForTesting("visible error", sessionID: "session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a",
            streamingText: "partial",
            phase: .reason
        )

        sut.showEmptyState()

        XCTAssertTrue(sut.isStreaming)
        XCTAssertEqual(sut.activeStreamSessionID, "session-a")
        XCTAssertFalse(sut.isCurrentSessionStreaming)
        XCTAssertFalse(sut.isStreamingInAnotherSession)
        XCTAssertTrue(sut.transcriptItems.isEmpty)
        XCTAssertNil(sut.errorMessage)
    }

    func testShowEmptyStateCleansUpWhenNotStreaming() {
        let sut = makeSUT()
        let visibleMessage = SessionMessage(role: .assistant, content: "visible", timestamp: 1)
        let hiddenMessage = SessionMessage(role: .assistant, content: "hidden", timestamp: 2)

        sut.prepareToDisplaySession("session-a")
        sut.appendMessageForTesting(visibleMessage, sessionID: "session-a")
        sut.handleStreamErrorForTesting("visible error", sessionID: "session-a")

        sut.showEmptyState()

        XCTAssertFalse(sut.isStreaming)
        XCTAssertNil(sut.activeStreamSessionID)
        XCTAssertTrue(sut.transcriptItems.isEmpty)
        XCTAssertNil(sut.errorMessage)

        sut.appendMessageForTesting(hiddenMessage, sessionID: "session-a")

        XCTAssertTrue(sut.transcriptItems.isEmpty)
    }

    func testQueuedMessageIsOnlyVisibleForItsOriginatingSession() {
        let sut = makeSUT(connectionStatus: .connected)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.draftMessage = "follow up"

        sut.sendDraft()

        XCTAssertEqual(sut.draftMessage, "")
        XCTAssertEqual(sut.queuedMessage, "follow up")

        sut.prepareToDisplaySession("session-b")
        XCTAssertNil(sut.queuedMessage)

        sut.prepareToDisplaySession("session-a")
        XCTAssertEqual(sut.queuedMessage, "follow up")
    }

    func testQueuedMessageDeliveryTargetsFinishedStreamingSession() {
        let sut = makeSUT(connectionStatus: .connected)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.draftMessage = "follow up"
        sut.sendDraft()

        sut.prepareToDisplaySession("session-b")

        let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

        XCTAssertEqual(delivery?.text, "follow up")
        XCTAssertEqual(delivery?.sessionID, "session-a")

        sut.prepareToDisplaySession("session-a")
        XCTAssertNil(sut.queuedMessage)
    }

    func testQueuedMessageDeliveryKeepsQueuedMessageWhenDifferentSessionFinishes() {
        let sut = makeSUT(connectionStatus: .connected)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.draftMessage = "follow up"
        sut.sendDraft()

        let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-c")

        XCTAssertNil(delivery)
        XCTAssertEqual(sut.queuedMessage, "follow up")

        sut.prepareToDisplaySession("session-c")
        XCTAssertNil(sut.queuedMessage)

        sut.prepareToDisplaySession("session-a")
        XCTAssertEqual(sut.queuedMessage, "follow up")
    }

    func testPrepareMessageInjectsTextAttachmentsAndSeparatesBinaryPayloads() {
        let imageData = Data([0x89, 0x50, 0x4E, 0x47])
        let pdfData = Data("%PDF".utf8)
        let textAttachment = makePendingAttachment(
            kind: .textFile,
            filename: "notes.txt",
            data: Data("alpha\nbeta".utf8),
            mediaType: "text/plain",
            textContent: "alpha\nbeta"
        )
        let imageAttachment = makePendingAttachment(
            kind: .image,
            filename: "photo.png",
            data: imageData,
            mediaType: "image/png"
        )
        let documentAttachment = makePendingAttachment(
            kind: .pdf,
            filename: "brief.pdf",
            data: pdfData,
            mediaType: "application/pdf"
        )

        let payload = AttachmentComposer.prepareMessage(
            message: "Please review",
            attachments: [textAttachment, imageAttachment, documentAttachment]
        )

        let expectedMessage = """
        [file: notes.txt]
        alpha
        beta
        [/file: notes.txt]

        Please review
        """

        XCTAssertEqual(payload.message, expectedMessage)
        XCTAssertEqual(
            payload.images,
            [ImagePayload(data: imageData.base64EncodedString(), mediaType: "image/png")]
        )
        XCTAssertEqual(
            payload.documents,
            [
                DocumentPayload(
                    data: pdfData.base64EncodedString(),
                    mediaType: "application/pdf",
                    filename: "brief.pdf"
                )
            ]
        )
        XCTAssertEqual(
            payload.contentBlocks,
            [
                .image(mediaType: "image/png", data: imageData.base64EncodedString()),
                .document(
                    mediaType: "application/pdf",
                    data: pdfData.base64EncodedString(),
                    filename: "brief.pdf"
                ),
                .text(expectedMessage),
            ]
        )
    }

    func testMaxAttachmentLimitRejectsEleventhAttachment() {
        let existingAttachments = (0..<AttachmentComposer.maxAttachmentCount).map { index in
            makePendingAttachment(
                kind: .textFile,
                filename: "file-\(index).txt",
                data: Data(),
                mediaType: "text/plain",
                textContent: ""
            )
        }
        let extraAttachment = makePendingAttachment(
            kind: .textFile,
            filename: "overflow.txt",
            data: Data(),
            mediaType: "text/plain",
            textContent: ""
        )

        XCTAssertThrowsError(
            try AttachmentComposer.append([extraAttachment], to: existingAttachments)
        ) { error in
            XCTAssertEqual(
                error as? AttachmentComposerError,
                .tooManyAttachments(limit: AttachmentComposer.maxAttachmentCount)
            )
        }
    }

    func testImageResizeAboveThreshold() throws {
        var oversizedImageData = makeNoisyPNGData(width: 3_000, height: 3_000)
        if oversizedImageData.count <= AttachmentComposer.maxImageBytes {
            oversizedImageData = makeNoisyPNGData(width: 4_096, height: 4_096)
        }

        XCTAssertGreaterThan(oversizedImageData.count, AttachmentComposer.maxImageBytes)

        let attachment = try AttachmentComposer.imageAttachment(
            data: oversizedImageData,
            filename: "oversized.png",
            mediaType: "image/png"
        )

        XCTAssertEqual(attachment.kind, .image)
        XCTAssertEqual(attachment.mediaType, "image/jpeg")
        XCTAssertLessThanOrEqual(attachment.data.count, AttachmentComposer.maxImageBytes)
    }

    func testPasteImageFromClipboardCreatesPendingAttachment() throws {
        let attachment = try AttachmentComposer.pastedImageAttachment(
            data: makeNoisyPNGData(width: 48, height: 48)
        )

        XCTAssertEqual(attachment.kind, .image)
        XCTAssertEqual(attachment.filename, "Pasted Image.png")
        XCTAssertEqual(attachment.mediaType, "image/png")
        XCTAssertFalse(attachment.data.isEmpty)
    }

    func testImageAttachmentPrefersDetectedMediaTypeOverProvidedLabel() throws {
        let attachment = try AttachmentComposer.imageAttachment(
            data: makeNoisyJPEGData(width: 96, height: 96),
            filename: "logo.png",
            mediaType: "image/png"
        )

        XCTAssertEqual(attachment.kind, .image)
        XCTAssertEqual(attachment.mediaType, "image/jpeg")
    }

    func testPendingAttachmentFromFileURLUsesDetectedImageMediaType() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("png")
        try makeNoisyJPEGData(width: 96, height: 96).write(to: url)
        defer {
            try? FileManager.default.removeItem(at: url)
        }

        let attachment = try AttachmentComposer.pendingAttachment(fromFileURL: url)

        XCTAssertEqual(attachment.kind, .image)
        XCTAssertEqual(attachment.filename, url.lastPathComponent)
        XCTAssertEqual(attachment.mediaType, "image/jpeg")
    }

    func testTextFileReadAndInject() throws {
        let attachment = try AttachmentComposer.textFileAttachment(
            data: Data("name,email\nAlice,alice@example.com".utf8),
            filename: "report.csv",
            mediaType: "text/csv"
        )

        let payload = AttachmentComposer.prepareMessage(
            message: "Analyze this customer list.",
            attachments: [attachment]
        )

        XCTAssertEqual(
            payload.message,
            """
            [file: report.csv]
            name,email
            Alice,alice@example.com
            [/file: report.csv]

            Analyze this customer list.
            """
        )
        XCTAssertTrue(payload.images.isEmpty)
        XCTAssertTrue(payload.documents.isEmpty)
    }

    func testTextFileRejectsBinary() {
        XCTAssertThrowsError(
            try AttachmentComposer.textFileAttachment(
                data: Data([0xFF, 0x00, 0xD8, 0x42]),
                filename: "report.csv",
                mediaType: "text/csv"
            )
        ) { error in
            XCTAssertEqual(
                error as? AttachmentComposerError,
                .binaryTextFile("report.csv")
            )
        }
    }

    func testTextFileSizeLimit() {
        XCTAssertThrowsError(
            try AttachmentComposer.textFileAttachment(
                data: Data(repeating: 0x61, count: AttachmentComposer.maxTextBytes + 1),
                filename: "large.txt",
                mediaType: "text/plain"
            )
        ) { error in
            XCTAssertEqual(
                error as? AttachmentComposerError,
                .textFileTooLarge("large.txt")
            )
        }
    }

    func testPDFAttachmentEncoding() throws {
        let pdfData = makeMinimalPDFData()
        let attachment = try AttachmentComposer.pdfAttachment(data: pdfData, filename: "brief.pdf")
        let payload = AttachmentComposer.prepareMessage(message: "", attachments: [attachment])

        XCTAssertEqual(
            payload.documents,
            [
                DocumentPayload(
                    data: pdfData.base64EncodedString(),
                    mediaType: "application/pdf",
                    filename: "brief.pdf"
                )
            ]
        )
        XCTAssertEqual(
            payload.contentBlocks,
            [
                .document(
                    mediaType: "application/pdf",
                    data: pdfData.base64EncodedString(),
                    filename: "brief.pdf"
                )
            ]
        )
    }

    func testPDFSizeLimit() {
        XCTAssertThrowsError(
            try AttachmentComposer.pdfAttachment(
                data: Data(repeating: 0x20, count: AttachmentComposer.maxPDFBytes + 1),
                filename: "large.pdf"
            )
        ) { error in
            XCTAssertEqual(
                error as? AttachmentComposerError,
                .pdfTooLarge("large.pdf")
            )
        }
    }

    func testSendDraftQueuesAttachmentOnlyMessageForCurrentStreamingSession() {
        let sut = makeSUT(connectionStatus: .connected)
        let documentAttachment = makePendingAttachment(
            kind: .pdf,
            filename: "brief.pdf",
            data: Data("%PDF".utf8),
            mediaType: "application/pdf"
        )

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.pendingAttachments = [documentAttachment]

        sut.sendDraft()

        XCTAssertEqual(sut.draftMessage, "")
        XCTAssertTrue(sut.pendingAttachments.isEmpty)
        XCTAssertEqual(sut.queuedMessage, "Queued brief.pdf")

        let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

        XCTAssertEqual(delivery?.text, "")
        XCTAssertEqual(delivery?.attachments, [documentAttachment])
        XCTAssertEqual(delivery?.sessionID, "session-a")
    }

    func testQueuedAttachmentOnlyMessageUsesAttachmentCountSummary() {
        let sut = makeSUT(connectionStatus: .connected)

        sut.prepareToDisplaySession("session-a")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.pendingAttachments = [
            makePendingAttachment(
                kind: .image,
                filename: "photo.png",
                data: Data([0x89, 0x50, 0x4E, 0x47]),
                mediaType: "image/png"
            ),
            makePendingAttachment(
                kind: .pdf,
                filename: "brief.pdf",
                data: Data("%PDF".utf8),
                mediaType: "application/pdf"
            ),
        ]

        sut.sendDraft()

        XCTAssertEqual(sut.queuedMessage, "Queued 2 attachments")
    }

    func testSendDraftSendsImmediatelyWhenAnotherSessionIsStreaming() async {
        let sut = makeSUT(connectionStatus: .connected)

        sut.prepareToDisplaySession("session-b")
        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-b",
            streamingSessionID: "session-a"
        )
        sut.draftMessage = "follow up"

        sut.sendDraft()

        XCTAssertEqual(sut.draftMessage, "")
        XCTAssertNil(sut.queuedMessage)

        await waitForTranscriptItems(on: sut, minimumCount: 1)

        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage).map(\.content), ["follow up"])
    }

    func testInvalidateSessionRemovesCacheAndClearsVisibleTranscriptForCurrentSession() {
        let sut = makeSUT()
        let message = SessionMessage(role: .assistant, content: "cached", timestamp: 1)

        sut.cacheMessages([message], for: "session-a")
        sut.prepareToDisplaySession("session-a")

        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage), [message])

        sut.invalidateSession("session-a")

        XCTAssertNil(sut.cachedMessages(for: "session-a"))
        XCTAssertTrue(sut.transcriptItems.isEmpty)
    }

    func testInvalidateSessionClearsDraftForSession() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-a")
        sut.draftMessage = "stale draft"

        sut.invalidateSession("session-a")

        XCTAssertEqual(sut.draftMessage, "")
    }

    func testInvalidateSessionDoesNotAffectOtherVisibleSession() {
        let sut = makeSUT()
        let hiddenMessage = SessionMessage(role: .assistant, content: "hidden", timestamp: 1)
        let visibleMessage = SessionMessage(role: .assistant, content: "visible", timestamp: 2)

        sut.cacheMessages([hiddenMessage], for: "session-a")
        sut.cacheMessages([visibleMessage], for: "session-b")
        sut.prepareToDisplaySession("session-b")

        sut.invalidateSession("session-a")

        XCTAssertNil(sut.cachedMessages(for: "session-a"))
        XCTAssertEqual(sut.cachedMessages(for: "session-b"), [visibleMessage])
        XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage), [visibleMessage])
    }

    func testTranscriptCacheEvictsLeastRecentlyUsedSessions() {
        let sut = makeSUT()

        for index in 0...ChatViewModel.maxCachedSessions {
            let message = SessionMessage(role: .assistant, content: "message-\(index)", timestamp: index)
            sut.cacheMessages([message], for: "session-\(index)")
        }

        XCTAssertNil(sut.cachedMessages(for: "session-0"))
        XCTAssertNotNil(sut.cachedMessages(for: "session-1"))
        XCTAssertNotNil(sut.cachedMessages(for: "session-\(ChatViewModel.maxCachedSessions)"))
    }

    func testEnqueuePermissionPromptExposesActivePromptAndIndicator() {
        let sut = makeSUT()
        let prompt = PermissionPrompt(id: "prompt-1", action: "write", path: "/tmp/report.md", tier: 2)

        sut.enqueuePermissionPromptForTesting(prompt)

        XCTAssertEqual(sut.activePermissionPrompt, prompt)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 1)
        XCTAssertTrue(sut.hasPendingPermissionPrompt)
        XCTAssertEqual(sut.permissionPromptIndicatorText, "Approval needed: write /tmp/report.md")
    }

    func testPermissionPromptQueuePromotesNextPromptWhenFirstCompletes() {
        let sut = makeSUT()
        let first = PermissionPrompt(id: "prompt-1", action: "write", path: "/tmp/first.md", tier: 1)
        let second = PermissionPrompt(id: "prompt-2", action: "run", path: "/usr/bin/git", tier: 3)

        sut.enqueuePermissionPromptForTesting(first)
        sut.enqueuePermissionPromptForTesting(second)

        XCTAssertEqual(sut.activePermissionPrompt, first)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 2)

        sut.finishActivePermissionPromptForTesting(id: first.id)

        XCTAssertEqual(sut.activePermissionPrompt, second)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 1)
    }

    func testStopStreamingClearsPermissionPromptState() {
        let sut = makeSUT()
        let prompt = PermissionPrompt(id: "prompt-1", action: "write", path: "/tmp/report.md", tier: 2)

        sut.prepareToDisplaySession("session-a")
        sut.enqueuePermissionPromptForTesting(prompt)

        sut.stopStreamingForTesting()

        XCTAssertNil(sut.activePermissionPrompt)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 0)
        XCTAssertNil(sut.permissionPromptIndicatorText)
        XCTAssertNil(sut.permissionPromptErrorMessage)
    }

    func testResetStreamingStateClearsPermissionPromptWhenLastStreamEnds() {
        let sut = makeSUT()
        let prompt = PermissionPrompt(id: "prompt-1", action: "write", path: "/tmp/report.md", tier: 2)

        sut.setStreamingStateForTesting(
            isStreaming: true,
            currentSessionID: "session-a",
            streamingSessionID: "session-a"
        )
        sut.enqueuePermissionPromptForTesting(prompt)

        sut.resetStreamingStateForTesting(sessionID: "session-a")

        XCTAssertNil(sut.activePermissionPrompt)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 0)
    }

    func testResetStreamingStateKeepsPermissionPromptWhileAnotherStreamIsActive() {
        let sut = makeSUT()
        let prompt = PermissionPrompt(id: "prompt-1", action: "write", path: "/tmp/report.md", tier: 2)

        sut.setStreamingSessionsForTesting(
            [
                "session-a": (text: "alpha", phase: .reason),
                "session-b": (text: "beta", phase: .act),
            ],
            currentSessionID: "session-b"
        )
        sut.enqueuePermissionPromptForTesting(prompt)

        sut.resetStreamingStateForTesting(sessionID: "session-a")

        XCTAssertEqual(sut.activePermissionPrompt, prompt)
        XCTAssertEqual(sut.pendingPermissionPromptCount, 1)
    }

    private func makeSUT(
        connectionStatus: ConnectionStatus = .disconnected,
        compactionBannerSleepHandler: @escaping @Sendable (Duration) async throws -> Void = { duration in
            try await Task.sleep(for: duration)
        }
    ) -> ChatViewModel {
        let appState = AppState(startLoadingPersistedState: false)
        appState.connectionStatus = connectionStatus
        let sessionViewModel = SessionViewModel(appState: appState)
        return ChatViewModel(
            appState: appState,
            sessionViewModel: sessionViewModel,
            compactionBannerSleepHandler: compactionBannerSleepHandler
        )
    }

    private func waitForSleepToBeScheduled(on sleeper: ControlledSleeper) async {
        for _ in 0..<20 {
            if await sleeper.pendingSleepCount > 0 {
                return
            }
            await Task.yield()
        }

        XCTFail("Expected the render timer to schedule a sleep.")
    }

    private func waitForTranscriptItems(on sut: ChatViewModel, minimumCount: Int) async {
        for _ in 0..<20 {
            if sut.transcriptItems.count >= minimumCount {
                return
            }
            await Task.yield()
        }

        XCTFail("Expected at least \(minimumCount) transcript item(s).")
    }

    private func makePendingAttachment(
        kind: PendingAttachmentKind,
        filename: String,
        data: Data,
        mediaType: String,
        textContent: String? = nil
    ) -> PendingAttachment {
        PendingAttachment(
            kind: kind,
            filename: filename,
            data: data,
            mediaType: mediaType,
            textContent: textContent
        )
    }

    private func makeNoisyPNGData(width: Int, height: Int) -> Data {
        makeNoisyImageData(width: width, height: height, type: .png)
    }

    private func makeNoisyJPEGData(width: Int, height: Int) -> Data {
        makeNoisyImageData(
            width: width,
            height: height,
            type: .jpeg,
            properties: [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary
        )
    }

    private func makeNoisyImageData(
        width: Int,
        height: Int,
        type: UTType,
        properties: CFDictionary? = nil
    ) -> Data {
        let bytesPerPixel = 4
        let bytesPerRow = width * bytesPerPixel
        var pixels = [UInt8](repeating: 0, count: bytesPerRow * height)
        var state: UInt64 = 0x1234_5678_9ABC_DEF0

        for y in 0..<height {
            for x in 0..<width {
                let offset = (y * bytesPerRow) + (x * bytesPerPixel)
                state = state &* 6364136223846793005 &+ 1
                pixels[offset] = UInt8(truncatingIfNeeded: state >> 24)
                state = state &* 6364136223846793005 &+ 1
                pixels[offset + 1] = UInt8(truncatingIfNeeded: state >> 16)
                state = state &* 6364136223846793005 &+ 1
                pixels[offset + 2] = UInt8(truncatingIfNeeded: state >> 8)
                pixels[offset + 3] = 255
            }
        }

        let provider = CGDataProvider(data: Data(pixels) as CFData)!
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
        let image = CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: bytesPerRow,
            space: colorSpace,
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        )!

        let output = NSMutableData()
        let destination = CGImageDestinationCreateWithData(
            output,
            type.identifier as CFString,
            1,
            nil
        )!
        CGImageDestinationAddImage(destination, image, properties)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return output as Data
    }

    private func makeMinimalPDFData() -> Data {
        Data(
            """
            %PDF-1.4
            1 0 obj
            << /Type /Catalog /Pages 2 0 R >>
            endobj
            2 0 obj
            << /Type /Pages /Count 1 /Kids [3 0 R] >>
            endobj
            3 0 obj
            << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>
            endobj
            trailer
            << /Root 1 0 R >>
            %%EOF
            """.utf8
        )
    }

    // MARK: - Transcript merge edge-case tests

    func testMergeFetchedMessagesReturnsServerTruthWhenNoSharedPrefix() {
        let sut = makeSUT()
        let localMessages = [
            SessionMessage(role: .user, content: "local only", timestamp: 1),
            SessionMessage(role: .assistant, content: "local reply", timestamp: 2),
        ]
        let fetchedMessages = [
            SessionMessage(role: .user, content: "server only", timestamp: 10),
            SessionMessage(role: .assistant, content: "server reply", timestamp: 11),
        ]

        sut.cacheMessages(localMessages, for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting(fetchedMessages, sessionID: "session-a")

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.content),
            ["server only", "server reply"]
        )
    }

    func testMergeFetchedMessagesFallsBackWhenNoAlignmentFoundAfterSharedPrefix() {
        let sut = makeSUT()
        let sharedMessage = SessionMessage(role: .user, content: "shared start", timestamp: 1)
        let localTail = [
            SessionMessage(role: .assistant, content: "local-a", timestamp: 2),
            SessionMessage(role: .assistant, content: "local-b", timestamp: 3),
        ]
        let fetchedTail = [
            SessionMessage(role: .assistant, content: "fetched-x", timestamp: 12),
            SessionMessage(role: .assistant, content: "fetched-y", timestamp: 13),
        ]

        sut.cacheMessages([sharedMessage] + localTail, for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting([sharedMessage] + fetchedTail, sessionID: "session-a")

        XCTAssertEqual(
            sut.cachedMessages(for: "session-a")?.map(\.content),
            ["shared start", "fetched-x", "fetched-y", "local-a", "local-b"]
        )
    }

    func testMergeFetchedMessagesInterleavesMultipleGapsCorrectly() {
        let sut = makeSUT()
        let anchor1 = SessionMessage(role: .user, content: "anchor-1", timestamp: 1)
        let anchor2 = SessionMessage(role: .user, content: "anchor-2", timestamp: 5)
        let anchor3 = SessionMessage(role: .user, content: "anchor-3", timestamp: 9)

        let localMessages = [
            anchor1,
            SessionMessage(role: .assistant, content: "local-gap-1", timestamp: 2),
            anchor2,
            SessionMessage(role: .assistant, content: "local-gap-2", timestamp: 6),
            anchor3,
        ]
        let fetchedMessages = [
            anchor1,
            SessionMessage(role: .assistant, content: "fetched-gap-1", timestamp: 3),
            anchor2,
            SessionMessage(role: .assistant, content: "fetched-gap-2", timestamp: 7),
            anchor3,
        ]

        sut.cacheMessages(localMessages, for: "session-a")
        sut.prepareToDisplaySession("session-a")

        sut.applyFetchedMessagesForTesting(fetchedMessages, sessionID: "session-a")

        let contents = sut.cachedMessages(for: "session-a")?.map(\.content)
        XCTAssertEqual(contents, [
            "anchor-1",
            "fetched-gap-1",
            "local-gap-1",
            "anchor-2",
            "fetched-gap-2",
            "local-gap-2",
            "anchor-3",
        ])
    }

    private func waitForCompactionBannerToDismiss(on sut: ChatViewModel) async {
        for _ in 0..<20 {
            if sut.compactionBannerInfo == nil {
                return
            }
            await Task.yield()
        }

        XCTFail("Expected the compaction banner to dismiss.")
    }
}

private actor ControlledSleeper {
    private var continuations: [CheckedContinuation<Void, Error>] = []

    var pendingSleepCount: Int {
        continuations.count
    }

    func sleep(for _: Duration) async throws {
        try await withCheckedThrowingContinuation { continuation in
            continuations.append(continuation)
        }
    }

    func resumeNextSleep() {
        guard !continuations.isEmpty else {
            return
        }

        continuations.removeFirst().resume(returning: ())
    }
}
