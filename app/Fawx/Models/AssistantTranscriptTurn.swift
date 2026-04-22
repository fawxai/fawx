import Foundation

enum AssistantTurnLifecycle: Hashable, Sendable {
    case collectingWork
    case summarizing
    case finalizing
    case completed
}

struct ChunkPresentation: Hashable, Sendable {
    let defaultExpanded: Bool
    let shouldCollapseOnComplete: Bool

    static let visibleStatic = ChunkPresentation(
        defaultExpanded: true,
        shouldCollapseOnComplete: false
    )
}

struct WorkingNarrationRecord: Identifiable, Hashable, Sendable {
    let id: String
    let text: String
    let isLive: Bool
}

enum AssistantTranscriptTurnChunk: Identifiable, Hashable, Sendable {
    case narration(WorkingNarrationRecord)
    case toolActivity(ToolActivityGroupRecord)
    case turnSteering(TurnSteeringRecord)

    var id: String {
        switch self {
        case .narration(let narration):
            return "narration:\(narration.id)"
        case .toolActivity(let group):
            return "tool-group:\(group.id)"
        case .turnSteering(let steering):
            return "turn-steering:\(steering.id)"
        }
    }

    func presentation(in lifecycle: AssistantTurnLifecycle) -> ChunkPresentation {
        switch self {
        case .narration:
            return .visibleStatic
        case .toolActivity(let group):
            return ChunkPresentation(
                defaultExpanded: group.isLive,
                shouldCollapseOnComplete: true
            )
        case .turnSteering:
            return .visibleStatic
        }
    }
}

struct AssistantTranscriptTurn: Identifiable, Hashable, Sendable {
    let id: String
    var chunks: [AssistantTranscriptTurnChunk]
    var completedSummary: CompletedWorkSummaryRecord?
    var finalAnswer: TranscriptMessage?

    var workingNarration: [WorkingNarrationRecord] {
        chunks.compactMap { chunk in
            guard case .narration(let narration) = chunk else {
                return nil
            }
            return narration
        }
    }

    var toolGroups: [ToolActivityGroupRecord] {
        chunks.compactMap { chunk in
            guard case .toolActivity(let group) = chunk else {
                return nil
            }
            return group
        }
    }

    var isEmpty: Bool {
        chunks.isEmpty
            && completedSummary == nil
            && finalAnswer == nil
    }

    var lifecycle: AssistantTurnLifecycle {
        if let finalAnswer {
            return finalAnswer.isStreaming ? .finalizing : .completed
        }
        if completedSummary != nil {
            return .summarizing
        }
        return .collectingWork
    }
}

enum TranscriptTurn: Identifiable, Hashable, Sendable {
    case standalone(ChatTranscriptItem)
    case assistant(AssistantTranscriptTurn)

    var id: String {
        switch self {
        case .standalone(let item):
            return "standalone:\(item.id)"
        case .assistant(let turn):
            return "assistant-turn:\(turn.id)"
        }
    }
}

extension BidirectionalCollection where Element == TranscriptTurn {
    var hasCurrentTurnTerminalAssistantOutput: Bool {
        guard let turn = last else {
            return false
        }
        switch turn {
        case .assistant(let turn):
            return turn.hasVisibleTerminalAssistantOutput
        case .standalone(let item):
            return item.hasVisibleTerminalAssistantOutput
        }
    }
}

private extension AssistantTranscriptTurn {
    var hasVisibleTerminalAssistantOutput: Bool {
        finalAnswer?.hasVisibleDisplayText == true
    }
}

private extension ChatTranscriptItem {
    var hasVisibleTerminalAssistantOutput: Bool {
        switch self {
        case .finalAnswer(let message):
            return message.hasVisibleDisplayText
        case .message(let message):
            return message.message.role == .assistant
                && !message.isWorkingNarration
                && message.hasVisibleDisplayText
        case .toolActivityGroup, .completedWorkSummary, .turnSteering:
            return false
        }
    }
}

private extension TranscriptMessage {
    var hasVisibleDisplayText: Bool {
        !displayText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

struct AssistantTranscriptTurnReduction: Sendable {
    let turns: [TranscriptTurn]
    let phaseOrderViolation: TranscriptPhaseOrderViolation?
}

struct AssistantTranscriptTurnReducer: Sendable {
    private var turns: [TranscriptTurn] = []
    private var currentAssistantTurn: AssistantTranscriptTurn?
    private var phaseOrderTracker = TranscriptPhaseOrderTracker()

    static func reduce(_ items: some Sequence<ChatTranscriptItem>) -> AssistantTranscriptTurnReduction {
        var reducer = AssistantTranscriptTurnReducer()
        for item in items {
            reducer.reduce(item)
        }
        return reducer.finish()
    }

    static func firstPhaseOrderViolation(
        in items: some Sequence<ChatTranscriptItem>
    ) -> TranscriptPhaseOrderViolation? {
        var tracker = TranscriptPhaseOrderTracker()
        for item in items {
            tracker.record(item)
            if let violation = tracker.violation {
                return violation
            }
        }
        return nil
    }

    mutating func reduce(_ item: ChatTranscriptItem) {
        recordPhaseOrderViolationIfNeeded(for: item)

        switch item {
        case .message(let message)
            where message.message.role == .assistant && message.isWorkingNarration:
            ensureAssistantTurn(id: message.id)
            currentAssistantTurn?.chunks.append(
                .narration(
                    WorkingNarrationRecord(
                        id: "message:\(message.id)",
                        text: message.displayText,
                        isLive: message.isStreaming
                    )
                )
            )
        case .toolActivityGroup(let group):
            ensureAssistantTurn(id: group.id)
            currentAssistantTurn?.chunks.append(.toolActivity(group))
        case .completedWorkSummary(let summary):
            ensureAssistantTurn(id: summary.id)
            currentAssistantTurn?.completedSummary = summary
        case .finalAnswer(let message):
            ensureAssistantTurn(id: message.id)
            currentAssistantTurn?.finalAnswer = message
            flushAssistantTurn()
        case .turnSteering(let steering):
            if currentAssistantTurn != nil {
                currentAssistantTurn?.chunks.append(.turnSteering(steering))
            } else {
                turns.append(.standalone(item))
            }
        case .message:
            flushAssistantTurn()
            turns.append(.standalone(item))
        }
    }

    mutating func finish() -> AssistantTranscriptTurnReduction {
        flushAssistantTurn()
        return AssistantTranscriptTurnReduction(
            turns: turns,
            phaseOrderViolation: phaseOrderTracker.violation
        )
    }

    private mutating func ensureAssistantTurn(id: String) {
        if currentAssistantTurn == nil {
            currentAssistantTurn = AssistantTranscriptTurn(
                id: id,
                chunks: [],
                completedSummary: nil,
                finalAnswer: nil
            )
        }
    }

    private mutating func flushAssistantTurn() {
        guard let turn = currentAssistantTurn, !turn.isEmpty else {
            currentAssistantTurn = nil
            return
        }

        turns.append(.assistant(turn))
        currentAssistantTurn = nil
    }

    private mutating func recordPhaseOrderViolationIfNeeded(for item: ChatTranscriptItem) {
        phaseOrderTracker.record(item)
    }
}

private struct TranscriptPhaseOrderTracker: Sendable {
    private(set) var violation: TranscriptPhaseOrderViolation?
    private var terminalItemID: String?

    mutating func record(_ item: ChatTranscriptItem) {
        guard violation == nil else {
            return
        }
        switch item {
        case .message(let message):
            if item.phase == .workingNarration, let terminalItemID {
                violation = TranscriptPhaseOrderViolation(
                    terminalItemID: terminalItemID,
                    laterWorkingItemID: item.id,
                    laterWorkingPhase: item.phase
                )
            }

            if message.message.role != .assistant {
                terminalItemID = nil
            }
        case .finalAnswer, .completedWorkSummary:
            terminalItemID = item.id
        case .toolActivityGroup:
            if let terminalItemID {
                violation = TranscriptPhaseOrderViolation(
                    terminalItemID: terminalItemID,
                    laterWorkingItemID: item.id,
                    laterWorkingPhase: item.phase
                )
            }
        case .turnSteering:
            // Steering is metadata within the turn; it should not clear a
            // terminal marker or count as later work.
            break
        }
    }
}

extension Collection where Element == ChatTranscriptItem {
    func transcriptTurns() -> [TranscriptTurn] {
        AssistantTranscriptTurnReducer.reduce(self).turns
    }
}
