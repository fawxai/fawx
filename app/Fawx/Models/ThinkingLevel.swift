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
    static let none = Self(rawValue: "none")
    static let low = Self(rawValue: "low")
    static let medium = Self(rawValue: "medium")
    static let adaptive = Self(rawValue: "adaptive")
    static let high = Self(rawValue: "high")

    static func disabledLevel(in levels: [ThinkingLevel]) -> ThinkingLevel? {
        if levels.contains(.off) {
            return .off
        }
        if levels.contains(.none) {
            return ThinkingLevel.none
        }
        return nil
    }

    var displayName: String {
        let displayValue = self == .none ? ThinkingLevel.off.rawValue : rawValue
        return displayValue
            .replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "-", with: " ")
            .localizedCapitalized
    }
}

struct ThinkingConfig: Decodable, Sendable, Hashable {
    let level: ThinkingLevel
    let budgetTokens: Int?
    let validLevels: [ThinkingLevel]

    enum CodingKeys: String, CodingKey {
        case level
        case budgetTokens = "budget_tokens"
        case validLevels = "valid_levels"
        case available
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        level = try container.decode(ThinkingLevel.self, forKey: .level)
        budgetTokens = try container.decodeIfPresent(Int.self, forKey: .budgetTokens)
        validLevels = try container.decodeIfPresent([ThinkingLevel].self, forKey: .validLevels)
            ?? container.decodeIfPresent([ThinkingLevel].self, forKey: .available)
            ?? [level]
    }
}

struct SetThinkingResponse: Decodable, Sendable, Hashable {
    let previousLevel: ThinkingLevel
    let level: ThinkingLevel
    let budgetTokens: Int?
    let validLevels: [ThinkingLevel]

    enum CodingKeys: String, CodingKey {
        case previousLevel = "previous_level"
        case level
        case budgetTokens = "budget_tokens"
        case validLevels = "valid_levels"
        case available
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        previousLevel = try container.decode(ThinkingLevel.self, forKey: .previousLevel)
        level = try container.decode(ThinkingLevel.self, forKey: .level)
        budgetTokens = try container.decodeIfPresent(Int.self, forKey: .budgetTokens)
        validLevels = try container.decodeIfPresent([ThinkingLevel].self, forKey: .validLevels)
            ?? container.decodeIfPresent([ThinkingLevel].self, forKey: .available)
            ?? [level]
    }
}
