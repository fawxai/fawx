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

  func testMakeTranscriptItemsProducesStableUniqueIDsForDuplicateAssistantMessages() {
    let sut = makeSUT()
    let duplicateA = SessionMessage(role: .assistant, content: "same", timestamp: 1)
    let duplicateB = SessionMessage(role: .assistant, content: "same", timestamp: 1)

    let firstPass = sut.makeTranscriptItems(from: [duplicateA, duplicateB])
    let secondPass = sut.makeTranscriptItems(from: [duplicateA, duplicateB])

    XCTAssertEqual(firstPass.map(\.id), secondPass.map(\.id))
    XCTAssertEqual(
      firstPass.map(\.id),
      [
        "message:assistant:1:97b5e18bf93ef5b",
        "final-answer:assistant:1:97b5e18bf93ef5b#1",
      ])
    XCTAssertEqual(firstPass.map(\.phase), [.message, .finalAnswer])
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

  func testApplyFetchedMessagesPreservesFinalAnswerBeforeQueuedFollowUpWhenServerInvertsOrder() {
    let sut = makeSUT()
    let initialUserMessage = SessionMessage(role: .user, content: "Review PR 1846", timestamp: 11)
    let completedAssistantMessage = SessionMessage(
      role: .assistant,
      content: "Comment posted to PR #1846.",
      timestamp: 12
    )
    let queuedFollowUpMessage = SessionMessage(
      role: .user,
      content: "Review the follow-up commit too.",
      timestamp: 13
    )
    let fetchedInitialUserMessage = SessionMessage(
      role: .user,
      content: initialUserMessage.content,
      timestamp: 21
    )
    let fetchedFollowUpMessage = SessionMessage(
      role: .user,
      content: queuedFollowUpMessage.content,
      timestamp: 22
    )
    let fetchedCompletedAssistantMessage = SessionMessage(
      role: .assistant,
      content: completedAssistantMessage.content,
      timestamp: 23
    )

    sut.cacheMessages(
      [
        initialUserMessage,
        completedAssistantMessage,
        queuedFollowUpMessage,
      ],
      for: "session-a"
    )
    sut.prepareToDisplaySession("session-a")

    sut.applyFetchedMessagesForTesting(
      [
        fetchedInitialUserMessage,
        fetchedFollowUpMessage,
        fetchedCompletedAssistantMessage,
      ],
      sessionID: "session-a"
    )

    XCTAssertEqual(
      sut.cachedMessages(for: "session-a")?.map(\.content),
      [
        initialUserMessage.content,
        completedAssistantMessage.content,
        queuedFollowUpMessage.content,
      ]
    )
    let transcriptMessages = sut.transcriptItems.compactMap(\.sessionMessage)
    XCTAssertEqual(
      transcriptMessages.map(\.content),
      [
        initialUserMessage.content,
        completedAssistantMessage.content,
        queuedFollowUpMessage.content,
      ]
    )
    guard
      let finalAnswerIndex = sut.transcriptItems.firstIndex(where: { item in
        if case .finalAnswer = item {
          return true
        }
        return false
      }),
      let followUpIndex = sut.transcriptItems.firstIndex(where: { item in
        item.sessionMessage?.role == .user
          && item.sessionMessage?.content == queuedFollowUpMessage.content
      })
    else {
      return XCTFail("Expected final answer and queued follow-up transcript items")
    }
    XCTAssertLessThan(finalAnswerIndex, followUpIndex)
  }

  func testApplyFetchedMessagesPreservesMultipleLocalAssistantUserTurnBoundaries() {
    let sut = makeSUT()
    let firstUser = SessionMessage(role: .user, content: "First prompt", timestamp: 11)
    let firstAnswer = SessionMessage(role: .assistant, content: "First answer", timestamp: 12)
    let secondUser = SessionMessage(role: .user, content: "Second prompt", timestamp: 13)
    let secondAnswer = SessionMessage(role: .assistant, content: "Second answer", timestamp: 14)
    let thirdUser = SessionMessage(role: .user, content: "Third prompt", timestamp: 15)

    let fetchedFirstUser = SessionMessage(role: .user, content: firstUser.content, timestamp: 21)
    let fetchedFirstAnswer = SessionMessage(role: .assistant, content: firstAnswer.content, timestamp: 22)
    let fetchedSecondUser = SessionMessage(role: .user, content: secondUser.content, timestamp: 23)
    let fetchedSecondAnswer = SessionMessage(role: .assistant, content: secondAnswer.content, timestamp: 24)
    let fetchedThirdUser = SessionMessage(role: .user, content: thirdUser.content, timestamp: 25)

    sut.cacheMessages(
      [firstUser, firstAnswer, secondUser, secondAnswer, thirdUser],
      for: "session-a"
    )
    sut.prepareToDisplaySession("session-a")

    sut.applyFetchedMessagesForTesting(
      [fetchedFirstUser, fetchedSecondUser, fetchedFirstAnswer, fetchedThirdUser, fetchedSecondAnswer],
      sessionID: "session-a"
    )

    XCTAssertEqual(
      sut.cachedMessages(for: "session-a")?.map(\.content),
      [
        firstUser.content,
        firstAnswer.content,
        secondUser.content,
        secondAnswer.content,
        thirdUser.content,
      ]
    )
    XCTAssertEqual(
      sut.transcriptItems.compactMap(\.sessionMessage).map(\.content),
      [
        firstUser.content,
        firstAnswer.content,
        secondUser.content,
        secondAnswer.content,
        thirdUser.content,
      ]
    )
  }

  func testApplyFetchedMessagesReplacesOptimisticAssistantTailWithServerCompletedTurn() {
    let sut = makeSUT()
    let localUserMessage = SessionMessage(
      role: .user,
      content: "Research the X API v2 POST /2/tweets endpoint.",
      timestamp: 11
    )
    let optimisticAssistant = SessionMessage(
      role: .assistant,
      content: "Relevant requirements for POST /2/tweets for the x-post skill...",
      timestamp: 12
    )

    let fetchedUserMessage = SessionMessage(
      role: .user,
      content: localUserMessage.content,
      timestamp: 21
    )
    let fetchedAssistantSummary = SessionMessage(
      role: .assistant,
      content: "I can't save the spec because the tool budget is exhausted.",
      timestamp: 22
    )
    let fetchedAssistantDecomposition = SessionMessage(
      role: .assistant,
      content:
        "Task decomposition results:\n1. Research X API v2 POST /2/tweets => budget exhausted",
      timestamp: 23
    )

    sut.cacheMessages([localUserMessage, optimisticAssistant], for: "session-a")
    sut.prepareToDisplaySession("session-a")

    sut.applyFetchedMessagesForTesting(
      [
        fetchedUserMessage,
        fetchedAssistantSummary,
        fetchedAssistantDecomposition,
      ],
      sessionID: "session-a"
    )

    XCTAssertEqual(
      sut.cachedMessages(for: "session-a")?.map(\.content),
      [
        localUserMessage.content,
        fetchedAssistantSummary.content,
        fetchedAssistantDecomposition.content,
      ]
    )
    XCTAssertEqual(
      sut.transcriptItems.compactMap(\.sessionMessage).map(\.content),
      [
        localUserMessage.content,
        fetchedAssistantSummary.content,
        fetchedAssistantDecomposition.content,
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

    let items = sut.makeTranscriptItems(from: [
      assistantToolMessage, toolResultMessage, assistantReply,
    ])

    XCTAssertEqual(items.count, 3)

    guard case .message(let narration) = items[0] else {
      return XCTFail("Expected first transcript item to be working narration")
    }
    XCTAssertEqual(narration.displayText, "Let me check.")
    XCTAssertTrue(narration.isWorkingNarration)
    guard case .toolActivityGroup(let group) = items[1] else {
      return XCTFail("Expected second transcript item to be tool activity")
    }
    XCTAssertEqual(group.toolCount, 1)
    XCTAssertEqual(group.toolCalls[0].name, "read_file")
    XCTAssertTrue(group.toolCalls[0].arguments.contains("README.md"))
    XCTAssertEqual(group.toolCalls[0].result, "file contents")
    XCTAssertFalse(group.toolCalls[0].isError)

    guard case .finalAnswer(let trailingMessage) = items[2] else {
      return XCTFail("Expected third transcript item to be final assistant message")
    }
    XCTAssertEqual(trailingMessage.displayText, "Done.")
  }

  func testMidTurnAssistantTextBeforeToolActivityIsNotFinalAnswer() {
    let sut = makeSUT()
    let planningMessage = SessionMessage(
      role: .assistant,
      content: "I’m checking the reducer before I answer.",
      timestamp: 1
    )
    let assistantToolMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("Now I’ll inspect the file."),
        .toolUse(
          id: "call_1",
          name: "read_file",
          input: .object(["path": .string("app/Fawx/ViewModels/ChatViewModel.swift")])
        ),
      ],
      timestamp: 2
    )
    let toolResultMessage = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("source"), isError: false)
      ],
      timestamp: 3
    )
    let assistantReply = SessionMessage(role: .assistant, content: "Done.", timestamp: 4)

    let items = sut.makeTranscriptItems(from: [
      planningMessage,
      assistantToolMessage,
      toolResultMessage,
      assistantReply,
    ])

    XCTAssertEqual(items.count, 4)
    guard case .message(let planning)? = items.first else {
      return XCTFail("Expected mid-turn assistant planning text to remain a message")
    }
    XCTAssertEqual(planning.displayText, "I’m checking the reducer before I answer.")
    XCTAssertTrue(planning.isWorkingNarration)
    XCTAssertEqual(items.first?.phase, .workingNarration)

    guard case .message(let toolNarration) = items[1] else {
      return XCTFail("Expected tool narration before activity")
    }
    XCTAssertEqual(toolNarration.displayText, "Now I’ll inspect the file.")
    XCTAssertTrue(toolNarration.isWorkingNarration)

    guard case .toolActivityGroup(let group) = items[2] else {
      return XCTFail("Expected tool activity after planning text")
    }
    XCTAssertEqual(group.toolCalls.first?.result, "source")

    guard case .finalAnswer(let finalAnswer) = items[3] else {
      return XCTFail("Expected terminal assistant reply to be final answer")
    }
    XCTAssertEqual(finalAnswer.displayText, "Done.")
  }

  func testAssistantTrailingTextAfterToolUseInSameMessageIsFinalAnswer() {
    let sut = makeSUT()
    let assistantToolMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("I’ll inspect the diff first."),
        .toolUse(
          id: "call_1",
          name: "run_command",
          input: .object(["command": .string("git diff --stat")])
        ),
        .text("The diff is small and focused."),
      ],
      timestamp: 100
    )
    let toolResultMessage = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("diff stats"), isError: false)
      ],
      timestamp: 120
    )

    let items = sut.makeTranscriptItems(from: [assistantToolMessage, toolResultMessage])

    XCTAssertEqual(items.map(\.phase), [.workingNarration, .toolGroup, .finalAnswer])
    guard case .finalAnswer(let finalMessage)? = items.last else {
      return XCTFail("Expected trailing assistant text after tool use to be final")
    }
    XCTAssertEqual(finalMessage.displayText, "The diff is small and focused.")
  }

  func testAssistantMessagePhaseOnlyUsesWorkingNarrationWhenFlagged() {
    let assistantMessage = SessionMessage(role: .assistant, content: "Plain answer.", timestamp: 1)
    let plainItem = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-plain",
        message: assistantMessage,
        displayText: "Plain answer.",
        footnoteText: nil
      )
    )
    let workingItem = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working",
        message: assistantMessage,
        displayText: "I’m checking first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )

    XCTAssertEqual(plainItem.phase, .message)
    XCTAssertEqual(workingItem.phase, .workingNarration)
  }

  func testMakeTranscriptItemsChunksNarrationWithNearestToolActivity() throws {
    let sut = makeSUT()
    let assistantToolMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("I’ll inspect the diff first."),
        .toolUse(
          id: "call_1",
          name: "run_command",
          input: .object(["command": .string("git diff --stat")])
        ),
        .text("Now I’ll open the implementation."),
        .toolUse(
          id: "call_2",
          name: "read_file",
          input: .object(["path": .string("src/lib.rs")])
        ),
      ],
      timestamp: 2
    )
    let toolResultMessage = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("diff stats"), isError: false),
        .toolResult(toolUseId: "call_2", content: .string("source contents"), isError: false),
      ],
      timestamp: 3
    )

    let transcriptItems = sut.makeTranscriptItems(from: [
      assistantToolMessage, toolResultMessage,
    ])
    let narrationTexts = transcriptItems.compactMap { item -> String? in
      guard case .message(let message) = item, message.isWorkingNarration else {
        return nil
      }
      return message.displayText
    }
    let groups = transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(groups.count, 2)
    XCTAssertEqual(narrationTexts, ["I’ll inspect the diff first.", "Now I’ll open the implementation."])
    let firstGroup = try XCTUnwrap(groups.first)
    let secondGroup = try XCTUnwrap(groups.dropFirst().first)
    XCTAssertEqual(firstGroup.toolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(firstGroup.toolCalls.first?.result, "diff stats")
    XCTAssertEqual(secondGroup.toolCalls.map(\.id), ["call_2"])
    XCTAssertEqual(secondGroup.toolCalls.first?.result, "source contents")
  }

  func testCompletedWorkSummaryWrapsActivityBeforeFinalAnswer() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let assistantToolMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("I’ll inspect the diff first."),
        .toolUse(
          id: "call_1",
          name: "run_command",
          input: .object(["command": .string("git diff --stat")])
        ),
        .text("The diff is small and focused."),
      ],
      timestamp: 100
    )
    let toolResultMessage = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("diff stats"), isError: false)
      ],
      timestamp: 120
    )

    sut.recordCompletedStreamingFootnoteForTesting(
      assistantToolMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [assistantToolMessage, toolResultMessage]
    )

    XCTAssertEqual(items.count, 2)
    guard case .completedWorkSummary(let summary) = items[0] else {
      return XCTFail("Expected completed work summary before final answer")
    }
    XCTAssertEqual(summary.elapsedText, "Worked for 42 seconds")
    XCTAssertEqual(summary.entries.count, 2)
    guard case .narration(let narration) = summary.entries[0] else {
      return XCTFail("Expected completed work narration before tool chunk")
    }
    XCTAssertEqual(narration.text, "I’ll inspect the diff first.")
    guard case .toolActivityGroup(let toolGroup) = summary.entries[1] else {
      return XCTFail("Expected completed work tool chunk after narration")
    }
    XCTAssertEqual(toolGroup.toolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(toolGroup.toolCalls[0].result, "diff stats")

    guard case .finalAnswer(let finalMessage) = items[1] else {
      return XCTFail("Expected final assistant message after completed work summary")
    }
    XCTAssertEqual(finalMessage.displayText, "The diff is small and focused.")
    XCTAssertNil(finalMessage.footnoteText)
  }

  func testCompletedWorkSummaryAbsorbsHistoricalWorkingRowsBeforeFinalAnswer() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let initialPrompt = SessionMessage(role: .user, content: "Inspect the transcript UI.", timestamp: 90)
    let firstNarration = "I'm locating the chat transcript components first."
    let secondNarration = "I found the reducer; now I'm checking how tool events are grouped."
    let firstHistoricalAssistant = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text(firstNarration),
        .toolUse(
          id: "call-1",
          name: "list_dir",
          input: .object(["path": .string("/Users/joseph/fawx/app")])
        ),
        .text("Historical loose narration after the first tool."),
      ],
      timestamp: 100
    )
    let firstToolResult = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call-1", content: .string("Fawx"), isError: false)
      ],
      timestamp: 101
    )
    let secondHistoricalAssistant = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text(secondNarration),
        .toolUse(
          id: "call-2",
          name: "read_file",
          input: .object(["path": .string("/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift")])
        ),
        .text("Historical loose narration after the second tool."),
      ],
      timestamp: 110
    )
    let secondToolResult = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call-2", content: .string("source"), isError: false)
      ],
      timestamp: 111
    )
    let finalAnswer = SessionMessage(
      role: .assistant,
      content: "The transcript UI has the right primitives but needs stronger turn boundaries.",
      timestamp: 142
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(firstNarration),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "List app directory", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-1", id: "call-1", name: "list_dir"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "list_dir",
        arguments: #"{"path":"/Users/joseph/fawx/app"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "list_dir",
        output: "Fawx",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(secondNarration),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-2", title: "Read reducer", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-2", id: "call-2", name: "read_file"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-2",
        id: "call-2",
        name: "read_file",
        arguments: #"{"path":"/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-2",
        id: "call-2",
        toolName: "read_file",
        output: "source",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-2"), sessionID: sessionID)

    sut.recordCompletedStreamingFootnoteForTesting(
      finalAnswer,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [
        initialPrompt,
        firstHistoricalAssistant,
        firstToolResult,
        secondHistoricalAssistant,
        secondToolResult,
        finalAnswer,
      ]
    )

    XCTAssertFalse(
      items.contains { item in
        if case .toolActivityGroup = item {
          return true
        }
        if case .message(let message) = item {
          return message.message.role == .assistant && message.isWorkingNarration
        }
        return false
      },
      "Historical working rows should be absorbed into the completed summary before the final answer."
    )
    guard case .completedWorkSummary(let summary)? = items.first(where: { item in
      if case .completedWorkSummary = item {
        return true
      }
      return false
    }) else {
      return XCTFail("expected completed work summary")
    }
    XCTAssertEqual(
      summary.entries.map { entry -> String in
        switch entry {
        case .narration(let narration):
          return "narration:\(narration.text)"
        case .toolActivityGroup(let group):
          let ids = group.toolCalls.map(\.id).joined(separator: ",")
          return "tool:\(ids)"
        case .turnSteering(let steering):
          return "steering:\(steering.text)"
        }
      },
      [
        "narration:\(firstNarration)",
        "tool:call-1",
        "narration:Historical loose narration after the first tool.",
        "narration:\(secondNarration)",
        "tool:call-2",
        "narration:Historical loose narration after the second tool.",
      ]
    )
    XCTAssertEqual(
      summary.activityGroups.flatMap(\.toolCalls).map(\.id),
      [
        "call-1",
        "call-2",
      ]
    )
    guard case .finalAnswer(let finalMessage)? = items.last else {
      return XCTFail("expected final answer")
    }
    XCTAssertEqual(finalMessage.displayText, finalAnswer.content)
  }

  func testCompletedActivityDoesNotAppendAfterFollowUpUserMessage() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let finalAnswer = SessionMessage(
      role: .assistant,
      content: "Done.",
      timestamp: 142
    )
    let followUp = SessionMessage(
      role: .user,
      content: "Next prompt",
      timestamp: 180
    )

    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "I inspected the diff.",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: #"{"command":"git diff --stat"}"#,
          result: "diff stats",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )
    sut.recordCompletedStreamingFootnoteForTesting(
      finalAnswer,
      sessionID: sessionID,
      startedAt: Date(timeIntervalSince1970: 100),
      endedAt: Date(timeIntervalSince1970: 142)
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [finalAnswer, followUp]
    )

    XCTAssertEqual(items.count, 3)
    guard case .completedWorkSummary(let summary) = items[0] else {
      return XCTFail("Expected completed work summary to stay attached to final answer")
    }
    XCTAssertEqual(summary.entries.count, 2)
    XCTAssertEqual(items.compactMap(\.sessionMessage).map(\.role), [.assistant, .user])
    XCTAssertFalse(
      items.dropFirst(2).contains { item in
        if case .toolActivityGroup = item {
          return true
        }
        return false
      },
      "Completed activity should not be re-appended after the next user prompt."
    )
  }

  func testToolActivityDescriptorClassifiesCommonToolKindsAndPartialTargets() {
    let commandDescriptor = ToolActivityDescriptor(
      name: "run_command",
      arguments: #"{"command":"cargo test"#
    )
    XCTAssertEqual(commandDescriptor.kind, .command)
    XCTAssertEqual(commandDescriptor.primaryTarget, "cargo test")

    let argvCommandDescriptor = ToolActivityDescriptor(
      name: "run_command",
      arguments: #"{"argv":["gh","pr","view","1863"]}"#
    )
    XCTAssertEqual(argvCommandDescriptor.kind, .command)
    XCTAssertEqual(argvCommandDescriptor.primaryTarget, "gh pr view 1863")

    let editDescriptor = ToolActivityDescriptor(
      name: "apply_patch",
      arguments: #"{"patch":"*** Begin Patch"}"#
    )
    XCTAssertEqual(editDescriptor.kind, .edit)
    XCTAssertTrue(editDescriptor.isCodeMutation)

    let searchDescriptor = ToolActivityDescriptor(
      name: "memory_search",
      arguments: #"{"query":"chat activity"}"#
    )
    XCTAssertEqual(searchDescriptor.kind, .search)
    XCTAssertEqual(searchDescriptor.primaryTarget, "chat activity")

    let malformedCompleteDescriptor = ToolActivityDescriptor(
      name: "read_file",
      arguments: #"{"path":README.md}"#
    )
    XCTAssertNil(malformedCompleteDescriptor.primaryTarget)
  }

  func testCompletedWorkSummarySnapshotKeepsNarrationAndNestedToolRows() {
    let summary = CompletedWorkSummaryRecord(
      id: "summary-a",
      elapsedText: "Worked for 42 seconds",
      entries: [
        .narration(
          CompletedWorkNarrationRecord(
            id: "group-a:narration",
            text: "I’ll inspect the diff first."
          )
        ),
      ] + CompletedWorkSummaryRecord.entries(
        from:
          ToolActivityGroupRecord(
            id: "group-a",
            toolCalls: [
              ToolCallRecord(
                id: "call_1",
                name: "run_command",
                arguments: #"{"command":"git diff --stat"}"#,
                result: "diff stats",
                isRunning: false,
                isError: false
              ),
              ToolCallRecord(
                id: "call_2",
                name: "run_command",
                arguments: #"{"command":"git status --short"}"#,
                result: "clean",
                isRunning: false,
                isError: false
              ),
            ],
            isLive: false
          )
      )
    )

    let snapshot = CompletedWorkSummarySnapshot(summary: summary)

    XCTAssertEqual(snapshot.elapsedText, "Worked for 42 seconds")
    XCTAssertTrue(snapshot.hasActivity)
    XCTAssertEqual(snapshot.entries.count, 3)
    guard case .narration(let narration) = snapshot.entries[0] else {
      return XCTFail("Expected completed work narration entry")
    }
    XCTAssertEqual(narration.text, "I’ll inspect the diff first.")
    guard case .toolChunk(let firstChunk) = snapshot.entries[1] else {
      return XCTFail("Expected first completed work tool chunk entry")
    }
    XCTAssertEqual(firstChunk.toolTitle, "Ran git diff --stat")
    XCTAssertEqual(firstChunk.rows.map(\.id), ["call_1"])
    XCTAssertTrue(firstChunk.rows[0].hasDetails)
    XCTAssertEqual(firstChunk.rows[0].detailSections.first?.title, "Shell")
    guard case .toolChunk(let secondChunk) = snapshot.entries[2] else {
      return XCTFail("Expected second completed work tool chunk entry")
    }
    XCTAssertEqual(secondChunk.toolTitle, "Ran git status --short")
    XCTAssertEqual(secondChunk.rows.map(\.id), ["call_2"])
    XCTAssertTrue(secondChunk.rows[0].hasDetails)
  }

  func testCompletedWorkSummarySnapshotShowsCodeMutationDetailsAsDiffOnly() {
    let patch = """
    *** Begin Patch
    *** Update File: README.md
    @@
    -old
    +new
    *** End Patch
    """
    let summary = CompletedWorkSummaryRecord(
      id: "summary-a",
      elapsedText: "Worked for 10 seconds",
      entries: [
        .narration(
          CompletedWorkNarrationRecord(
            id: "group-a:narration",
            text: "Now I’ll apply the focused patch."
          )
        ),
        .toolActivityGroup(
          ToolActivityGroupRecord(
            id: "group-a",
            toolCalls: [
              ToolCallRecord(
                id: "call_1",
                name: "apply_patch",
                arguments: patch,
                result: "Success",
                isRunning: false,
                isError: false
              )
            ],
            isLive: false
          )
        )
      ]
    )

    let snapshot = CompletedWorkSummarySnapshot(summary: summary)

    guard case .toolChunk(let chunk) = snapshot.entries[1] else {
      return XCTFail("Expected completed work tool chunk entry")
    }
    XCTAssertEqual(chunk.toolTitle, "Edited file")
    XCTAssertEqual(chunk.rows.count, 1)
    XCTAssertTrue(chunk.rows[0].hasDetails)
    XCTAssertEqual(chunk.rows[0].detailSections.map(\.title), ["Diff"])
  }

  func testToolActivityEventSnapshotUsesTypedProgressForSemanticSummary() {
    let advanced = AssistantActivityEventSnapshot(
      toolCall: ToolCallRecord(
        id: "call_1",
        name: "read_file",
        arguments: #"{"path":"README.md"}"#,
        result: "contents",
        isRunning: false,
        isError: false,
        progress: ToolProgressRecord(
          category: "observation",
          target: "README.md",
          advancesSlot: "evidence:file:README.md",
          outcome: "advanced"
        )
      )
    )

    XCTAssertEqual(advanced.summary, "Advanced evidence: README.md")

    let duplicate = AssistantActivityEventSnapshot(
      toolCall: ToolCallRecord(
        id: "call_2",
        name: "read_file",
        arguments: #"{"path":"README.md"}"#,
        result: "contents",
        isRunning: false,
        isError: false,
        progress: ToolProgressRecord(
          category: "observation",
          target: "README.md",
          advancesSlot: nil,
          outcome: "duplicate"
        )
      )
    )

    XCTAssertEqual(duplicate.summary, "Repeated work: README.md")
  }

  func testHiddenSessionToolActivityRemainsVisibleWhenReturning() throws {
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
    let group = try XCTUnwrap(groups.first)
    let toolCall = try XCTUnwrap(group.toolCalls.first)
    XCTAssertEqual(toolCall.name, "read_file")
    XCTAssertEqual(toolCall.result, "file contents")
  }

  func testLiveToolActivityGroupsStreamingAssistantTextIntoCurrentTurn() throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "Based on my inspection, here is what I found.",
      timestamp: 1
    )

    sut.cacheMessages([finalMessage], for: sessionID)
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .act
    )
    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "I’m checking the rendering path.",
      toolCalls: [
        ToolCallRecord(
          id: "call-1",
          name: "read_file",
          arguments: "{\"path\":\"ChatDetailView.swift\"}",
          result: nil,
          isRunning: true,
          isError: false
        )
      ]
    )

    sut.prepareToDisplaySession(sessionID)

    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.workingNarration, .workingNarration, .toolGroup])
    guard case .assistant(let turn)? = sut.transcriptTurns.first else {
      return XCTFail("expected a single assistant turn")
    }
    XCTAssertEqual(
      turn.workingNarration.map(\.text),
      ["Based on my inspection, here is what I found.", "I’m checking the rendering path."]
    )
    XCTAssertEqual(turn.toolGroups.map(\.id), ["__fawx_live_activity__:session-a:test"])
    XCTAssertNil(turn.finalAnswer)
  }

  func testFinalizingLiveTurnPlacesCachedAssistantTextAfterLiveActivity() throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "Based on my inspection, here is what I found.",
      timestamp: 1
    )

    sut.cacheMessages([finalMessage], for: sessionID)
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .act,
      transcriptPhase: .finalizing
    )
    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "I’m checking the rendering path.",
      toolCalls: [
        ToolCallRecord(
          id: "call-1",
          name: "read_file",
          arguments: "{\"path\":\"ChatDetailView.swift\"}",
          result: "file contents",
          isRunning: false,
          isError: false
        )
      ]
    )

    sut.prepareToDisplaySession(sessionID)

    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.workingNarration, .toolGroup, .finalAnswer])
    guard case .assistant(let turn)? = sut.transcriptTurns.first else {
      return XCTFail("expected a single assistant turn")
    }
    XCTAssertEqual(turn.workingNarration.map(\.text), ["I’m checking the rendering path."])
    XCTAssertEqual(turn.toolGroups.map(\.id), ["__fawx_live_activity__:session-a:test"])
    XCTAssertEqual(turn.finalAnswer?.displayText, "Based on my inspection, here is what I found.")
    XCTAssertTrue(sut.transcriptTurns.hasCurrentTurnTerminalAssistantOutput)
  }

  func testLiveToolActivityDoesNotInsertBeforePreviousTurnFinalAnswer() throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let previousAnswer = SessionMessage(role: .assistant, content: "Previous answer.", timestamp: 1)
    let currentPrompt = SessionMessage(role: .user, content: "Follow up.", timestamp: 2)

    sut.cacheMessages([previousAnswer, currentPrompt], for: sessionID)
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .act
    )
    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "I'm checking the follow-up request.",
      toolCalls: [
        ToolCallRecord(
          id: "call-1",
          name: "read_file",
          arguments: "{\"path\":\"ChatViewModel.swift\"}",
          result: nil,
          isRunning: true,
          isError: false
        )
      ]
    )

    sut.prepareToDisplaySession(sessionID)

    let previousAnswerIndex = try XCTUnwrap(
      sut.transcriptItems.firstIndex { $0.sessionMessage?.content == "Previous answer." }
    )
    let currentPromptIndex = try XCTUnwrap(
      sut.transcriptItems.firstIndex { $0.sessionMessage?.content == "Follow up." }
    )
    let liveGroupIndex = try XCTUnwrap(
      sut.transcriptItems.firstIndex {
        guard case .toolActivityGroup = $0 else {
          return false
        }
        return true
      }
    )

    XCTAssertLessThan(previousAnswerIndex, currentPromptIndex)
    XCTAssertLessThan(currentPromptIndex, liveGroupIndex)
  }

  func testLiveToolGroupPreservesCompletedToolsAcrossMultipleRoundsInOneTurn() throws {
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
    var group = try XCTUnwrap(groups.first)
    XCTAssertEqual(group.toolCalls.map(\.id), ["call_1", "call_2"])
    XCTAssertEqual(group.toolCalls.first?.result, "first result")
    XCTAssertTrue(group.toolCalls.dropFirst().first?.isRunning == true)

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
    group = try XCTUnwrap(groups.first)
    XCTAssertEqual(group.toolCalls.map(\.result), ["first result", "second result"])
  }

  func testLiveActivityNarrationBoundariesCreateSeparateToolChunks() throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingSessionsForTesting(
      [sessionID: (text: "", phase: .act)],
      currentSessionID: sessionID
    )
    sut.prepareToDisplaySession(sessionID)

    sut.appendActivityNarrationForTesting("I’ll inspect the diff first.", sessionID: sessionID)
    sut.markActivityNarrationBoundaryForTesting(sessionID: sessionID)
    sut.beginToolCallForTesting(sessionID: sessionID, id: "call_1", name: "run_command")
    sut.finishToolCallForTesting(
      sessionID: sessionID,
      id: "call_1",
      output: "diff stats",
      isError: false
    )

    sut.appendActivityNarrationForTesting("Now I’ll open the touched file.", sessionID: sessionID)
    sut.markActivityNarrationBoundaryForTesting(sessionID: sessionID)
    sut.beginToolCallForTesting(sessionID: sessionID, id: "call_2", name: "read_file")

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }
    let narrationTexts = sut.transcriptItems.compactMap { item -> String? in
      guard case .message(let message) = item, message.isWorkingNarration else {
        return nil
      }
      return message.displayText
    }

    XCTAssertEqual(groups.count, 2)
    XCTAssertEqual(narrationTexts, ["I’ll inspect the diff first.", "Now I’ll open the touched file."])
    let firstGroup = try XCTUnwrap(groups.first)
    let secondGroup = try XCTUnwrap(groups.dropFirst().first)
    XCTAssertEqual(firstGroup.toolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(secondGroup.toolCalls.map(\.id), ["call_2"])
  }

  func testTypedActivityStartCarriesPendingNarrationIntoToolChunk() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingSessionsForTesting(
      [sessionID: (text: "", phase: .act)],
      currentSessionID: sessionID
    )
    sut.prepareToDisplaySession(sessionID)

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’m locating the transcript components first."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "activity-a", title: nil, kind: nil),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "activity-a", id: "call_1", name: "read_file"),
      sessionID: sessionID
    )

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(groups.count, 1)
    let group = try XCTUnwrap(groups.first)
    XCTAssertEqual(group.id, "activity-a")
    XCTAssertTrue(
      sut.transcriptItems.contains { item in
        guard case .message(let message) = item, message.isWorkingNarration else {
          return false
        }
        return message.displayText == "I’m locating the transcript components first."
      }
    )
    XCTAssertEqual(group.toolCalls.map(\.id), ["call_1"])
    XCTAssertTrue(group.toolCalls[0].isRunning)
  }

  func testTypedActivityStartCoalescesPendingNarrationDeltasForOneActivity() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingSessionsForTesting(
      [sessionID: (text: "", phase: .act)],
      currentSessionID: sessionID
    )
    sut.prepareToDisplaySession(sessionID)

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’m locating the transcript model"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(" and then I’ll inspect the reducer."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "activity-a", title: nil, kind: nil),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "activity-a", id: "call_1", name: "read_file"),
      sessionID: sessionID
    )

    let narrationTexts = sut.transcriptItems.compactMap { item -> String? in
      guard case .message(let message) = item, message.isWorkingNarration else {
        return nil
      }
      return message.displayText
    }
    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(narrationTexts, [
      "I’m locating the transcript model and then I’ll inspect the reducer."
    ])
    XCTAssertEqual(groups.map(\.id), ["activity-a"])
    XCTAssertEqual(groups.first?.toolCalls.map(\.id), ["call_1"])
  }

  func testTypedActivityStartAbsorbsNarrationIntoExistingToolChunk() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingSessionsForTesting(
      [sessionID: (text: "", phase: .act)],
      currentSessionID: sessionID
    )
    sut.prepareToDisplaySession(sessionID)

    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "activity-a", id: "call_1", name: "read_file"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I found the reducer; now I’m checking grouping."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "activity-a", title: nil, kind: nil),
      sessionID: sessionID
    )

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(groups.count, 1)
    let group = try XCTUnwrap(groups.first)
    XCTAssertEqual(group.id, "activity-a")
    XCTAssertTrue(
      sut.transcriptItems.contains { item in
        guard case .message(let message) = item, message.isWorkingNarration else {
          return false
        }
        return message.displayText == "I found the reducer; now I’m checking grouping."
      }
    )
    XCTAssertEqual(group.toolCalls.map(\.id), ["call_1"])
    XCTAssertTrue(group.toolCalls[0].isRunning)
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

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)

    XCTAssertEqual(snapshot.detailStyle, .liveStatusOnly)
    XCTAssertFalse(snapshot.showsPayloadDetails)
    XCTAssertEqual(snapshot.visibleToolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(
      snapshot.accessibilityHint,
      "Collapse activity. Detailed arguments and output appear after the response finishes."
    )
  }

  func testPreviewTextBecomesActivityNarrationInsteadOfFinalStreamingText() {
    let sut = makeSUT()

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      phase: .reason
    )
    sut.appendActivityNarrationForTesting("I’ll inspect the diff first.", sessionID: "session-a")

    XCTAssertEqual(sut.visibleStreamingText, "")

    guard case .message(let narration)? = sut.transcriptItems.first else {
      return XCTFail("Expected preview text to render as live activity")
    }
    XCTAssertEqual(narration.displayText, "I’ll inspect the diff first.")
    XCTAssertTrue(narration.isWorkingNarration)
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.workingNarration])
  }

  func testLegacyTextDeltaAfterTypedActivityBecomesCompletedWorkNarration() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 136)
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "The architecture has partial separation and needs a final-answer lane.",
      timestamp: 136
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’m inspecting the model layer first."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Read model files", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-1", id: "call-1", name: "read_file"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "read_file",
        arguments: #"{"path":"app/Fawx/Models/ChatTranscript.swift"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "read_file",
        output: "enum ChatTranscriptItem { ... }",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)

    await sut.reduceStreamEventForTesting(
      .textDelta(
        "Good — the model layer is rich. Now I need to see how the reducer builds these items."
      ),
      sessionID: sessionID
    )

    XCTAssertEqual(
      sut.streamingTextForTesting(sessionID: sessionID),
      "",
      "Legacy mid-turn text after typed activity should not become transient final-answer text."
    )

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    let narrationText = summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }
    XCTAssertTrue(
      narrationText.contains(
        "Good — the model layer is rich. Now I need to see how the reducer builds these items."
      )
    )
    guard case .finalAnswer(let finalAnswer)? = items.last else {
      return XCTFail("expected final answer after completed work")
    }
    XCTAssertEqual(
      finalAnswer.displayText,
      "The architecture has partial separation and needs a final-answer lane."
    )
  }

  func testLegacyTextDeltaAfterLegacyToolActivityBecomesCompletedWorkNarration() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 135)
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "The transcript UI has partial separation but still needs stronger turn boundaries.",
      timestamp: 135
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .act,
      startedAt: startedAt
    )
    await sut.reduceStreamEventForTesting(
      .toolCallStart(id: "call-1", name: "read_file"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .toolCallComplete(
        id: "call-1",
        name: "read_file",
        arguments: #"{"path":"app/Fawx/Models/ChatTranscript.swift"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .toolResult(id: "call-1", output: "enum ChatTranscriptItem { ... }", isError: false),
      sessionID: sessionID
    )

    await sut.reduceStreamEventForTesting(
      .textDelta(
        "I’m locating the chat transcript components first. The key files appear to be ChatTranscript.swift, MessageBubble.swift, ToolCallCard.swift, and ChatViewModel.swift."
      ),
      sessionID: sessionID
    )

    XCTAssertEqual(
      sut.streamingTextForTesting(sessionID: sessionID),
      "",
      "Legacy text after legacy tool events should survive as work narration, not transient final output."
    )

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    let narrationText = summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }
    XCTAssertTrue(
      narrationText.contains {
        $0.contains("The key files appear to be ChatTranscript.swift")
      },
      "The rich legacy narration should be retained in completed work."
    )
    guard case .finalAnswer(let finalAnswer)? = items.last else {
      return XCTFail("expected final answer after completed work")
    }
    XCTAssertEqual(
      finalAnswer.displayText,
      "The transcript UI has partial separation but still needs stronger turn boundaries."
    )
  }

  func testDistinctWorkingNarrationChunksAreNotOverwrittenByLaterProgressNarration() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 138)
    let richNarration =
      "Found the transcript rendering switch — it already has distinct cases for .message, .toolActivityGroup, .completedWorkSummary, and .finalAnswer. Now I’m checking the model definitions and the reducer."
    let progressNarration = "I'm reading local files in Views/Shared/ChatDetailView.swift."
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "The transcript UI has the right primitives but needs stronger completed-work preservation.",
      timestamp: 138
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(richNarration),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(progressNarration, voiceoverSuppressed: true),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-2", title: "Read file", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-2", id: "call-2", name: "read_file"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-2",
        id: "call-2",
        name: "read_file",
        arguments: #"{"path":"app/Fawx/Views/Shared/ChatDetailView.swift"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-2",
        id: "call-2",
        toolName: "read_file",
        output: "switch item { ... }",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-2"), sessionID: sessionID)

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    let narrationText = summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }

    XCTAssertEqual(narrationText, [richNarration])
    XCTAssertFalse(
      narrationText.contains(progressNarration),
      "Tool-summary narration should be represented by the adjacent tool chunk, not preserved as duplicate prose."
    )
    XCTAssertEqual(
      summary.activityGroups.flatMap(\.toolCalls).map(\.id),
      ["call-2"]
    )
  }

  func testToolSummaryNarrationIsFilteredFromCompletedSummaryButToolChunkStays() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 124)
    let toolSummaryNarration =
      "I'm searching fx-kernel/src/act.rs for external_actions_from_argv|argv.*external_action."
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "The external action evidence is handled in the command diagnostics path.",
      timestamp: 124
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(toolSummaryNarration, voiceoverSuppressed: true),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Search act.rs", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "search_text",
        arguments: #"{"path":"fx-kernel/src/act.rs","pattern":"external_actions_from_argv|argv.*external_action"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "search_text",
        output: "matched",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    let narrationText = summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }
    XCTAssertFalse(narrationText.contains(toolSummaryNarration))
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testToolSummaryNarrationIsFilteredFromLiveActivityButToolChunkStays()
    async throws
  {
    let sut = makeSUT()
    let sessionID = "session-a"
    let toolSummaryNarration =
      "I'm searching fx-kernel/src/act.rs for external_actions_from_argv|argv.*external_action."

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta(toolSummaryNarration, voiceoverSuppressed: true),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Search act.rs", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "search_text",
        arguments: #"{"path":"fx-kernel/src/act.rs","pattern":"external_actions_from_argv|argv.*external_action"}"#
      ),
      sessionID: sessionID
    )

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    let group = try XCTUnwrap(groups.first)
    XCTAssertFalse(
      sut.transcriptItems.contains { item in
        guard case .message(let message) = item, message.isWorkingNarration else {
          return false
        }
        return message.displayText.contains("external_actions_from_argv")
      },
      "Low-value narration that repeats the tool target should not render as standalone live prose."
    )
    XCTAssertEqual(group.toolCalls.map(\.id), ["call-1"])
  }

  func testRichNarrationAdjacentToToolSummaryIsPreserved() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 130)
    let richNarration =
      "Now let me check the key logic areas more carefully — the external_action_completed function and the command_posts_github_issue_comment interaction with the legacy PR-comment path:"
    let finalMessage = SessionMessage(
      role: .assistant,
      content: "The PR comment completion path is now typed.",
      timestamp: 130
    )

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )

    await sut.reduceStreamEventForTesting(.workingNarrationDelta(richNarration), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Search action contracts", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "search_text",
        arguments: #"{"path":"engine/crates/fx-kernel/src/loop_engine/mod.rs","pattern":"external_action_completed|command_posts_github_issue_comment"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "search_text",
        output: "matched",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    let narrationText = summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }
    XCTAssertTrue(narrationText.contains(richNarration))
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testMixedToolSummaryAndReasoningNarrationIsPreserved() async throws {
    let mixedNarration =
      "I'm searching fx-kernel/src/act.rs because I need to verify the external action path."
    let summary = try await completedSummaryForToolNarration(
      [mixedNarration],
      arguments: #"{"path":"fx-kernel/src/act.rs","pattern":"external_action"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertTrue(narrationText.contains(mixedNarration))
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testSearchKindNarrationWithoutToolTargetIsPreserved() async throws {
    let reasoningNarration = "I searched the likely cause."
    let summary = try await completedSummaryForToolNarration(
      [reasoningNarration],
      arguments: #"{"path":"app/Fawx/Views","pattern":"MessageBubble"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertTrue(
      narrationText.contains(reasoningNarration),
      "Search-shaped narration should not be filtered unless it echoes the typed tool target."
    )
  }

  func testToolSummaryNarrationFiltersAcrossInterveningWorkingNarration() async throws {
    let toolSummaryNarration =
      "I'm searching fx-kernel/src/act.rs for external_actions_from_argv|argv.*external_action."
    let reasoningNarration = "I need to understand how the evidence contract is represented."
    let summary = try await completedSummaryForToolNarration(
      [
        (text: toolSummaryNarration, voiceoverSuppressed: true),
        (text: reasoningNarration, voiceoverSuppressed: false),
      ],
      arguments: #"{"path":"fx-kernel/src/act.rs","pattern":"external_actions_from_argv|argv.*external_action"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertFalse(narrationText.contains(toolSummaryNarration))
    XCTAssertTrue(narrationText.contains(reasoningNarration))
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testCommandEchoNarrationIsFilteredWhenToolSummaryShowsCommand() async throws {
    let commandNarration = "I ran gh pr view 1863 --json title,body,headRefName."
    let summary = try await completedSummaryForToolNarration(
      [(text: commandNarration, voiceoverSuppressed: true)],
      toolName: "run_command",
      arguments: #"{"command":"gh pr view 1863 --json title,body,headRefName"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertFalse(
      narrationText.contains(commandNarration),
      "Command echoes belong in the tool summary, not in the assistant voiceover lane."
    )
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testSuppressedVoiceoverDoesNotDependOnTargetLength() async throws {
    let shortTargetNarration = "I'm reading app."
    let summary = try await completedSummaryForToolNarration(
      [(text: shortTargetNarration, voiceoverSuppressed: true)],
      toolName: "read_file",
      arguments: #"{"path":"app"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertFalse(
      narrationText.contains(shortTargetNarration),
      "Suppression is a typed stream contract, not a target-length heuristic."
    )
  }

  func testSuppressedVoiceoverDoesNotDependOnKnownArgumentKeys() async throws {
    let narration = "I inspected api."
    let summary = try await completedSummaryForToolNarration(
      [(text: narration, voiceoverSuppressed: true)],
      toolName: "custom_inspect",
      arguments: #"{"resource":"api"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertFalse(
      narrationText.contains(narration),
      "Suppression should not depend on hardcoded tool argument key paths."
    )
  }

  func testUnsuppressedToolShapedNarrationIsPreserved() async throws {
    let narration = "I ran the command because I need to compare the current PR metadata."
    let summary = try await completedSummaryForToolNarration(
      [narration],
      toolName: "run_command",
      arguments: #"{"command":"gh pr view 1863 --json title,body"}"#
    )
    let narrationText = completedSummaryNarrationText(summary)

    XCTAssertTrue(
      narrationText.contains(narration),
      "Tool-shaped prose should remain visible unless the stream explicitly marks it as suppressed."
    )
  }

  func testTypedStreamEventsBuildSeparateNarrationToolAndFinalLanes() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason
    )

    await sut.reduceStreamEventForTesting(
      .textPreviewDelta("I’ll inspect the diff first."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’ll inspect the diff first."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Ran 1 tool", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-1", id: "call-1", name: "run_command"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "run_command",
        arguments: #"{"command":"git diff --stat"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "run_command",
        output: "diff stats",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .toolProgress(
        activityID: "round-1",
        id: "call-1",
        toolName: "run_command",
        category: "observation",
        target: "PR #1834 diff",
        advancesSlot: "evidence:pr:1834",
        outcome: "advanced"
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .toolResult(id: "call-1", output: "legacy duplicate", isError: false),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(
      .completedSummary("Worked this turn: 1 command."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.textDelta("Done."), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.finalAnswerDelta("Done."), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.textDelta(" Legacy final duplicate."), sessionID: sessionID)
    sut.flushStreamingDisplayForTesting()

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(groups.count, 1)
    let toolGroup = try XCTUnwrap(groups.first)
    let toolCall = try XCTUnwrap(toolGroup.toolCalls.first)
    XCTAssertTrue(
      sut.transcriptItems.contains { item in
        guard case .message(let message) = item, message.isWorkingNarration else {
          return false
        }
        return message.displayText == "I’ll inspect the diff first."
      }
    )
    XCTAssertEqual(toolGroup.toolCalls.map(\.id), ["call-1"])
    XCTAssertEqual(toolCall.name, "run_command")
    XCTAssertEqual(toolCall.result, "diff stats")
    XCTAssertEqual(toolCall.progress?.category, "observation")
    XCTAssertEqual(toolCall.progress?.target, "PR #1834 diff")
    XCTAssertEqual(toolCall.progress?.advancesSlot, "evidence:pr:1834")
    XCTAssertEqual(toolCall.progress?.outcome, "advanced")
    XCTAssertFalse(toolGroup.isLive)
    guard case .completedWorkSummary(let completedSummary)? = sut.transcriptItems.first(where: {
      if case .completedWorkSummary = $0 {
        return true
      }
      return false
    }) else {
      return XCTFail("expected live backend completed summary")
    }
    XCTAssertEqual(completedSummary.summaryText, "Worked this turn: 1 command.")
    let snapshot = AssistantActivityTimelineSnapshot(group: toolGroup, isExpanded: true)
    XCTAssertEqual(snapshot.detailStyle, .historicalPayload)
    XCTAssertEqual(snapshot.rows.first?.summary, "Advanced evidence: PR #1834 diff")
    XCTAssertEqual(sut.streamingTextForTesting(sessionID: sessionID), "Done.")
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [
      .workingNarration,
      .toolGroup,
      .completedSummary,
      .finalAnswer,
    ])
    guard case .finalAnswer(let finalAnswer)? = sut.transcriptItems.last else {
      return XCTFail("expected live final answer transcript item")
    }
    XCTAssertEqual(finalAnswer.displayText, "Done.")
    XCTAssertTrue(finalAnswer.isStreaming)
  }

  func testBackendCompletedSummaryPersistsThroughFinalizedTranscript() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 112)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’ll inspect the diff first."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Ran 1 tool", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "run_command",
        arguments: #"{"command":"git diff --stat"}"#
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: "run_command",
        output: "diff stats",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .completedSummary("Worked this turn: inspected the diff and verified one command."),
      sessionID: sessionID
    )

    let assistantMessage = SessionMessage(role: .assistant, content: "Done.", timestamp: 100)
    sut.recordCompletedStreamingFootnoteForTesting(
      assistantMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [assistantMessage]
    )

    XCTAssertEqual(items.map(\.phase), [.completedSummary, .finalAnswer])
    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed summary before final answer")
    }
    XCTAssertEqual(
      summary.summaryText,
      "Worked this turn: inspected the diff and verified one command."
    )
    XCTAssertEqual(summary.elapsedText, "Worked for 12 seconds")
    XCTAssertEqual(summary.activityGroups.flatMap(\.toolCalls).map(\.id), ["call-1"])
  }

  func testBackendCompletedSummaryLatestEventWins() async {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason
    )

    await sut.reduceStreamEventForTesting(
      .completedSummary("Worked this turn: initial summary."),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .completedSummary("Worked this turn: revised summary."),
      sessionID: sessionID
    )

    let summaries = sut.transcriptItems.compactMap { item -> CompletedWorkSummaryRecord? in
      guard case .completedWorkSummary(let summary) = item else {
        return nil
      }
      return summary
    }

    XCTAssertEqual(summaries.map(\.summaryText), ["Worked this turn: revised summary."])
  }

  func testTypedFinalAnswerPromotesMatchingPreviewOutOfWorkLog() async {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason
    )

    await sut.reduceStreamEventForTesting(.textPreviewDelta("Done."), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.workingNarrationDelta("Done."), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.textDelta("Done."), sessionID: sessionID)
    await sut.reduceStreamEventForTesting(.finalAnswerDelta("Done."), sessionID: sessionID)
    sut.flushStreamingDisplayForTesting()

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }
    XCTAssertEqual(groups, [])
    XCTAssertEqual(sut.streamingTextForTesting(sessionID: sessionID), "Done.")
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.finalAnswer])
  }

  func testTypedFinalAnswerStreamingUsesSnapScrollWhenPinned() async {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .other("respond")
    )

    await sut.reduceStreamEventForTesting(.finalAnswerDelta("Done."), sessionID: sessionID)
    sut.flushStreamingDisplayForTesting()

    XCTAssertEqual(sut.streamingTextForTesting(sessionID: sessionID), "Done.")
    XCTAssertEqual(sut.pendingTranscriptScrollBehavior, .snap)
  }

  func testTranscriptPhaseOrderAllowsWorkBeforeTerminalBoundary() {
    let narration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working-1",
        message: SessionMessage(role: .assistant, content: "I’m checking the reducer first.", timestamp: 1),
        displayText: "I’m checking the reducer first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let summary = ChatTranscriptItem.completedWorkSummary(
      CompletedWorkSummaryRecord(
        id: "assistant-1",
        elapsedText: "Worked for 12 seconds",
        activityGroups: []
      )
    )
    let finalAnswer = ChatTranscriptItem.finalAnswer(
      TranscriptMessage(
        id: "assistant-1",
        message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
        displayText: "Done.",
        footnoteText: nil
      )
    )

    XCTAssertNil([narration, summary, finalAnswer].firstTranscriptPhaseOrderViolation())
  }

  func testTranscriptPhaseOrderRejectsWorkAfterTerminalBoundaryUntilNextUserMessage() throws {
    let finalAnswer = ChatTranscriptItem.finalAnswer(
      TranscriptMessage(
        id: "assistant-1",
        message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
        displayText: "Done.",
        footnoteText: nil
      )
    )
    let toolGroup = ChatTranscriptItem.toolActivityGroup(
      ToolActivityGroupRecord(
        id: "tool-group-1",
        toolCalls: [
          ToolCallRecord(
            id: "call-1",
            name: "read_file",
            arguments: "{\"path\":\"README.md\"}",
            result: nil,
            isRunning: true,
            isError: false
          )
        ],
        isLive: true
      )
    )
    let userMessage = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "user-2",
        message: SessionMessage(role: .user, content: "Follow up", timestamp: 3),
        displayText: "Follow up",
        footnoteText: nil
      )
    )

    let violation = try XCTUnwrap([finalAnswer, toolGroup].firstTranscriptPhaseOrderViolation())
    XCTAssertEqual(violation.terminalItemID, "final-answer:assistant-1")
    XCTAssertEqual(violation.laterWorkingItemID, "tool-group:tool-group-1")
    XCTAssertEqual(violation.laterWorkingPhase, .toolGroup)
    XCTAssertNil([finalAnswer, userMessage, toolGroup].firstTranscriptPhaseOrderViolation())
  }

  func testTranscriptTurnsGroupAssistantWorkIntoTypedSlots() throws {
    let assistantMessage = SessionMessage(role: .assistant, content: "I’m checking first.", timestamp: 1)
    let workingNarration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working",
        message: assistantMessage,
        displayText: "I’m checking first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let toolGroup = ChatTranscriptItem.toolActivityGroup(
      ToolActivityGroupRecord(
        id: "tool-group-1",
        toolCalls: [
          ToolCallRecord(
            id: "call-1",
            name: "read_file",
            arguments: "{\"path\":\"ChatViewModel.swift\"}",
            result: "source",
            isRunning: false,
            isError: false
          )
        ],
        isLive: false
      )
    )
    let summary = ChatTranscriptItem.completedWorkSummary(
      CompletedWorkSummaryRecord(
        id: "summary-1",
        elapsedText: "Worked for 12 seconds",
        activityGroups: []
      )
    )
    let finalAnswer = ChatTranscriptItem.finalAnswer(
      TranscriptMessage(
        id: "assistant-final",
        message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
        displayText: "Done.",
        footnoteText: nil
      )
    )

    let turns = [workingNarration, toolGroup, summary, finalAnswer].transcriptTurns()

    guard case .assistant(let turn)? = turns.first else {
      return XCTFail("expected assistant transcript turn")
    }
    XCTAssertEqual(turn.workingNarration.map(\.text), ["I’m checking first."])
    XCTAssertEqual(turn.toolGroups.map(\.id), ["tool-group-1"])
    XCTAssertEqual(turn.completedSummary?.elapsedText, "Worked for 12 seconds")
    XCTAssertEqual(turn.finalAnswer?.displayText, "Done.")
  }

  func testTranscriptTurnsPreserveInterleavedNarrationAndToolOrder() throws {
    let firstNarration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working-1",
        message: SessionMessage(role: .assistant, content: "I’m checking first.", timestamp: 1),
        displayText: "I’m checking first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let firstToolGroup = ChatTranscriptItem.toolActivityGroup(
      ToolActivityGroupRecord(
        id: "tool-group-1",
        toolCalls: [
          ToolCallRecord(
            id: "call-1",
            name: "list_dir",
            arguments: "{\"path\":\"app\"}",
            result: "files",
            isRunning: false,
            isError: false
          )
        ],
        isLive: true
      )
    )
    let secondNarration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working-2",
        message: SessionMessage(role: .assistant, content: "I found the app; now I’m reading files.", timestamp: 2),
        displayText: "I found the app; now I’m reading files.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let secondToolGroup = ChatTranscriptItem.toolActivityGroup(
      ToolActivityGroupRecord(
        id: "tool-group-2",
        toolCalls: [
          ToolCallRecord(
            id: "call-2",
            name: "read_file",
            arguments: "{\"path\":\"ChatDetailView.swift\"}",
            result: "source",
            isRunning: false,
            isError: false
          )
        ],
        isLive: true
      )
    )

    let turns = [firstNarration, firstToolGroup, secondNarration, secondToolGroup].transcriptTurns()

    guard case .assistant(let turn)? = turns.first else {
      return XCTFail("expected assistant transcript turn")
    }
    XCTAssertEqual(
      turn.chunks.map(\.id),
      [
        "narration:message:assistant-working-1",
        "tool-group:tool-group-1",
        "narration:message:assistant-working-2",
        "tool-group:tool-group-2",
      ]
    )
  }

  func testTranscriptTurnsPreserveStandaloneUserBoundary() throws {
    let assistantNarration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working",
        message: SessionMessage(role: .assistant, content: "I’m checking first.", timestamp: 1),
        displayText: "I’m checking first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let userMessage = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "user-follow-up",
        message: SessionMessage(role: .user, content: "Follow up", timestamp: 2),
        displayText: "Follow up",
        footnoteText: nil
      )
    )
    let plainAssistantMessage = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-plain",
        message: SessionMessage(role: .assistant, content: "Plain response.", timestamp: 3),
        displayText: "Plain response.",
        footnoteText: nil
      )
    )

    let turns = [assistantNarration, userMessage, plainAssistantMessage].transcriptTurns()

    XCTAssertEqual(turns.count, 3)
    guard case .assistant(let firstTurn) = turns[0] else {
      return XCTFail("expected first assistant turn")
    }
    XCTAssertEqual(firstTurn.workingNarration.map(\.text), ["I’m checking first."])
    guard case .standalone(let userItem) = turns[1],
          case .message(let userTranscript) = userItem else {
      return XCTFail("expected standalone user message")
    }
    XCTAssertEqual(userTranscript.displayText, "Follow up")
    guard case .standalone(let assistantItem) = turns[2],
          case .message(let assistantTranscript) = assistantItem else {
      return XCTFail("expected standalone plain assistant message")
    }
    XCTAssertEqual(assistantTranscript.displayText, "Plain response.")
  }

  func testTranscriptTurnsExposeCurrentTurnTerminalAssistantOutput() throws {
    let workingNarration = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "assistant-working",
        message: SessionMessage(role: .assistant, content: "I’m checking first.", timestamp: 1),
        displayText: "I’m checking first.",
        footnoteText: nil,
        isWorkingNarration: true
      )
    )
    let finalAnswer = ChatTranscriptItem.finalAnswer(
      TranscriptMessage(
        id: "assistant-final",
        message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
        displayText: "Done.",
        footnoteText: nil
      )
    )
    let userMessage = ChatTranscriptItem.message(
      TranscriptMessage(
        id: "user-follow-up",
        message: SessionMessage(role: .user, content: "Follow up", timestamp: 3),
        displayText: "Follow up",
        footnoteText: nil
      )
    )

    XCTAssertFalse([workingNarration].transcriptTurns().hasCurrentTurnTerminalAssistantOutput)
    XCTAssertTrue([workingNarration, finalAnswer].transcriptTurns().hasCurrentTurnTerminalAssistantOutput)
    XCTAssertFalse(
      [workingNarration, finalAnswer, userMessage].transcriptTurns()
        .hasCurrentTurnTerminalAssistantOutput
    )
  }

  func testAssistantTranscriptTurnReducerReportsTerminalWorkInversion() throws {
    let finalAnswer = ChatTranscriptItem.finalAnswer(
      TranscriptMessage(
        id: "assistant-final",
        message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
        displayText: "Done.",
        footnoteText: nil
      )
    )
    let lateToolGroup = ChatTranscriptItem.toolActivityGroup(
      ToolActivityGroupRecord(
        id: "late-tool-group",
        toolCalls: [
          ToolCallRecord(
            id: "call-1",
            name: "read_file",
            arguments: "{\"path\":\"ChatViewModel.swift\"}",
            result: "source",
            isRunning: false,
            isError: false
          )
        ],
        isLive: false
      )
    )

    let reduction = AssistantTranscriptTurnReducer.reduce([finalAnswer, lateToolGroup])

    let violation = try XCTUnwrap(reduction.phaseOrderViolation)
    XCTAssertEqual(violation.terminalItemID, "final-answer:assistant-final")
    XCTAssertEqual(violation.laterWorkingItemID, "tool-group:late-tool-group")
    XCTAssertEqual(violation.laterWorkingPhase, .toolGroup)
  }

  func testAssistantTranscriptTurnLifecycleAndPresentationContracts() throws {
    let liveNarration = WorkingNarrationRecord(
      id: "narration-live",
      text: "I’m checking the reducer.",
      isLive: true
    )
    let completedToolGroup = ToolActivityGroupRecord(
      id: "tool-group-1",
      toolCalls: [
        ToolCallRecord(
          id: "call-1",
          name: "read_file",
          arguments: "{\"path\":\"ChatTranscript.swift\"}",
          result: "source",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )
    let liveToolGroup = ToolActivityGroupRecord(
      id: "tool-group-2",
      toolCalls: [
        ToolCallRecord(
          id: "call-2",
          name: "run_command",
          arguments: "{\"command\":\"rg transcript app/Fawx\"}",
          result: nil,
          isRunning: true,
          isError: false
        )
      ],
      isLive: true
    )
    let summary = CompletedWorkSummaryRecord(
      id: "summary-1",
      elapsedText: "Worked for 12 seconds",
      activityGroups: [completedToolGroup]
    )
    let finalAnswer = TranscriptMessage(
      id: "assistant-final",
      message: SessionMessage(role: .assistant, content: "Done.", timestamp: 2),
      displayText: "Done.",
      footnoteText: nil
    )

    let collectingTurn = AssistantTranscriptTurn(
      id: "turn-1",
      chunks: [.narration(liveNarration), .toolActivity(liveToolGroup)],
      completedSummary: nil,
      finalAnswer: nil
    )
    XCTAssertEqual(collectingTurn.lifecycle, .collectingWork)
    XCTAssertEqual(
      collectingTurn.chunks.map { $0.presentation(in: collectingTurn.lifecycle).defaultExpanded },
      [true, true]
    )
    XCTAssertEqual(
      collectingTurn.chunks.map { $0.presentation(in: collectingTurn.lifecycle).shouldCollapseOnComplete },
      [false, true]
    )

    let completedTurn = AssistantTranscriptTurn(
      id: "turn-2",
      chunks: [.narration(liveNarration), .toolActivity(completedToolGroup)],
      completedSummary: summary,
      finalAnswer: finalAnswer
    )
    XCTAssertEqual(completedTurn.lifecycle, .completed)
    XCTAssertEqual(
      completedTurn.chunks.map { $0.presentation(in: completedTurn.lifecycle).defaultExpanded },
      [true, false]
    )
    XCTAssertEqual(
      completedTurn.chunks.map { $0.presentation(in: completedTurn.lifecycle).shouldCollapseOnComplete },
      [false, true]
    )
  }

  func testDoneResponsePromotionRemovesMatchingPreviewFromCompletedWorkSummary() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let message = SessionMessage(role: .assistant, content: "Done.", timestamp: 142)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )
    sut.appendActivityNarrationForTesting("Done.", sessionID: sessionID)
    sut.removePromotedPreviewNarrationForTesting("Done.", sessionID: sessionID)
    sut.recordCompletedStreamingFootnoteForTesting(
      message,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [message])
    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary before final answer")
    }

    XCTAssertEqual(summary.elapsedText, "Worked for 42 seconds")
    XCTAssertEqual(summary.activityGroups, [])

    guard case .finalAnswer(let finalMessage)? = items.last else {
      return XCTFail("expected final assistant message")
    }
    XCTAssertEqual(finalMessage.message.content, "Done.")
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

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: false)

    XCTAssertEqual(snapshot.detailStyle, .collapsed)
    XCTAssertFalse(snapshot.isExpanded)
    XCTAssertEqual(snapshot.headerTitle, "Reading README.md")
    XCTAssertEqual(snapshot.visibleToolCalls, [])
    XCTAssertEqual(snapshot.accessibilityHint, "Expand activity")
  }

  func testCompletedToolActivitySnapshotUsesCodexStyleAggregateSummary() {
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
        ),
        ToolCallRecord(
          id: "call_2",
          name: "search_files",
          arguments: "{\"pattern\":\"TODO\"}",
          result: "matches",
          isRunning: false,
          isError: false
        ),
        ToolCallRecord(
          id: "call_3",
          name: "run_command",
          arguments: "{\"command\":\"git diff --stat\"}",
          result: "diff stats",
          isRunning: false,
          isError: false
        ),
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: false)

    XCTAssertEqual(snapshot.headerTitle, "Explored 1 file, 1 search, ran 1 command")
  }

  func testCompletedToolActivitySnapshotPluralizesSearches() {
    let group = ToolActivityGroupRecord(
      id: "history-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "search_files",
          arguments: "{\"pattern\":\"TODO\"}",
          result: "matches",
          isRunning: false,
          isError: false
        ),
        ToolCallRecord(
          id: "call_2",
          name: "grep",
          arguments: "{\"pattern\":\"FIXME\"}",
          result: "matches",
          isRunning: false,
          isError: false
        ),
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: false)
    let completedChunk = CompletedWorkChunkSnapshot(group: group)

    XCTAssertEqual(snapshot.headerTitle, "2 searches")
    XCTAssertEqual(completedChunk.toolTitle, "Ran 2 searches")
  }

  func testToolActivitySnapshotDoesNotSynthesizeToolEchoNarrationWhenMissing() {
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

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: false)
    let completedChunk = CompletedWorkChunkSnapshot(group: group)

    XCTAssertEqual(snapshot.headerTitle, "Read README.md")
    XCTAssertEqual(completedChunk.toolTitle, "Read README.md")
  }

  func testRunningToolActivitySnapshotSummarizesCompletedWorkWithoutPlusCount() {
    let group = ToolActivityGroupRecord(
      id: "live-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "read_file",
          arguments: "{\"path\":\"README.md\"}",
          result: "contents",
          isRunning: false,
          isError: false
        ),
        ToolCallRecord(
          id: "call_2",
          name: "run_command",
          arguments: "{\"command\":\"gh pr view 1837\"}",
          result: nil,
          isRunning: true,
          isError: false
        ),
      ],
      isLive: true
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: false)

    XCTAssertEqual(snapshot.headerTitle, "Explored 1 file")
    XCTAssertFalse(snapshot.headerTitle.contains("+1"))
  }

  func testLiveToolActivitySnapshotNarratesPartialCommandArguments() {
    let group = ToolActivityGroupRecord(
      id: "live-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: #"{"command":"cargo test -p fx-api"#,
          result: nil,
          isRunning: true,
          isError: false
        )
      ],
      isLive: true
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)

    XCTAssertEqual(snapshot.headerTitle, "Running cargo test -p fx-api")
    XCTAssertEqual(snapshot.groupSummary, "cargo test -p fx-api")
    XCTAssertEqual(snapshot.rows.first?.summary, "cargo test -p fx-api")
  }

  func testHistoricalToolActivitySnapshotUsesPayloadDetails() throws {
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

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)

    XCTAssertEqual(snapshot.detailStyle, .historicalPayload)
    XCTAssertTrue(snapshot.showsPayloadDetails)
    XCTAssertEqual(snapshot.visibleToolCalls.map(\.arguments), ["{\"path\":\"README.md\"}"])
    XCTAssertEqual(snapshot.accessibilityHint, "Collapse activity")
  }

  func testCodeMutationActivityDetailsRenderDiffOnly() throws {
    let group = ToolActivityGroupRecord(
      id: "history-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "edit_file",
          arguments: """
            {"path":"Sources/App.swift","old_text":"let title = \\"Old\\"","new_text":"let title = \\"New\\""}
            """,
          result: "Successfully edited Sources/App.swift (lines 1-1)",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)
    let row = try XCTUnwrap(snapshot.rows.first)

    XCTAssertEqual(row.detailSections.map(\.title), ["Diff"])
    XCTAssertEqual(row.detailSections.first?.language, "diff")
    XCTAssertTrue(row.detailSections.first?.content.contains("--- a/Sources/App.swift") == true)
    XCTAssertTrue(row.detailSections.first?.content.contains(#"-let title = "Old""#) == true)
    XCTAssertTrue(row.detailSections.first?.content.contains(#"+let title = "New""#) == true)
    XCTAssertFalse(row.detailSections.contains { $0.title == "Inputs" })
    XCTAssertFalse(row.detailSections.contains { $0.content.contains("Successfully edited") })
  }

  func testCodeMutationActivityDetailsSuppressRawPayloadWhenDiffCannotBeRendered() throws {
    let group = ToolActivityGroupRecord(
      id: "history-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "write_file",
          arguments: "{\"path\":\"Sources/App.swift\"}",
          result: "wrote 128 bytes to Sources/App.swift",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)
    let row = try XCTUnwrap(snapshot.rows.first)

    XCTAssertEqual(row.summary, "Code change recorded")
    XCTAssertTrue(row.detailSections.isEmpty)
  }

  func testApplyPatchActivityDetailsRenderPatchAsDiffOnly() throws {
    let patch = """
      *** Begin Patch
      *** Update File: Sources/App.swift
      @@
      -let title = "Old"
      +let title = "New"
      *** End Patch
      """
    let group = ToolActivityGroupRecord(
      id: "history-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "apply_patch",
          arguments: patch,
          result: "Success. Updated the following files:\nM Sources/App.swift",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)
    let row = try XCTUnwrap(snapshot.rows.first)

    XCTAssertEqual(row.summary, "Diff available")
    XCTAssertTrue(row.hasDetails)
    XCTAssertEqual(row.detailSections.map(\.title), ["Diff"])
    XCTAssertEqual(row.detailSections.first?.language, "diff")
    XCTAssertTrue(row.detailSections.first?.content.contains("*** Begin Patch") == true)
    XCTAssertTrue(row.detailSections.first?.content.contains(#"+let title = "New""#) == true)
    XCTAssertFalse(row.detailSections.contains { $0.title == "Inputs" })
    XCTAssertFalse(row.detailSections.contains { $0.content.contains("Success. Updated") })
  }

  func testRunCommandActivityDetailsRenderNestedShellTranscript() throws {
    let group = ToolActivityGroupRecord(
      id: "history-session-a",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: "{\"command\":\"git diff --stat\"}",
          result: "README.md | 2 +-",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )

    let snapshot = AssistantActivityTimelineSnapshot(group: group, isExpanded: true)
    let row = try XCTUnwrap(snapshot.rows.first)

    XCTAssertEqual(row.detailSections.map(\.title), ["Shell"])
    XCTAssertEqual(row.detailSections.first?.language, "shell")
    XCTAssertEqual(row.detailSections.first?.content, "$ git diff --stat\nREADME.md | 2 +-")
  }

  func testFetchedHistoryReplacesMatchingLiveToolOverlayInsteadOfDuplicatingIt() throws {
    let sut = makeSUT()
    let optimisticAssistant = SessionMessage(
      role: .assistant, content: "Let me check.", timestamp: 1)
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

    sut.applyFetchedMessagesForTesting(
      [historicalAssistant, historicalToolResult], sessionID: "session-a")

    let groups = sut.transcriptItems.compactMap { item -> ToolActivityGroupRecord? in
      guard case .toolActivityGroup(let group) = item else {
        return nil
      }
      return group
    }

    XCTAssertEqual(groups.count, 1)
    let group = try XCTUnwrap(groups.first)
    let toolCall = try XCTUnwrap(group.toolCalls.first)
    XCTAssertEqual(toolCall.id, "call_1")
    XCTAssertEqual(toolCall.result, "file contents")
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

  func testDraftThreadModelDoesNotLeakIntoSelectedSession() async {
    let context = makeThreadModelSUT()
    let sut = context.chatViewModel

    sut.prepareToDisplaySession(nil)
    await sut.selectModelForCurrentThread("draft-model")

    XCTAssertEqual(sut.selectedThreadModelID, "draft-model")

    sut.prepareToDisplaySession("session-a")

    XCTAssertEqual(sut.selectedThreadModelID, "active-model")
  }

  func testSelectingModelForStreamingThreadShowsToast() async {
    let context = makeThreadModelSUT()
    let sut = context.chatViewModel

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )

    await sut.selectModelForCurrentThread("draft-model")

    XCTAssertEqual(
      context.appState.toast?.message,
      "Stop this response before changing the thread model."
    )
    XCTAssertEqual(context.appState.toast?.style, .warning)
    XCTAssertEqual(sut.selectedThreadModelID, "active-model")
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
    XCTAssertEqual(sut.composerPhaseLabel, "Thinking")

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
    XCTAssertNil(sut.composerPhaseLabel)
  }

  func testStreamingFinalResponseStateDistinguishesWorkFromAnswer() {
    let sut = makeSUT()

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      progress: ChatViewModel.StreamingProgress(
        kind: .researching,
        message: "Checking the code path."
      )
    )

    XCTAssertTrue(sut.isCurrentSessionStreaming)
    XCTAssertFalse(sut.isCurrentSessionStreamingFinalResponse)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      hasTypedFinalAnswerEvents: true
    )

    XCTAssertTrue(sut.isCurrentSessionStreamingFinalResponse)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      streamingText: "Final answer is streaming."
    )

    XCTAssertTrue(sut.isCurrentSessionStreamingFinalResponse)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      transcriptPhase: .finalizing
    )

    XCTAssertTrue(sut.isCurrentSessionStreamingFinalResponse)
  }

  func testPreviewTextStreamsAsCandidateFinalAnswerUntilReset() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      progress: ChatViewModel.StreamingProgress(
        kind: .researching,
        message: "Researching the request and planning the next step..."
      )
    )

    await sut.reduceStreamEventForTesting(
      .textPreviewDelta("Current Architecture Assessment"),
      sessionID: sessionID
    )
    sut.flushStreamingDisplayForTesting()

    XCTAssertEqual(sut.visibleStreamingText, "Current Architecture Assessment")
    XCTAssertTrue(sut.isCurrentSessionStreamingFinalResponse)
    XCTAssertTrue(sut.transcriptTurns.hasCurrentTurnTerminalAssistantOutput)
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.finalAnswer])
  }

  func testPreviewTextResetDemotesCandidateAnswerToWorkingNarration() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID
    )

    await sut.reduceStreamEventForTesting(
      .textPreviewDelta("I’m locating the transcript components first."),
      sessionID: sessionID
    )
    sut.flushStreamingDisplayForTesting()
    await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)

    XCTAssertEqual(sut.visibleStreamingText, "")
    XCTAssertFalse(sut.isCurrentSessionStreamingFinalResponse)
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.workingNarration])
    guard case .assistant(let turn)? = sut.transcriptTurns.first else {
      return XCTFail("expected preview text to demote into an assistant work turn")
    }
    XCTAssertEqual(turn.workingNarration.map(\.text), [
      "I’m locating the transcript components first."
    ])
    XCTAssertNil(turn.finalAnswer)
  }

  func testWorkingNarrationDeltaPromotesMatchingPreviewOutOfFinalLane() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID
    )

    await sut.reduceStreamEventForTesting(
      .textPreviewDelta("I’ll inspect the diff first."),
      sessionID: sessionID
    )
    sut.flushStreamingDisplayForTesting()
    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I’ll inspect the diff first."),
      sessionID: sessionID
    )

    XCTAssertEqual(sut.visibleStreamingText, "")
    XCTAssertEqual(sut.transcriptItems.map(\.phase), [.workingNarration])
    guard case .assistant(let turn)? = sut.transcriptTurns.first else {
      return XCTFail("expected matching preview to become working narration")
    }
    XCTAssertEqual(turn.workingNarration.map(\.text), [
      "I’ll inspect the diff first."
    ])
    XCTAssertNil(turn.finalAnswer)
  }

  func testFinalizingBoundaryClearsStaleWorkingProgress() async throws {
    let sut = makeSUT()
    let sessionID = "session-a"

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      progress: ChatViewModel.StreamingProgress(
        kind: .researching,
        message: "Researching the request and planning the next step..."
      )
    )

    await sut.reduceStreamEventForTesting(
      .transcriptPhaseBoundary("finalizing"),
      sessionID: sessionID
    )

    XCTAssertNil(sut.visibleProgress)
    XCTAssertTrue(sut.isCurrentSessionStreamingFinalResponse)
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

  func testVisibleProgressOverridesComposerLabelWhenStreaming() {
    let sut = makeSUT()

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      progress: ChatViewModel.StreamingProgress(
        kind: .implementing,
        message: "Implementing the committed plan."
      )
    )

    XCTAssertEqual(
      sut.visibleProgress,
      ChatViewModel.StreamingProgress(
        kind: .implementing,
        message: "Implementing the committed plan."
      )
    )
    XCTAssertEqual(sut.composerPhaseLabel, "Implementing")
    XCTAssertEqual(sut.visibleStreamingText, "")
  }

  func testResearchingProgressUsesNeutralWorkingCopy() {
    XCTAssertEqual(ChatViewModel.StreamingProgressKind(rawValue: "researching").label, "Working")
  }

  func testVisibleStreamingElapsedTextAppearsAfterThreshold() {
    let sut = makeSUT()
    let startedAt = Date(timeIntervalSince1970: 100)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      progress: ChatViewModel.StreamingProgress(
        kind: .researching,
        message: "Reading local files in skills/"
      ),
      startedAt: startedAt
    )

    XCTAssertNil(sut.visibleStreamingElapsedText(now: Date(timeIntervalSince1970: 114)))
    XCTAssertEqual(
      sut.visibleStreamingElapsedText(now: Date(timeIntervalSince1970: 195)),
      "Worked for 1 minute, 35 seconds"
    )
  }

  func testCompletedStreamingSummaryRendersBeforeFinalAssistantTranscriptItem() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 195)
    let message = SessionMessage(role: .assistant, content: "done", timestamp: 195)

    sut.recordCompletedStreamingFootnoteForTesting(
      message,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [message])
    XCTAssertEqual(items.count, 2)

    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    XCTAssertEqual(summary.elapsedText, "Worked for 1 minute, 35 seconds")
    XCTAssertEqual(summary.activityGroups, [])

    guard case .finalAnswer(let transcriptMessage)? = items.last else {
      return XCTFail("expected final message transcript item")
    }
    XCTAssertNil(transcriptMessage.footnoteText)
  }

  func testCompletedStreamingSummarySurvivesFetchedTimestampChange() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let optimisticMessage = SessionMessage(role: .assistant, content: "Done.", timestamp: 100)
    let fetchedMessage = SessionMessage(role: .assistant, content: "Done.", timestamp: 142)

    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: #"{"command":"git diff --stat"}"#,
          result: "README.md | 2 +-",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )
    sut.recordCompletedStreamingFootnoteForTesting(
      optimisticMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [fetchedMessage])

    XCTAssertEqual(items.count, 2)
    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    XCTAssertEqual(summary.elapsedText, "Worked for 42 seconds")
    XCTAssertEqual(summary.activityGroups.count, 1)
    XCTAssertEqual(summary.activityGroups[0].toolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(summary.activityGroups[0].toolCalls[0].result, "README.md | 2 +-")
  }

  func testCompletedStreamingSummaryPrefersStoredActivityWhenFetchedMessageContainsHistoricalToolBlocks() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let optimisticMessage = SessionMessage(role: .assistant, content: "Final answer.", timestamp: 100)
    let fetchedMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("**Historical narration should not become completed work.**"),
        .toolUse(
          id: "call_1",
          name: "run_command",
          input: .object(["command": .string("git diff --stat")])
        ),
        .text("Final answer."),
      ],
      timestamp: 142
    )
    let fetchedToolResult = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("historical result"), isError: false)
      ],
      timestamp: 143
    )

    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "Typed live narration.",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: #"{"command":"git diff --stat"}"#,
          result: "typed result",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )
    sut.recordCompletedStreamingFootnoteForTesting(
      optimisticMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [fetchedMessage, fetchedToolResult]
    )

    XCTAssertEqual(items.count, 2)
    guard case .completedWorkSummary(let summary)? = items.first else {
      return XCTFail("expected completed work summary item")
    }
    XCTAssertEqual(summary.entries.count, 2)
    guard case .narration(let narration) = summary.entries[0] else {
      return XCTFail("expected stored live narration")
    }
    XCTAssertEqual(narration.text, "Typed live narration.")
    guard case .toolActivityGroup(let toolGroup) = summary.entries[1] else {
      return XCTFail("expected stored live tool group")
    }
    XCTAssertEqual(toolGroup.toolCalls[0].result, "typed result")

    guard case .finalAnswer(let finalMessage)? = items.last else {
      return XCTFail("expected final assistant message")
    }
    XCTAssertEqual(finalMessage.displayText, "Final answer.")
  }

  func testCompletedStreamingSummaryRetainsLiveNarrationAfterHistoryReconciliation() {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 142)
    let initialUserMessage = SessionMessage(role: .user, content: "Review PR 1846", timestamp: 90)
    let fetchedToolMessage = SessionMessage(
      role: .assistant,
      contentBlocks: [
        .text("Historical narration should not replace the live voiceover."),
        .toolUse(
          id: "call_1",
          name: "run_command",
          input: .object(["command": .string("gh pr diff 1846")])
        ),
      ],
      timestamp: 120
    )
    let fetchedToolResult = SessionMessage(
      role: .tool,
      contentBlocks: [
        .toolResult(toolUseId: "call_1", content: .string("diff output"), isError: false)
      ],
      timestamp: 121
    )
    let finalAnswer = SessionMessage(
      role: .assistant,
      content: "Comment posted to PR #1846.",
      timestamp: 142
    )

    sut.cacheMessages([initialUserMessage], for: sessionID)
    sut.prepareToDisplaySession(sessionID)
    sut.setLiveToolGroupForTesting(
      sessionID: sessionID,
      narration: "I inspected the PR diff and checked the follow-up commit.",
      toolCalls: [
        ToolCallRecord(
          id: "call_1",
          name: "run_command",
          arguments: #"{"command":"gh pr diff 1846"}"#,
          result: "typed diff output",
          isRunning: false,
          isError: false
        )
      ],
      isLive: false
    )

    sut.applyFetchedMessagesForTesting(
      [initialUserMessage, fetchedToolMessage, fetchedToolResult],
      sessionID: sessionID
    )
    sut.recordCompletedStreamingFootnoteForTesting(
      finalAnswer,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: sessionID,
      messages: [initialUserMessage, fetchedToolMessage, fetchedToolResult, finalAnswer]
    )

    guard case .completedWorkSummary(let summary)? = items.first(where: { item in
      if case .completedWorkSummary = item {
        return true
      }
      return false
    }) else {
      return XCTFail("Expected completed work summary")
    }
    XCTAssertEqual(summary.entries.count, 2)
    guard case .narration(let narration) = summary.entries[0] else {
      return XCTFail("Expected retained live narration")
    }
    XCTAssertEqual(narration.text, "I inspected the PR diff and checked the follow-up commit.")
    guard case .toolActivityGroup(let toolGroup) = summary.entries[1] else {
      return XCTFail("Expected retained live tool group")
    }
    XCTAssertEqual(toolGroup.toolCalls.map(\.id), ["call_1"])
    XCTAssertEqual(toolGroup.toolCalls.first?.result, "typed diff output")
  }

  func testAssistantMessageTimestampUsesStreamingStartTime() {
    let sut = makeSUT()

    XCTAssertEqual(
      sut.assistantMessageTimestampForTesting(
        startedAt: Date(timeIntervalSince1970: 100.9),
        fallbackUnixTimestamp: 195
      ),
      100
    )
    XCTAssertEqual(
      sut.assistantMessageTimestampForTesting(
        startedAt: nil,
        fallbackUnixTimestamp: 195
      ),
      195
    )
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

  func testTextResetClearsFlushedAndPendingPreviewTokens() {
    let sut = makeSUT()

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      streamingText: "preview"
    )

    sut.appendStreamingTokenForTesting(" pending")
    sut.resetStreamingTextForTesting(sessionID: "session-a")
    sut.flushStreamingDisplayForTesting()

    XCTAssertEqual(sut.streamingTextForTesting(sessionID: "session-a"), "")
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

  func testQueuedMessageSteeringCanBeEditedBeforeDelivery() {
    let sut = makeSUT(connectionStatus: .connected)

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "follow up"

    sut.sendDraft()

    XCTAssertEqual(sut.queuedMessage, "follow up")
    XCTAssertEqual(sut.draftSteering, "")

    sut.draftSteering = "answer in bullets"

    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertEqual(delivery?.text, "follow up")
    XCTAssertEqual(delivery?.steering, "answer in bullets")
    XCTAssertEqual(delivery?.sessionID, "session-a")
  }

  func testQueuedMessageKeepsInitialSteeringVisibleAfterQueueing() {
    let sut = makeSUT(connectionStatus: .connected)

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "follow up"
    sut.draftSteering = "keep it terse"

    sut.sendDraft()

    XCTAssertEqual(sut.queuedMessage, "follow up")
    XCTAssertEqual(sut.draftSteering, "keep it terse")

    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertEqual(delivery?.steering, "keep it terse")
  }

  func testQueuedMessageSteerToggleSendsLiveSteerAndStopsQueuedDelivery() async throws {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()

    XCTAssertEqual(sut.queuedMessage, "focus on the reducer")
    XCTAssertFalse(sut.queuedMessageIsSteering)

    sut.toggleQueuedMessageSteering()

    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageCleared(on: sut)

    XCTAssertNil(sut.queuedMessage)
    let steerRequest = try XCTUnwrap(
      MockChatURLProtocol.recordedRequests().first { request in
        request.httpMethod == "POST" && request.url?.path == "/v1/sessions/session-a/steer"
      }
    )
    let body = try XCTUnwrap(steerRequest.bodyDataForTesting())
    let payload = try JSONSerialization.jsonObject(with: body) as? [String: Any]
    XCTAssertEqual(payload?["text"] as? String, "focus on the reducer")

    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertNil(delivery)
  }

  func testAcceptedQueuedSteeringPopsQueuedDraftAndRemainsVisibleInTranscript() async throws {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageCleared(on: sut)

    XCTAssertNil(sut.queuedMessage)
    XCTAssertNil(sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a"))

    XCTAssertTrue(
      visibleTranscriptText(on: sut).contains("focus on the reducer"),
      "Accepted steering must remain visible in transcript state after it stops being a queued message."
    )
  }

  func testRapidAcceptedSteeringWithSameTextKeepsDistinctTranscriptRecords() async throws {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )

    for expectedRequestCount in 1...2 {
      sut.draftMessage = "focus on the reducer"
      sut.sendDraft()
      sut.toggleQueuedMessageSteering()
      await waitForRecordedRequestCount(
        expectedRequestCount,
        method: "POST",
        path: "/v1/sessions/session-a/steer"
      )
      await waitForQueuedMessageCleared(on: sut)
    }

    let steeringRecords = sut.transcriptItems.compactMap { item -> TurnSteeringRecord? in
      guard case .turnSteering(let record) = item else {
        return nil
      }
      return record
    }

    XCTAssertEqual(steeringRecords.map(\.text), [
      "focus on the reducer",
      "focus on the reducer",
    ])
    XCTAssertEqual(Set(steeringRecords.map(\.id)).count, 2)
  }

  func testAcceptedQueuedSteeringAnchorsAfterCurrentLiveWork() async throws {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.cacheMessages(
      [SessionMessage(role: .user, content: "Inspect the transcript UI.", timestamp: 100)],
      for: "session-a"
    )
    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I'm fetching the GitHub issue first."),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: "session-a")
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Fetch issue", kind: "tool_round"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-1", id: "call-1", name: "web_fetch"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "web_fetch",
        arguments: #"{"url":"https://github.com/fawxai/fawx/issues/1849"}"#
      ),
      sessionID: "session-a"
    )

    sut.draftMessage = "push that file to dev when finished"
    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageCleared(on: sut)

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I'm checking the referenced markdown file next."),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: "session-a")
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-2", title: "Read markdown", kind: "tool_round"),
      sessionID: "session-a"
    )

    let visibleRows = sut.transcriptItems.map(visibleTranscriptText(for:))
    let firstWorkIndex = try XCTUnwrap(
      visibleRows.firstIndex { $0.contains("I'm fetching the GitHub issue first.") }
    )
    let steeringIndex = try XCTUnwrap(
      visibleRows.firstIndex { $0.contains("push that file to dev when finished") }
    )
    let secondWorkIndex = try XCTUnwrap(
      visibleRows.firstIndex { $0.contains("I'm checking the referenced markdown file next.") }
    )

    XCTAssertLessThan(firstWorkIndex, steeringIndex)
    XCTAssertLessThan(steeringIndex, secondWorkIndex)
  }

  func testAcceptedSteeringInsideLiveWorkFoldsIntoCompletedSummaryWithoutSplittingTurn()
    async throws
  {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 140)
    let userMessage = SessionMessage(role: .user, content: "Inspect the transcript UI.", timestamp: 100)
    let finalAnswer = SessionMessage(
      role: .assistant,
      content: "The transcript needs explicit turn boundaries.",
      timestamp: 140
    )

    sut.cacheMessages([userMessage], for: "session-a")
    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      startedAt: startedAt
    )

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I'm checking the first transcript files."),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: "session-a")
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Read model", kind: "tool_round"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-1", id: "call-1", name: "read_file"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: "read_file",
        arguments: #"{"path":"app/Fawx/Models/ChatTranscript.swift"}"#
      ),
      sessionID: "session-a"
    )

    sut.draftMessage = "push that file to dev when finished"
    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageCleared(on: sut)

    await sut.reduceStreamEventForTesting(
      .workingNarrationDelta("I'm checking the renderer now."),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(.textReset, sessionID: "session-a")
    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-2", title: "Read view", kind: "tool_round"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallStart(activityID: "round-2", id: "call-2", name: "read_file"),
      sessionID: "session-a"
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-2",
        id: "call-2",
        name: "read_file",
        arguments: #"{"path":"app/Fawx/Views/Shared/ChatDetailView.swift"}"#
      ),
      sessionID: "session-a"
    )

    sut.recordCompletedStreamingFootnoteForTesting(
      finalAnswer,
      sessionID: "session-a",
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(
      sessionID: "session-a",
      messages: [userMessage, finalAnswer]
    )

    XCTAssertFalse(items.contains { item in
      if case .toolActivityGroup = item {
        return true
      }
      if case .turnSteering = item {
        return true
      }
      return false
    })
    guard case .completedWorkSummary(let summary)? = items.first(where: { item in
      if case .completedWorkSummary = item {
        return true
      }
      return false
    }) else {
      return XCTFail("Expected steering and live work to fold into one completed summary")
    }
    XCTAssertEqual(
      summary.entries.map { entry -> String in
        switch entry {
        case .narration(let narration):
          return "narration:\(narration.text)"
        case .toolActivityGroup(let group):
          let ids = group.toolCalls.map(\.id).joined(separator: ",")
          return "tool:\(ids)"
        case .turnSteering(let steering):
          return "steering:\(steering.text)"
        }
      },
      [
        "narration:I'm checking the first transcript files.",
        "tool:call-1",
        "steering:push that file to dev when finished",
        "narration:I'm checking the renderer now.",
        "tool:call-2",
      ]
    )
  }

  func testQueuedMessageSteerToggleRestoresQueuedDeliveryWhenNoActiveRunAcceptsSteer() async {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":false,"reason":"no_active_run"}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageSteeringState(false, on: sut)

    XCTAssertFalse(sut.queuedMessageIsSteering)
    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertEqual(delivery?.text, "focus on the reducer")
  }

  func testQueuedMessageSteerToggleRestoresQueuedDeliveryWhenSteerRequestFails() async {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"error":"boom"}"#, statusCode: 500)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageSteeringState(false, on: sut)

    XCTAssertFalse(sut.queuedMessageIsSteering)
    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertEqual(delivery?.text, "focus on the reducer")
  }

  func testQueuedSteeringFailureDoesNotRestoreAfterUserTogglesBackToMessage() async {
    let context = makeNetworkedContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        Thread.sleep(forTimeInterval: 0.05)
        return .json(#"{"error":"boom"}"#, statusCode: 500)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    let sut = context.chatViewModel
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    sut.toggleQueuedMessageSteering()
    await waitForQueuedMessageSteeringState(false, on: sut)
    try? await Task.sleep(for: .milliseconds(100))

    XCTAssertEqual(sut.queuedMessage, "focus on the reducer")
    XCTAssertFalse(sut.queuedMessageIsSteering)
    XCTAssertNil(context.appState.toast)
  }

  func testQueuedSteeringUnknownRejectReasonIsVisibleForDebugging() async {
    let context = makeNetworkedContext { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":false,"reason":"turn_locked"}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    let sut = context.chatViewModel
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "focus on the reducer"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageSteeringState(false, on: sut)

    XCTAssertEqual(
      context.appState.toast?.message,
      "No active turn accepted steering (turn_locked). It will remain queued."
    )
    XCTAssertEqual(context.appState.toast?.style, .warning)
  }

  func testAcceptedQueuedSteeringCannotBeDeliveredAgainAsQueuedMessage() async {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/steer"):
        return .json(#"{"key":"session-a","steered":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "follow up"

    sut.sendDraft()
    sut.toggleQueuedMessageSteering()
    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/steer")
    await waitForQueuedMessageCleared(on: sut)
    sut.toggleQueuedMessageSteering()

    XCTAssertNil(sut.queuedMessage)
    let delivery = sut.consumeQueuedMessageForTesting(finishedSessionID: "session-a")

    XCTAssertNil(delivery)
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
    sut.draftSteering = "keep it terse"

    sut.sendDraft()

    XCTAssertEqual(sut.draftMessage, "")
    XCTAssertEqual(sut.draftSteering, "")
    XCTAssertNil(sut.queuedMessage)

    await waitForTranscriptItems(on: sut, minimumCount: 1)

    XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage).map(\.content), ["follow up"])
  }

  func testSendDraftDoesNotStopNewServerRunWhenAnotherSessionIsStreaming() async throws {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-b/messages"):
        return .eventStream(
          """
          event: done
          data: {"response":"server reply"}

          """)
      case ("GET", "/v1/sessions/session-b/messages"):
        return .json(
          """
          {
            "messages": [
              {"role": "user", "content": "follow up", "timestamp": 1},
              {"role": "assistant", "content": "server reply", "timestamp": 2}
            ],
            "total": 2
          }
          """)
      case ("GET", "/v1/sessions/session-b/context"):
        return .json(
          """
          {
            "used_tokens": 0,
            "max_tokens": 100,
            "percentage": 0,
            "compaction_threshold": 80
          }
          """)
      case ("POST", "/v1/sessions/session-b/stop"):
        return .json(#"{"key":"session-b","stopped":true}"#)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-b")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-b",
      streamingSessionID: "session-a"
    )
    sut.draftMessage = "follow up"
    sut.draftSteering = "keep it terse"

    sut.sendDraft()

    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-b/messages")
    await waitForRecordedRequest(method: "GET", path: "/v1/sessions/session-b/context")
    await waitForTranscriptContents(on: sut, ["follow up", "server reply"])
    let requests = MockChatURLProtocol.recordedRequests()

    XCTAssertTrue(
      requests.contains { request in
        request.httpMethod == "POST" && request.url?.path == "/v1/sessions/session-b/messages"
      }
    )
    let messageRequest = try XCTUnwrap(
      requests.first { request in
        request.httpMethod == "POST" && request.url?.path == "/v1/sessions/session-b/messages"
      }
    )
    let body = try XCTUnwrap(messageRequest.bodyDataForTesting())
    let payload = try JSONSerialization.jsonObject(with: body) as? [String: Any]
    XCTAssertEqual(payload?["steering"] as? String, "keep it terse")
    XCTAssertFalse(
      requests.contains { request in
        request.httpMethod == "POST" && request.url?.path == "/v1/sessions/session-b/stop"
      },
      "Starting a freshly opened stream must not send /stop for the same session."
    )
  }

  func testExplicitStopStreamingSendsServerStopForCurrentSession() async {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/stop"):
        return .json(#"{"key":"session-a","stopped":true}"#)
      case ("GET", "/v1/sessions/session-a/context"):
        return .json(
          """
          {
            "used_tokens": 0,
            "max_tokens": 100,
            "percentage": 0,
            "compaction_threshold": 80
          }
          """)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )

    sut.stopStreamingForTesting()

    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/stop")
    await waitForNoActiveStreams(on: sut)
  }

  func testExplicitStopStreamingFinalizesLocalStreamImmediately() async {
    let sut = makeNetworkedSUT { request in
      switch (request.httpMethod, request.url?.path) {
      case ("POST", "/v1/sessions/session-a/stop"):
        return .json(#"{"key":"session-a","stopped":true}"#)
      case ("GET", "/v1/sessions/session-a/context"):
        return .json(
          """
          {
            "used_tokens": 0,
            "max_tokens": 100,
            "percentage": 0,
            "compaction_threshold": 80
          }
          """)
      default:
        return .json("{}", statusCode: 404)
      }
    }
    defer {
      MockChatURLProtocol.reset()
    }

    sut.prepareToDisplaySession("session-a")
    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a",
      streamingText: "partial answer"
    )

    sut.stopStreamingForTesting()

    await waitForRecordedRequest(method: "POST", path: "/v1/sessions/session-a/stop")
    await waitForNoActiveStreams(on: sut)
    await waitForTranscriptContents(on: sut, ["partial answer\n\n(interrupted)"])
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

  func testResetStreamingStateRetiresCompletedToolGroupsFromRuntimeActivity() throws {
    let sut = makeSUT()

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: "session-a",
      streamingSessionID: "session-a"
    )
    sut.setLiveToolGroupForTesting(
      sessionID: "session-a",
      toolCalls: [
        ToolCallRecord(
          id: "tool-1",
          name: "read_file",
          arguments: "{\"path\":\"README.md\"}",
          result: "docs",
          isRunning: false,
          isError: false
        )
      ]
    )

    let activeRuntime = try XCTUnwrap(sut.runtimeActivityForTesting(sessionID: "session-a"))
    XCTAssertTrue(activeRuntime.isStreaming)
    XCTAssertEqual(activeRuntime.liveToolCallCount, 1)

    sut.resetStreamingStateForTesting(sessionID: "session-a")

    XCTAssertNil(sut.runtimeActivityForTesting(sessionID: "session-a"))
  }

  private func makeSUT(
    connectionStatus: ConnectionStatus = .disconnected,
    compactionBannerSleepHandler: @escaping @Sendable (Duration) async throws -> Void = {
      duration in
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

  private func makeNetworkedSUT(
    responder: @escaping MockChatURLProtocolStore.Responder
  ) -> ChatViewModel {
    makeNetworkedContext(responder: responder).chatViewModel
  }

  private func makeNetworkedContext(
    responder: @escaping MockChatURLProtocolStore.Responder
  ) -> NetworkedChatViewModelContext {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [MockChatURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = FawxClient(
      baseURL: URL(string: "http://localhost:8400"),
      bearerToken: "test-token",
      restSession: session,
      streamSession: session
    )
    let defaultsSuiteName = "ChatViewModelTests.\(UUID().uuidString)"
    UserDefaults(suiteName: defaultsSuiteName)?.removePersistentDomain(forName: defaultsSuiteName)
    let persistence = AppStatePersistence(
      defaultsSuiteName: defaultsSuiteName,
      keychainService: "ChatViewModelTests.\(UUID().uuidString)",
      localInstallLoader: { nil }
    )
    let appState = AppState(
      persistence: persistence,
      client: client,
      startLoadingPersistedState: false
    )
    appState.connectionStatus = .connected
    MockChatURLProtocol.setResponder(responder)
    let sessionViewModel = SessionViewModel(appState: appState)
    return NetworkedChatViewModelContext(
      chatViewModel: ChatViewModel(appState: appState, sessionViewModel: sessionViewModel),
      appState: appState
    )
  }

  private func makeThreadModelSUT()
    -> (appState: AppState, sessionViewModel: SessionViewModel, chatViewModel: ChatViewModel)
  {
    let appState = AppState(startLoadingPersistedState: false)
    let activeModel = ModelInfo(
      modelID: "active-model",
      provider: "Anthropic",
      authMethod: "api_key",
      displayName: "Active Model"
    )
    appState.activeModel = activeModel
    appState.availableModels = [
      activeModel,
      ModelInfo(
        modelID: "draft-model",
        provider: "Fireworks AI",
        authMethod: "api_key",
        displayName: "Draft Model"
      ),
    ]
    let sessionViewModel = SessionViewModel(appState: appState)
    let chatViewModel = ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
    return (appState, sessionViewModel, chatViewModel)
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
    for _ in 0..<50 {
      if sut.transcriptItems.count >= minimumCount {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTFail("Expected at least \(minimumCount) transcript item(s).")
  }

  private func waitForQueuedMessageSteeringState(
    _ expected: Bool,
    on sut: ChatViewModel
  ) async {
    for _ in 0..<50 {
      if sut.queuedMessageIsSteering == expected {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTFail("Expected queued message steering state to become \(expected).")
  }

  private func waitForQueuedMessageCleared(on sut: ChatViewModel) async {
    for _ in 0..<50 {
      if sut.queuedMessage == nil {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTFail("Expected queued message to clear.")
  }

  private func waitForRecordedRequest(method: String, path: String) async {
    for _ in 0..<50 {
      if MockChatURLProtocol.recordedRequests().contains(where: { request in
        request.httpMethod == method && request.url?.path == path
      }) {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTFail("Expected \(method) \(path) to be requested.")
  }

  private func waitForRecordedRequestCount(
    _ expectedCount: Int,
    method: String,
    path: String
  ) async {
    for _ in 0..<50 {
      let count = MockChatURLProtocol.recordedRequests().filter { request in
        request.httpMethod == method && request.url?.path == path
      }.count
      if count >= expectedCount {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTFail("Expected at least \(expectedCount) \(method) \(path) request(s).")
  }

  private func waitForNoActiveStreams(on sut: ChatViewModel) async {
    for _ in 0..<50 {
      if sut.activeStreamSessionIDs.isEmpty {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTAssertTrue(sut.activeStreamSessionIDs.isEmpty)
  }

  private func waitForTranscriptContents(on sut: ChatViewModel, _ expected: [String]) async {
    for _ in 0..<50 {
      let contents = sut.transcriptItems.compactMap(\.sessionMessage).map(\.content)
      if contents == expected {
        return
      }
      try? await Task.sleep(for: .milliseconds(10))
    }

    XCTAssertEqual(sut.transcriptItems.compactMap(\.sessionMessage).map(\.content), expected)
  }

  private func visibleTranscriptText(on sut: ChatViewModel) -> String {
    sut.transcriptItems
      .map(visibleTranscriptText(for:))
      .joined(separator: "\n")
  }

  private func visibleTranscriptText(for item: ChatTranscriptItem) -> String {
    switch item {
    case .message(let message):
      return [message.displayText, message.footnoteText]
        .compactMap { $0 }
        .joined(separator: "\n")
    case .finalAnswer(let message):
      return [message.displayText, message.footnoteText]
        .compactMap { $0 }
        .joined(separator: "\n")
    case .toolActivityGroup(let group):
      return visibleTranscriptText(for: group)
    case .completedWorkSummary(let summary):
      return ([summary.elapsedText] + summary.entries.map(visibleTranscriptText(for:)))
        .joined(separator: "\n")
    case .turnSteering(let steering):
      return steering.text
    }
  }

  private func visibleTranscriptText(for entry: CompletedWorkEntry) -> String {
    switch entry {
    case .narration(let narration):
      return narration.text
    case .toolActivityGroup(let group):
      return visibleTranscriptText(for: group)
    case .turnSteering(let steering):
      return steering.text
    }
  }

  private func visibleTranscriptText(for group: ToolActivityGroupRecord) -> String {
    let toolText = group.toolCalls.map { toolCall in
      [
        toolCall.displayName,
        toolCall.arguments,
        toolCall.result,
        toolCall.progress?.targetDisplay,
      ]
      .compactMap { $0 }
      .joined(separator: "\n")
    }
    return toolText.joined(separator: "\n")
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
        state = state &* 6_364_136_223_846_793_005 &+ 1
        pixels[offset] = UInt8(truncatingIfNeeded: state >> 24)
        state = state &* 6_364_136_223_846_793_005 &+ 1
        pixels[offset + 1] = UInt8(truncatingIfNeeded: state >> 16)
        state = state &* 6_364_136_223_846_793_005 &+ 1
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
    XCTAssertEqual(
      contents,
      [
        "anchor-1",
        "fetched-gap-1",
        "local-gap-1",
        "anchor-2",
        "fetched-gap-2",
        "local-gap-2",
        "anchor-3",
      ])
  }

  func testMergeFetchedMessagesDoesNotTreatSameContentWithDifferentRolesAsEquivalent() {
    let sut = makeSUT()
    let localMessages = [
      SessionMessage(role: .assistant, content: "same", timestamp: 1)
    ]
    let fetchedMessages = [
      SessionMessage(role: .user, content: "same", timestamp: 2)
    ]

    sut.cacheMessages(localMessages, for: "session-a")
    sut.prepareToDisplaySession("session-a")

    sut.applyFetchedMessagesForTesting(fetchedMessages, sessionID: "session-a")

    XCTAssertEqual(
      sut.cachedMessages(for: "session-a")?.map(\.role),
      [.user]
    )
    XCTAssertEqual(
      sut.cachedMessages(for: "session-a")?.map(\.timestamp),
      [2]
    )
  }

  private func completedSummaryForToolNarration(
    _ narrations: [String],
    toolName: String = "search_text",
    arguments: String
  ) async throws -> CompletedWorkSummaryRecord {
    try await completedSummaryForToolNarration(
      narrations.map { ($0, false) },
      toolName: toolName,
      arguments: arguments
    )
  }

  private func completedSummaryForToolNarration(
    _ narrations: [(text: String, voiceoverSuppressed: Bool)],
    toolName: String = "search_text",
    arguments: String
  ) async throws -> CompletedWorkSummaryRecord {
    let sut = makeSUT()
    let sessionID = "session-a"
    let startedAt = Date(timeIntervalSince1970: 100)
    let endedAt = Date(timeIntervalSince1970: 124)
    let finalMessage = SessionMessage(role: .assistant, content: "Done.", timestamp: 124)

    sut.setStreamingStateForTesting(
      isStreaming: true,
      currentSessionID: sessionID,
      streamingSessionID: sessionID,
      phase: .reason,
      startedAt: startedAt
    )

    for narration in narrations {
      await sut.reduceStreamEventForTesting(
        .workingNarrationDelta(
          narration.text,
          voiceoverSuppressed: narration.voiceoverSuppressed
        ),
        sessionID: sessionID
      )
      await sut.reduceStreamEventForTesting(.textReset, sessionID: sessionID)
    }

    await sut.reduceStreamEventForTesting(
      .activityStart(id: "round-1", title: "Run tool", kind: "tool_round"),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolCallComplete(
        activityID: "round-1",
        id: "call-1",
        name: toolName,
        arguments: arguments
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(
      .activityToolResult(
        activityID: "round-1",
        id: "call-1",
        toolName: toolName,
        output: "matched",
        isError: false
      ),
      sessionID: sessionID
    )
    await sut.reduceStreamEventForTesting(.activityEnd(id: "round-1"), sessionID: sessionID)

    sut.recordCompletedStreamingFootnoteForTesting(
      finalMessage,
      sessionID: sessionID,
      startedAt: startedAt,
      endedAt: endedAt
    )

    let items = sut.makeTranscriptItemsForTesting(sessionID: sessionID, messages: [finalMessage])
    guard case .completedWorkSummary(let summary)? = items.first else {
      throw XCTSkip("Expected completed work summary item")
    }
    return summary
  }

  private func completedSummaryNarrationText(
    _ summary: CompletedWorkSummaryRecord
  ) -> [String] {
    summary.entries.compactMap { entry -> String? in
      guard case .narration(let narration) = entry else {
        return nil
      }
      return narration.text
    }
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

private struct NetworkedChatViewModelContext {
  let chatViewModel: ChatViewModel
  let appState: AppState
}

// Keep this file-local until a second test file needs the same HTTP recording surface.
private final class MockChatURLProtocol: URLProtocol, @unchecked Sendable {
  private static let store = MockChatURLProtocolStore()

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

  static func setResponder(_ responder: @escaping MockChatURLProtocolStore.Responder) {
    store.setResponder(responder)
  }

  static func recordedRequests() -> [URLRequest] {
    store.recordedRequests()
  }

  static func reset() {
    store.reset()
  }
}

private final class MockChatURLProtocolStore: @unchecked Sendable {
  typealias Responder = @Sendable (URLRequest) throws -> MockChatResponse

  // URLProtocol entry points are synchronous, so this store stays lock-backed instead of actor-backed.
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
      throw MockChatProtocolError.missingResponder
    }
    configuredResponder = responder
    lock.unlock()

    let response = try configuredResponder(request)
    guard let url = request.url,
      let httpResponse = HTTPURLResponse(
        url: url,
        statusCode: response.statusCode,
        httpVersion: nil,
        headerFields: ["Content-Type": response.contentType]
      )
    else {
      throw MockChatProtocolError.invalidResponse
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

private struct MockChatResponse: Sendable {
  let statusCode: Int
  let body: Data
  let contentType: String

  static func json(_ body: String, statusCode: Int = 200) -> MockChatResponse {
    MockChatResponse(
      statusCode: statusCode,
      body: Data(body.utf8),
      contentType: "application/json"
    )
  }

  static func eventStream(_ body: String, statusCode: Int = 200) -> MockChatResponse {
    MockChatResponse(
      statusCode: statusCode,
      body: Data(body.utf8),
      contentType: "text/event-stream"
    )
  }
}

private enum MockChatProtocolError: Error {
  case missingResponder
  case invalidResponse
  case unreadableRequestBody
}

private extension URLRequest {
  func bodyDataForTesting() throws -> Data? {
    if let httpBody {
      return httpBody
    }

    guard let httpBodyStream else {
      return nil
    }

    return try Data(readingRequestBodyFrom: httpBodyStream)
  }
}

private extension Data {
  init(readingRequestBodyFrom stream: InputStream) throws {
    stream.open()
    defer { stream.close() }

    self.init()

    let bufferSize = 4096
    let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
    defer { buffer.deallocate() }

    while stream.hasBytesAvailable {
      let bytesRead = stream.read(buffer, maxLength: bufferSize)
      if bytesRead < 0 {
        throw stream.streamError ?? MockChatProtocolError.unreadableRequestBody
      }
      if bytesRead == 0 {
        break
      }

      append(buffer, count: bytesRead)
    }
  }
}
