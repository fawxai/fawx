import Foundation

enum SessionKind: String, Codable, CaseIterable, Sendable, Hashable {
    case main
    case subagent
    case channel
    case cron
}

enum SessionStatus: String, Codable, CaseIterable, Sendable, Hashable {
    case active
    case idle
    case completed
    case failed
    case paused
}

struct Session: Codable, Identifiable, Sendable, Hashable {
    let key: String
    let kind: SessionKind
    var status: SessionStatus
    var label: String?
    var title: String?
    var preview: String?
    var model: String
    let createdAt: Int
    var updatedAt: Int
    var messageCount: Int

    var id: String { key }

    var displayTitle: String {
        if let label = label?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty {
            return truncateSessionTitle(label)
        }

        if let title = title?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty {
            return summarizedSessionTitle(from: title)
        }

        return "New Session"
    }

    var subtitlePreview: String? {
        preview?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    enum CodingKeys: String, CodingKey {
        case key
        case kind
        case status
        case label
        case title
        case preview
        case model
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case messageCount = "message_count"
    }
}

struct SessionsResponse: Codable, Sendable, Hashable {
    let sessions: [Session]
    let total: Int
}

struct DeleteSessionResponse: Codable, Sendable, Hashable {
    let deleted: Bool
    let key: String
}

struct ClearSessionResponse: Codable, Sendable, Hashable {
    let cleared: Bool
    let key: String
}

struct SessionMemory: Codable, Sendable, Hashable {
    static let maxItems = 20
    static let maxTokens = 2_000

    var project: String?
    var currentState: String?
    var keyDecisions: [String]
    var activeFiles: [String]
    var customContext: [String]
    var lastUpdated: Int

    init(
        project: String? = nil,
        currentState: String? = nil,
        keyDecisions: [String] = [],
        activeFiles: [String] = [],
        customContext: [String] = [],
        lastUpdated: Int = 0
    ) {
        self.project = project
        self.currentState = currentState
        self.keyDecisions = keyDecisions
        self.activeFiles = activeFiles
        self.customContext = customContext
        self.lastUpdated = lastUpdated
    }

    enum CodingKeys: String, CodingKey {
        case project
        case currentState = "current_state"
        case keyDecisions = "key_decisions"
        case activeFiles = "active_files"
        case customContext = "custom_context"
        case lastUpdated = "last_updated"
    }

    var isEmpty: Bool {
        normalizedProject == nil
            && normalizedCurrentState == nil
            && normalizedKeyDecisions.isEmpty
            && normalizedActiveFiles.isEmpty
            && normalizedCustomContext.isEmpty
    }

    var estimatedTokens: Int {
        let text = renderedMemory
        guard !text.isEmpty else {
            return 0
        }

        // This is a lightweight client-side approximation, so we keep the larger
        // of the character-based and whitespace-based heuristics to avoid
        // undercounting before the server applies its stricter token cap.
        let whitespaceTokens = text.split(whereSeparator: \.isWhitespace).count
        let characterTokens = (text.count + 3) / 4
        return max(characterTokens, whitespaceTokens, 1)
    }

    var sanitizedForSaving: SessionMemory {
        SessionMemory(
            project: normalizedProject,
            currentState: normalizedCurrentState,
            keyDecisions: normalizedKeyDecisions,
            activeFiles: normalizedActiveFiles,
            customContext: normalizedCustomContext,
            lastUpdated: lastUpdated
        )
    }

    private var normalizedProject: String? {
        normalized(project)
    }

    private var normalizedCurrentState: String? {
        normalized(currentState)
    }

    private var normalizedKeyDecisions: [String] {
        normalizedItems(keyDecisions)
    }

    private var normalizedActiveFiles: [String] {
        normalizedItems(activeFiles)
    }

    private var normalizedCustomContext: [String] {
        normalizedItems(customContext)
    }

    private var renderedMemory: String {
        let sanitized = sanitizedForSaving
        guard !sanitized.isEmpty else {
            return ""
        }

        var lines = ["[Session Memory]"]
        if let project = sanitized.project {
            lines.append("Project: \(project)")
        }
        if let currentState = sanitized.currentState {
            lines.append("Current state: \(currentState)")
        }
        appendRenderedItems(sanitized.keyDecisions, heading: "Key decisions:", into: &lines)
        appendRenderedItems(sanitized.activeFiles, heading: "Active files:", into: &lines)
        appendRenderedItems(sanitized.customContext, heading: "Context:", into: &lines)
        return lines.joined(separator: "\n")
    }
}

private extension SessionMemory {
    func normalized(_ value: String?) -> String? {
        value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    func normalizedItems(_ values: [String]) -> [String] {
        values.compactMap { normalized($0) }
    }

    func appendRenderedItems(
        _ items: [String],
        heading: String,
        into lines: inout [String]
    ) {
        guard !items.isEmpty else {
            return
        }

        lines.append(heading)
        for item in items {
            lines.append("- \(item)")
        }
    }
}

extension Session {
    static func sidebarSort(_ lhs: Session, _ rhs: Session) -> Bool {
        if lhs.updatedAt == rhs.updatedAt {
            return lhs.key < rhs.key
        }
        return lhs.updatedAt > rhs.updatedAt
    }

    mutating func applyPreview(_ previewText: String, model newModel: String?) {
        let trimmed = previewText.trimmingCharacters(in: .whitespacesAndNewlines)
        preview = trimmed.isEmpty ? nil : trimmed

        if title?.nonEmpty == nil, label?.nonEmpty == nil, !trimmed.isEmpty {
            title = summarizedSessionTitle(from: trimmed)
        }

        if let newModel, !newModel.isEmpty {
            model = newModel
        }

        updatedAt = Int(Date().timeIntervalSince1970)
    }
}

func summarizedSessionTitle(from text: String) -> String {
    let normalized = normalizedSessionTitleText(text)
    guard normalized.isEmpty == false else {
        return "New Session"
    }

    let strippedPrompt = strippedSessionPromptPrefix(from: normalized)
    let sentenceCased = sentenceCase(strippedPrompt.nonEmpty ?? normalized)
    return truncateSessionTitle(sentenceCased)
}

private func normalizedSessionTitleText(_ text: String) -> String {
    let withoutMarkdown = text
        .replacingOccurrences(of: #"[`*_]+"#, with: "", options: .regularExpression)
        .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)

    return withoutMarkdown
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .trimmingCharacters(in: CharacterSet(charactersIn: " .,!?:;-"))
}

func strippedSessionPromptPrefix(from text: String) -> String {
    let patterns = [
        #"^hey\s+fawx[!,]?\s*"#,
        #"^(?:can|could|would)\s+you\s+"#,
        #"^please\s+"#,
        #"^help\s+(?:me|us)\s+(?:with\s+)?"#,
        #"^show\s+(?:me|us)\s+"#,
        #"^tell\s+(?:me|us)\s+about\s+"#,
        #"^give\s+(?:me|us)\s+"#,
        #"^what\s+can\s+(?:i|we)\s+do\s+with\s+"#,
        #"^what\s+are\s+(?:a\s+few\s+)?ways\s+(?:i|we|you)\s+could\s+"#,
        #"^how\s+do\s+(?:i|we)\s+"#,
        #"^how\s+can\s+(?:i|we)\s+"#,
    ]

    var result = text
    for pattern in patterns {
        let updated = result.replacingOccurrences(
            of: pattern,
            with: "",
            options: [.regularExpression, .caseInsensitive]
        )
        if updated != result {
            result = updated
        }
    }

    result = result.replacingOccurrences(
        of: #"^(?:the|a|an)\s+"#,
        with: "",
        options: [.regularExpression, .caseInsensitive]
    )

    return result.trimmingCharacters(in: CharacterSet(charactersIn: " .,!?:;-"))
}

private func sentenceCase(_ text: String) -> String {
    guard let first = text.first else {
        return text
    }

    return String(first).uppercased() + text.dropFirst()
}

func truncateSessionTitle(_ text: String, maxLength: Int = 44) -> String {
    let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard cleaned.count > maxLength else {
        return cleaned
    }

    let cutoff = cleaned.index(cleaned.startIndex, offsetBy: maxLength)
    let truncatedSlice = cleaned[..<cutoff]
    let wordBoundary = truncatedSlice.lastIndex(of: " ") ?? cutoff
    return String(cleaned[..<wordBoundary]).trimmingCharacters(in: .whitespacesAndNewlines) + "..."
}
