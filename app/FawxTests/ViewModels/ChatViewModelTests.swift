import XCTest
@testable import Fawx

@MainActor
final class ChatViewModelTests: XCTestCase {
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

    func testPrepareToDisplaySessionShowsLoadingStateWhenCacheIsMissing() {
        let sut = makeSUT()

        sut.prepareToDisplaySession("session-missing")

        XCTAssertTrue(sut.isLoadingHistory)
        XCTAssertTrue(sut.transcriptItems.isEmpty)
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

        sut.appendStreamingTokenForTesting("tail")

        XCTAssertEqual(sut.streamingText, "")

        sut.flushStreamingDisplayForTesting()

        XCTAssertEqual(sut.streamingText, "tail")

        try await Task.sleep(for: .milliseconds(80))

        XCTAssertEqual(sut.streamingText, "tail")
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

    func testQueuedMessageDeliveryDiscardsOnSessionMismatch() {
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
        XCTAssertNil(sut.queuedMessage)
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
