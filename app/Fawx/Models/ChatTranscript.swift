import Foundation

struct ToolCallRecord: Identifiable, Hashable, Sendable {
    let id: String
    var name: String
    var arguments: String
    var result: String?
    var isRunning: Bool
    var isError: Bool

    var displayName: String {
        name.isEmpty ? "tool" : name
    }
}

struct ToolActivityGroupRecord: Identifiable, Hashable, Sendable {
    let id: String
    var toolCalls: [ToolCallRecord]
    var isLive: Bool

    var runningCount: Int {
        toolCalls.filter(\.isRunning).count
    }

    var errorCount: Int {
        toolCalls.filter(\.isError).count
    }

    var completedCount: Int {
        toolCalls.filter { !$0.isRunning }.count
    }

    var toolCount: Int {
        toolCalls.count
    }
}

struct TranscriptMessage: Identifiable, Hashable, Sendable {
    let id: String
    let message: SessionMessage
    let displayText: String
}

enum ChatTranscriptItem: Identifiable, Hashable, Sendable {
    case message(TranscriptMessage)
    case toolActivityGroup(ToolActivityGroupRecord)

    var id: String {
        switch self {
        case .message(let message):
            return "message:\(message.id)"
        case .toolActivityGroup(let group):
            return "tool-group:\(group.id)"
        }
    }

    var sessionMessage: SessionMessage? {
        guard case .message(let transcriptMessage) = self else {
            return nil
        }

        return transcriptMessage.message
    }
}
