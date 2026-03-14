import Foundation

enum MessageRole: String, Codable, CaseIterable, Sendable, Hashable {
    case user
    case assistant
    case system
}

struct SessionMessage: Codable, Identifiable, Sendable, Hashable {
    let role: MessageRole
    let content: String
    let timestamp: Int

    var id: String {
        "\(role.rawValue)-\(timestamp)-\(content)"
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
