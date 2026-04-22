import Foundation
import OSLog

enum ToolActivityKind: String, Hashable, Sendable {
    case file
    case search
    case command
    case edit
    case other
}

struct ToolActivityDescriptor: Hashable, Sendable {
    let arguments: String
    let normalizedName: String
    let kind: ToolActivityKind

    private static let logger = Logger(
        subsystem: "ai.fawx.app",
        category: "ToolActivityDescriptor"
    )
    let primaryTarget: String?

    init(name: String, arguments: String) {
        self.arguments = arguments
        normalizedName = Self.normalizedName(name)
        kind = Self.kind(forNormalizedName: normalizedName)
        primaryTarget = Self.argumentValue(
            ["path", "file", "filename", "url", "query", "pattern", "command", "cmd", "argv"],
            in: arguments
        )
    }

    var isCodeMutation: Bool {
        kind == .edit
    }

    var isCommand: Bool {
        kind == .command
    }

    func argumentValue(_ keys: [String]) -> String? {
        Self.argumentValue(keys, in: arguments)
    }

    private static func kind(forNormalizedName name: String) -> ToolActivityKind {
        switch name {
        case "run_command", "exec_command", "shell":
            return .command
        case "search_text", "search_files", "rg", "grep", "web_search", "memory_search":
            return .search
        case "write_file", "edit_file", "apply_patch":
            return .edit
        case "read_file", "read", "list_dir", "ls", "web_fetch", "fetch_url":
            return .file
        default:
            if name.contains("search") || name == "find" {
                return .search
            }
            if name.contains("command") || name.contains("shell") {
                return .command
            }
            if name.contains("edit") || name.contains("write") || name.contains("patch") {
                return .edit
            }
            if name.contains("file") || name.contains("read") || name.contains("list") {
                return .file
            }
            return .other
        }
    }

    private static func argumentValue(_ keys: [String], in arguments: String) -> String? {
        let trimmed = arguments.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return nil
        }

        if let data = trimmed.data(using: .utf8),
           let value = try? JSONDecoder().decode(JSONValue.self, from: data),
           case .object(let object) = value {
            for key in keys {
                if let value = object[key]?.stringValue?.nonEmpty {
                    return value
                }
                // Display/matching only: this is not intended to reconstruct a shell-safe command.
                if key == "argv",
                   let value = object[key]?.stringArrayValue,
                   !value.isEmpty {
                    return value.joined(separator: " ")
                }
            }
        }

        if looksLikeIncompleteJSONObject(trimmed) {
            for key in keys {
                if let value = partialStringValue(for: key, in: trimmed)?.nonEmpty {
                    logger.debug("partial tool argument JSON fallback used")
                    return value
                }
            }
        }
        return nil
    }

    private static func looksLikeIncompleteJSONObject(_ raw: String) -> Bool {
        raw.hasPrefix("{") && !(raw.hasSuffix("}") || raw.hasSuffix("]"))
    }

    private static func partialStringValue(for key: String, in raw: String) -> String? {
        // Live tool arguments can arrive as partial JSON before the backend's
        // complete event provides well-formed JSON. Keep this fallback scoped to
        // incomplete objects so malformed completed payloads do not get
        // silently reinterpreted by the view layer.
        guard let keyRange = raw.range(of: "\"\(key)\""),
              let colon = raw[keyRange.upperBound...].firstIndex(of: ":") else {
            return nil
        }

        let afterColon = raw[raw.index(after: colon)...]
            .drop(while: { $0.isWhitespace })
        guard afterColon.first == "\"" else {
            return nil
        }

        let valueStart = afterColon.index(after: afterColon.startIndex)
        let valueTail = afterColon[valueStart...]
        let valueEnd = valueTail.firstIndex(of: "\"") ?? valueTail.endIndex
        return String(valueTail[..<valueEnd])
    }

    private static func normalizedName(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }
}

struct ToolCallRecord: Identifiable, Hashable, Sendable {
    let id: String
    var name: String
    var arguments: String
    var result: String?
    var isRunning: Bool
    var isError: Bool
    var progress: ToolProgressRecord? = nil

    var displayName: String {
        name.isEmpty ? "tool" : name
    }

    var activityDescriptor: ToolActivityDescriptor {
        ToolActivityDescriptor(name: name, arguments: arguments)
    }
}

struct ToolProgressRecord: Hashable, Sendable {
    let category: String
    let target: String?
    let advancesSlot: String?
    let outcome: String

    var normalizedCategory: String {
        category.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    var normalizedOutcome: String {
        outcome.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    var targetDisplay: String? {
        target?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
    }

    var isMutation: Bool {
        normalizedCategory == "mutation"
    }

    var didAdvance: Bool {
        normalizedOutcome == "advanced"
    }

    var isDuplicate: Bool {
        normalizedOutcome == "duplicate"
    }

    var isRetryableFailure: Bool {
        normalizedOutcome == "retryable_failure"
    }
}

struct ToolActivityGroupRecord: Identifiable, Hashable, Sendable {
    let id: String
    var toolCalls: [ToolCallRecord]
    var isLive: Bool

    // Tool groups intentionally carry only tool activity. Narration is modeled
    // as first-class transcript messages/summary entries so live and historical
    // reduction use the same ordering contract. This type is not Codable, so
    // the previous in-memory narration field was not a persisted data shape.
    init(
        id: String,
        toolCalls: [ToolCallRecord],
        isLive: Bool
    ) {
        self.id = id
        self.toolCalls = toolCalls
        self.isLive = isLive
    }

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

    var hasVisibleActivity: Bool {
        !toolCalls.isEmpty
    }
}

struct CompletedWorkSummaryRecord: Identifiable, Hashable, Sendable {
    let id: String
    let elapsedText: String
    let summaryText: String?
    var entries: [CompletedWorkEntry]

    var hasActivity: Bool {
        entries.contains(where: \.hasVisibleActivity)
    }

    var activityGroups: [ToolActivityGroupRecord] {
        entries.compactMap { entry in
            guard case .toolActivityGroup(let group) = entry else {
                return nil
            }
            return group
        }
    }

    init(
        id: String,
        elapsedText: String,
        summaryText: String? = nil,
        activityGroups: [ToolActivityGroupRecord]
    ) {
        self.init(
            id: id,
            elapsedText: elapsedText,
            summaryText: summaryText,
            entries: activityGroups.flatMap(Self.entries(from:))
        )
    }

    init(
        id: String,
        elapsedText: String,
        summaryText: String? = nil,
        entries: [CompletedWorkEntry]
    ) {
        self.id = id
        self.elapsedText = elapsedText
        self.summaryText = summaryText?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
        self.entries = entries
    }

    static func entries(from group: ToolActivityGroupRecord) -> [CompletedWorkEntry] {
        guard !group.toolCalls.isEmpty else {
            return []
        }

        return group.toolCalls.map { toolCall in
            .toolActivityGroup(
                ToolActivityGroupRecord(
                    id: "\(group.id):tool:\(toolCall.id)",
                    toolCalls: [toolCall],
                    isLive: group.isLive
                )
            )
        }
    }
}

struct CompletedWorkNarrationRecord: Identifiable, Hashable, Sendable {
    let id: String
    var text: String

    var hasVisibleActivity: Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty != nil
    }
}

enum CompletedWorkEntry: Identifiable, Hashable, Sendable {
    case narration(CompletedWorkNarrationRecord)
    case toolActivityGroup(ToolActivityGroupRecord)
    case turnSteering(TurnSteeringRecord)

    var id: String {
        switch self {
        case .narration(let narration):
            return "narration:\(narration.id)"
        case .toolActivityGroup(let group):
            return "tool-group:\(group.id)"
        case .turnSteering(let steering):
            return "turn-steering:\(steering.id)"
        }
    }

    var hasVisibleActivity: Bool {
        switch self {
        case .narration(let narration):
            return narration.hasVisibleActivity
        case .toolActivityGroup(let group):
            return group.hasVisibleActivity
        case .turnSteering(let steering):
            return !steering.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
    }
}

struct TranscriptMessage: Identifiable, Hashable, Sendable {
    let id: String
    let message: SessionMessage
    let displayText: String
    let footnoteText: String?
    var isWorkingNarration: Bool = false
    var isStreaming: Bool = false
}

struct TurnSteeringRecord: Identifiable, Hashable, Sendable {
    let id: String
    let text: String
    let timestamp: Int
}

enum ChatTranscriptItem: Identifiable, Hashable, Sendable {
    case message(TranscriptMessage)
    case toolActivityGroup(ToolActivityGroupRecord)
    case completedWorkSummary(CompletedWorkSummaryRecord)
    case finalAnswer(TranscriptMessage)
    case turnSteering(TurnSteeringRecord)

    var id: String {
        switch self {
        case .message(let message):
            return "message:\(message.id)"
        case .toolActivityGroup(let group):
            return "tool-group:\(group.id)"
        case .completedWorkSummary(let summary):
            return "work-summary:\(summary.id)"
        case .finalAnswer(let message):
            return "final-answer:\(message.id)"
        case .turnSteering(let steering):
            return "turn-steering:\(steering.id)"
        }
    }

    var sessionMessage: SessionMessage? {
        transcriptMessage?.message
    }

    var transcriptMessage: TranscriptMessage? {
        switch self {
        case .message(let transcriptMessage), .finalAnswer(let transcriptMessage):
            return transcriptMessage
        case .toolActivityGroup, .completedWorkSummary, .turnSteering:
            return nil
        }
    }

    var phase: TranscriptPhase {
        switch self {
        case .message(let message):
            return message.isWorkingNarration ? .workingNarration : .message
        case .toolActivityGroup:
            return .toolGroup
        case .completedWorkSummary:
            return .completedSummary
        case .finalAnswer:
            return .finalAnswer
        case .turnSteering:
            return .turnSteering
        }
    }
}

enum TranscriptPhase: Int, Hashable, Sendable {
    case message
    case turnSteering
    case workingNarration
    case toolGroup
    case completedSummary
    case finalAnswer
}

struct TranscriptPhaseOrderViolation: Equatable, Sendable {
    let terminalItemID: String
    let laterWorkingItemID: String
    let laterWorkingPhase: TranscriptPhase
}

extension Collection where Element == ChatTranscriptItem {
    func firstTranscriptPhaseOrderViolation() -> TranscriptPhaseOrderViolation? {
        AssistantTranscriptTurnReducer.firstPhaseOrderViolation(in: self)
    }
}
