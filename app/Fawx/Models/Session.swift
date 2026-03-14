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
        for value in [title, label] {
            if let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty {
                return trimmed
            }
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
            title = truncatedSessionTitle(from: trimmed)
        }

        if let newModel, !newModel.isEmpty {
            model = newModel
        }

        updatedAt = Int(Date().timeIntervalSince1970)
    }
}

private func truncatedSessionTitle(from text: String) -> String {
    let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard cleaned.count > 60 else {
        return cleaned
    }

    let cutoff = cleaned.index(cleaned.startIndex, offsetBy: 60)
    return String(cleaned[..<cutoff]).trimmingCharacters(in: .whitespacesAndNewlines) + "..."
}

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
