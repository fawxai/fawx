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

    func testTranscriptScrollCoordinatorPublishCurrentPinnedStatePrimesDeduplication() {
        let coordinator = TranscriptScrollCoordinator()

        coordinator.activateSession("session-a")

        XCTAssertEqual(
            coordinator.publishCurrentPinnedState(distanceFromBottom: -10),
            TranscriptPinnedStateUpdate(distanceFromBottom: 0, isPinnedToBottom: true)
        )
        XCTAssertNil(
            coordinator.update(
                observation: TranscriptScrollObservation(contentOffsetY: 20, distanceFromBottom: 10),
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

    private func makeSUT(connectionStatus: ConnectionStatus = .disconnected) -> ChatViewModel {
        let appState = AppState(startLoadingPersistedState: false)
        appState.connectionStatus = connectionStatus
        let sessionViewModel = SessionViewModel(appState: appState)
        return ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
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
