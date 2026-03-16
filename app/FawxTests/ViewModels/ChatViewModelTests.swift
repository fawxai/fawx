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

    private func makeSUT() -> ChatViewModel {
        let appState = AppState()
        let sessionViewModel = SessionViewModel(appState: appState)
        return ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
    }
}
