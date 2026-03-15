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
        content = try container.decode(String.self, forKey: .content)
        timestamp = try container.decode(Int.self, forKey: .timestamp)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(role, forKey: .role)
        try container.encode(content, forKey: .content)
        try container.encode(timestamp, forKey: .timestamp)
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
