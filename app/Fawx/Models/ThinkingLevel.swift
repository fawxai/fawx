import Foundation

struct ThinkingLevel: RawRepresentable, Codable, Hashable, Sendable {
    let rawValue: String

    init(rawValue: String) {
        self.rawValue = rawValue
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        self.rawValue = try container.decode(String.self)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }

    static let off = Self(rawValue: "off")
    static let low = Self(rawValue: "low")
    static let adaptive = Self(rawValue: "adaptive")
    static let high = Self(rawValue: "high")
    static let defaultOptions: [ThinkingLevel] = [.off, .low, .adaptive, .high]

    var displayName: String {
        rawValue
            .replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "-", with: " ")
            .localizedCapitalized
    }
}

struct ThinkingConfig: Codable, Sendable, Hashable {
    let level: ThinkingLevel
    let budgetTokens: Int?
    let available: [ThinkingLevel]

    enum CodingKeys: String, CodingKey {
        case level
        case budgetTokens = "budget_tokens"
        case available
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        level = try container.decode(ThinkingLevel.self, forKey: .level)
        budgetTokens = try container.decodeIfPresent(Int.self, forKey: .budgetTokens)
        available = try container.decodeIfPresent([ThinkingLevel].self, forKey: .available)
            ?? ThinkingLevel.defaultOptions
    }
}

struct SetThinkingResponse: Codable, Sendable, Hashable {
    let previousLevel: ThinkingLevel
    let level: ThinkingLevel
    let budgetTokens: Int?
    let available: [ThinkingLevel]

    enum CodingKeys: String, CodingKey {
        case previousLevel = "previous_level"
        case level
        case budgetTokens = "budget_tokens"
        case available
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        previousLevel = try container.decode(ThinkingLevel.self, forKey: .previousLevel)
        level = try container.decode(ThinkingLevel.self, forKey: .level)
        budgetTokens = try container.decodeIfPresent(Int.self, forKey: .budgetTokens)
        available = try container.decodeIfPresent([ThinkingLevel].self, forKey: .available)
            ?? ThinkingLevel.defaultOptions
    }
}
