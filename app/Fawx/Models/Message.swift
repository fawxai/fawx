import Foundation

enum MessageRole: String, Codable, CaseIterable, Sendable, Hashable {
    case user
    case assistant
    case system
}

struct SessionMessage: Codable, Identifiable, Sendable, Hashable {
    let id: UUID
    let role: MessageRole
    let content: String
    let timestamp: Int

    init(id: UUID = UUID(), role: MessageRole, content: String, timestamp: Int) {
        self.id = id
        self.role = role
        self.content = content
        self.timestamp = timestamp
    }

    enum CodingKeys: String, CodingKey {
        case role
        case content
        case timestamp
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = UUID()
        role = try container.decode(MessageRole.self, forKey: .role)
        timestamp = try container.decode(Int.self, forKey: .timestamp)
        content = try Self.decodeContent(from: container)
    }

    private static func decodeContent(from container: KeyedDecodingContainer<CodingKeys>) throws -> String {
        // Try plain string first (legacy format)
        if let text = try? container.decode(String.self, forKey: .content) {
            return text
        }
        // Try structured content blocks (new format from PR #1542)
        if let blocks = try? container.decode([SessionContentBlock].self, forKey: .content) {
            return blocks.compactMap(\.displayText).joined(separator: "\n\n")
        }
        return ""
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(role, forKey: .role)
        try container.encode(content, forKey: .content)
        try container.encode(timestamp, forKey: .timestamp)
    }
}

enum SessionContentBlock: Codable, Sendable, Hashable {
    case text(String)
    case toolUse(id: String, name: String)
    case toolResult(toolUseId: String)
    case image(mediaType: String)

    var displayText: String? {
        switch self {
        case .text(let text): return text
        case .toolUse(_, let name): return "[\(name)]"
        case .toolResult: return nil
        case .image: return "[image]"
        }
    }

    enum CodingKeys: String, CodingKey {
        case type, text, id, name
        case toolUseId = "tool_use_id"
        case mediaType = "media_type"
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
            self = .toolUse(id: id, name: name)
        case "tool_result":
            let toolUseId = try container.decode(String.self, forKey: .toolUseId)
            self = .toolResult(toolUseId: toolUseId)
        case "image":
            let mediaType = try container.decode(String.self, forKey: .mediaType)
            self = .image(mediaType: mediaType)
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
        case .toolUse(let id, let name):
            try container.encode("tool_use", forKey: .type)
            try container.encode(id, forKey: .id)
            try container.encode(name, forKey: .name)
        case .toolResult(let toolUseId):
            try container.encode("tool_result", forKey: .type)
            try container.encode(toolUseId, forKey: .toolUseId)
        case .image(let mediaType):
            try container.encode("image", forKey: .type)
            try container.encode(mediaType, forKey: .mediaType)
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
