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

extension Session {
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

private func summarizedSessionTitle(from text: String) -> String {
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

private func strippedSessionPromptPrefix(from text: String) -> String {
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

private func truncateSessionTitle(_ text: String, maxLength: Int = 44) -> String {
    let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard cleaned.count > maxLength else {
        return cleaned
    }

    let cutoff = cleaned.index(cleaned.startIndex, offsetBy: maxLength)
    let truncatedSlice = cleaned[..<cutoff]
    let wordBoundary = truncatedSlice.lastIndex(of: " ") ?? cutoff
    return String(cleaned[..<wordBoundary]).trimmingCharacters(in: .whitespacesAndNewlines) + "..."
}

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
