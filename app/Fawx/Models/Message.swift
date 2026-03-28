import Foundation

enum MessageRole: String, Codable, CaseIterable, Sendable, Hashable {
    case user
    case assistant
    case system
    case tool
}

struct SessionMessage: Codable, Identifiable, Sendable, Hashable {
    let id: UUID
    let role: MessageRole
    let contentBlocks: [SessionContentBlock]
    let timestamp: Int

    var content: String {
        Self.renderStructuredContent(contentBlocks, role: role)
    }

    var transcriptDisplayText: String {
        Self.renderTranscriptContent(contentBlocks, role: role)
    }

    init(id: UUID = UUID(), role: MessageRole, content: String, timestamp: Int) {
        self.id = id
        self.role = role
        self.contentBlocks = [.text(content)]
        self.timestamp = timestamp
    }

    init(
        id: UUID = UUID(),
        role: MessageRole,
        contentBlocks: [SessionContentBlock],
        timestamp: Int
    ) {
        self.id = id
        self.role = role
        self.contentBlocks = contentBlocks
        self.timestamp = timestamp
    }

    enum CodingKeys: String, CodingKey {
        case role
        case content
        case timestamp
        case tokenCount = "token_count"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = UUID()
        role = (try? container.decode(MessageRole.self, forKey: .role)) ?? .system
        timestamp = (try? container.decode(Int.self, forKey: .timestamp)) ?? 0
        contentBlocks = try Self.decodeContentBlocks(from: container)
    }

    private static func decodeContentBlocks(from container: KeyedDecodingContainer<CodingKeys>) throws -> [SessionContentBlock] {
        // Try plain string first (legacy format)
        if let text = try? container.decode(String.self, forKey: .content) {
            return [.text(text)]
        }
        // Try structured content blocks (new format from PR #1542)
        if let blocks = try? container.decode([SessionContentBlock].self, forKey: .content) {
            return blocks
        }
        return []
    }

    private static func renderStructuredContent(
        _ blocks: [SessionContentBlock],
        role: MessageRole
    ) -> String {
        switch role {
        case .tool:
            let visibleBlocks = blocks.compactMap(\.toolTranscriptText)
            if !visibleBlocks.isEmpty {
                return visibleBlocks.joined(separator: "\n\n")
            }
            if blocks.contains(where: \.containsToolResult) {
                return "Tool output available."
            }
            return ""
        case .user, .assistant, .system:
            return blocks.compactMap(\.displayText).joined(separator: "\n\n")
        }
    }

    private static func renderTranscriptContent(
        _ blocks: [SessionContentBlock],
        role: MessageRole
    ) -> String {
        switch role {
        case .tool:
            let visibleBlocks = blocks.compactMap(\.toolTranscriptText)
            if !visibleBlocks.isEmpty {
                return visibleBlocks.joined(separator: "\n\n")
            }
            if blocks.contains(where: \.containsToolResult) {
                return "Tool output available."
            }
            return ""
        case .user, .assistant, .system:
            return blocks.compactMap(\.transcriptDisplayText).joined(separator: "\n\n")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(role, forKey: .role)
        try container.encode(contentBlocks, forKey: .content)
        try container.encode(timestamp, forKey: .timestamp)
    }
}

enum SessionContentBlock: Codable, Sendable, Hashable {
    case text(String)
    case toolUse(id: String, name: String, input: JSONValue)
    case toolResult(toolUseId: String, content: JSONValue, isError: Bool?)
    case image(mediaType: String, data: String?)
    case document(mediaType: String, data: String?, filename: String?)

    var displayText: String? {
        switch self {
        case .text(let text): return text
        case .toolUse:
            return nil
        case .toolResult:
            return nil
        case .image: return "[image]"
        case .document(_, _, let filename):
            return filename.map { "[document: \($0)]" } ?? "[document]"
        }
    }

    var transcriptDisplayText: String? {
        switch self {
        case .text(let text):
            return text
        case .image:
            return "[image]"
        case .document(_, _, let filename):
            return filename.map { "[document: \($0)]" } ?? "[document]"
        case .toolUse, .toolResult:
            return nil
        }
    }

    var toolTranscriptText: String? {
        switch self {
        case .text(let text):
            return text
        case .image:
            return "[image]"
        case .document(_, _, let filename):
            return filename.map { "[document: \($0)]" } ?? "[document]"
        case .toolUse, .toolResult:
            return nil
        }
    }

    var containsToolResult: Bool {
        if case .toolResult = self {
            return true
        }
        return false
    }

    enum CodingKeys: String, CodingKey {
        case type, text, id, name, input, content
        case isError = "is_error"
        case toolUseId = "tool_use_id"
        case mediaType = "media_type"
        case data
        case filename
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        switch type {
        case "text":
            let text = try container.decode(String.self, forKey: .text)
            self = .text(text)
        case "tool_use":
            let id = try container.decode(String.self, forKey: .id)
            let name = try container.decode(String.self, forKey: .name)
            let input = (try? container.decode(JSONValue.self, forKey: .input)) ?? .null
            self = .toolUse(id: id, name: name, input: input)
        case "tool_result":
            let toolUseId = try container.decode(String.self, forKey: .toolUseId)
            let content = (try? container.decode(JSONValue.self, forKey: .content)) ?? .null
            let isError = try? container.decode(Bool.self, forKey: .isError)
            self = .toolResult(toolUseId: toolUseId, content: content, isError: isError)
        case "image":
            let mediaType = try container.decode(String.self, forKey: .mediaType)
            let data = try? container.decode(String.self, forKey: .data)
            self = .image(mediaType: mediaType, data: data)
        case "document":
            let mediaType = try container.decode(String.self, forKey: .mediaType)
            let data = try? container.decode(String.self, forKey: .data)
            let filename = try? container.decode(String.self, forKey: .filename)
            self = .document(mediaType: mediaType, data: data, filename: filename)
        default:
            self = .text("[unknown block: \(type)]")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .text(let text):
            try container.encode("text", forKey: .type)
            try container.encode(text, forKey: .text)
        case .toolUse(let id, let name, let input):
            try container.encode("tool_use", forKey: .type)
            try container.encode(id, forKey: .id)
            try container.encode(name, forKey: .name)
            try container.encode(input, forKey: .input)
        case .toolResult(let toolUseId, let content, let isError):
            try container.encode("tool_result", forKey: .type)
            try container.encode(toolUseId, forKey: .toolUseId)
            try container.encode(content, forKey: .content)
            try container.encodeIfPresent(isError, forKey: .isError)
        case .image(let mediaType, let data):
            try container.encode("image", forKey: .type)
            try container.encode(mediaType, forKey: .mediaType)
            try container.encodeIfPresent(data, forKey: .data)
        case .document(let mediaType, let data, let filename):
            try container.encode("document", forKey: .type)
            try container.encode(mediaType, forKey: .mediaType)
            try container.encodeIfPresent(data, forKey: .data)
            try container.encodeIfPresent(filename, forKey: .filename)
        }
    }
}

struct MessagesResponse: Codable, Sendable, Hashable {
    let messages: [SessionMessage]
    let total: Int
}

struct MessageResponse: Codable, Sendable, Hashable {
    let response: String
    let model: String
    let iterations: Int
}

struct ImagePayload: Codable, Sendable, Hashable {
    let data: String
    let mediaType: String

    enum CodingKeys: String, CodingKey {
        case data
        case mediaType = "media_type"
    }
}

struct DocumentPayload: Codable, Sendable, Hashable {
    let data: String
    let mediaType: String
    let filename: String?

    enum CodingKeys: String, CodingKey {
        case data
        case mediaType = "media_type"
        case filename
    }
}

private extension JSONValue {
    var chatDisplayText: String {
        switch self {
        case .null:
            return ""
        case .string(let value):
            return value.trimmingCharacters(in: .whitespacesAndNewlines)
        default:
            return description.trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }
}
