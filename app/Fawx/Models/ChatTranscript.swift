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

struct TranscriptMessage: Identifiable, Hashable, Sendable {
    let id: String
    let message: SessionMessage
}

enum ChatTranscriptItem: Identifiable, Hashable, Sendable {
    case message(TranscriptMessage)
    case toolCall(ToolCallRecord)

    var id: String {
        switch self {
        case .message(let message):
            return "message:\(message.id)"
        case .toolCall(let toolCall):
            return "tool:\(toolCall.id)"
        }
    }

    var sessionMessage: SessionMessage? {
        guard case .message(let transcriptMessage) = self else {
            return nil
        }

        return transcriptMessage.message
    }
}
