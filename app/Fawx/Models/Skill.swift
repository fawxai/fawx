import Foundation

struct SkillSummary: Codable, Identifiable, Sendable, Hashable {
    let name: String
    let description: String?
    let tools: [String]
    let capabilities: [String]

    var id: String { name }

    var displayDescription: String? {
        description?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    enum CodingKeys: String, CodingKey {
        case name
        case description
        case tools
        case capabilities
    }

    init(
        name: String,
        description: String?,
        tools: [String],
        capabilities: [String]
    ) {
        self.name = name
        self.description = description
        self.tools = tools
        self.capabilities = capabilities
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        description = try container.decodeIfPresent(String.self, forKey: .description)
        tools = try container.decodeIfPresent([String].self, forKey: .tools) ?? []
        capabilities = try container.decodeIfPresent([String].self, forKey: .capabilities) ?? []
    }

    static let editableCapabilities = [
        "network",
        "storage",
        "notifications",
        "sensors",
        "phone_actions",
    ]

    var unsupportedCapabilities: [String] {
        capabilities.filter { capability in
            !Self.editableCapabilities.contains(capability)
        }
    }
}

struct SkillsResponse: Codable, Sendable, Hashable {
    let skills: [SkillSummary]
    let total: Int
}

struct SkillSettingsField: Codable, Sendable, Hashable, Identifiable {
    enum FieldType: String, Codable, Sendable, Hashable {
        case text
        case secret
        case boolean
    }

    let key: String
    let label: String
    let fieldType: FieldType
    let placeholder: String?
    let helpText: String?
    let required: Bool
    let minLength: Int?
    let pattern: String?

    var id: String { key }

    enum CodingKeys: String, CodingKey {
        case key
        case label
        case fieldType = "field_type"
        case placeholder
        case helpText = "help_text"
        case required
        case minLength = "min_length"
        case pattern
    }

    func validate(_ value: String?) -> String? {
        let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines)

        if required && (trimmed?.isEmpty != false) {
            return "\(label) is required."
        }

        guard let trimmed, !trimmed.isEmpty else {
            return nil
        }

        if let minLength, trimmed.count < minLength {
            return "\(label) must be at least \(minLength) characters."
        }

        if let pattern,
           let regex = try? NSRegularExpression(pattern: pattern) {
            let range = NSRange(trimmed.startIndex..<trimmed.endIndex, in: trimmed)
            if regex.firstMatch(in: trimmed, options: [], range: range) == nil {
                return "\(label) is invalid."
            }
        }

        if fieldType == .boolean, trimmed != "true", trimmed != "false" {
            return "\(label) must be either true or false."
        }

        return nil
    }
}

struct SkillSettingsSchema: Codable, Sendable, Hashable {
    let version: Int
    let fields: [SkillSettingsField]
}

struct SkillSettingValue: Codable, Sendable, Hashable {
    let key: String
    let value: String?
    let isSecret: Bool
    let isConfigured: Bool

    enum CodingKeys: String, CodingKey {
        case key
        case value
        case isSecret = "is_secret"
        case isConfigured = "is_configured"
    }
}

struct SkillSettingsResponse: Codable, Sendable, Hashable {
    let skillName: String
    let schema: SkillSettingsSchema
    let values: [SkillSettingValue]

    enum CodingKeys: String, CodingKey {
        case skillName = "skill_name"
        case schema
        case values
    }
}

struct SkillSettingInput: Codable, Sendable, Hashable {
    let key: String
    let value: String?
}

struct UpdateSkillSettingsResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let restartRequired: Bool
    let settings: SkillSettingsResponse

    enum CodingKeys: String, CodingKey {
        case updated
        case restartRequired = "restart_required"
        case settings
    }
}

struct UpdateSkillPermissionsResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let name: String
    let capabilities: [String]
    let restartRequired: Bool

    enum CodingKeys: String, CodingKey {
        case updated
        case name
        case capabilities
        case restartRequired = "restart_required"
    }
}
