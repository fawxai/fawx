import Foundation

struct SynthesisResponse: Codable, Sendable, Hashable {
    let synthesis: String?
    let updatedAt: Int?
    let source: String
    let version: Int
    let maxLength: Int

    enum CodingKeys: String, CodingKey {
        case synthesis
        case source
        case version
        case updatedAt = "updated_at"
        case maxLength = "max_length"
    }
}

struct SetSynthesisRequest: Encodable, Sendable {
    let synthesis: String
    let version: Int?
}

struct SetSynthesisResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let synthesis: String
    let updatedAt: Int
    let version: Int

    enum CodingKeys: String, CodingKey {
        case updated
        case synthesis
        case version
        case updatedAt = "updated_at"
    }
}

struct ClearSynthesisResponse: Codable, Sendable, Hashable {
    let cleared: Bool
    let version: Int
}
