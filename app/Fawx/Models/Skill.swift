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
    enum FieldType: Sendable, Hashable {
        case text
        case secret
        case boolean
        case unknown(String)
    }

    let key: String
    let label: String
    let fieldType: FieldType
    let placeholder: String?
    let helpText: String?
    let required: Bool
    let minLength: Int?
    let maxLength: Int?
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
        case maxLength = "max_length"
        case pattern
    }

    init(
        key: String,
        label: String,
        fieldType: FieldType,
        placeholder: String?,
        helpText: String?,
        required: Bool,
        minLength: Int?,
        maxLength: Int?,
        pattern: String?
    ) {
        self.key = key
        self.label = label
        self.fieldType = fieldType
        self.placeholder = placeholder
        self.helpText = helpText
        self.required = required
        self.minLength = minLength
        self.maxLength = maxLength
        self.pattern = pattern
    }

    var supportsInlineEditing: Bool {
        switch fieldType {
        case .text, .secret, .boolean:
            true
        case .unknown:
            false
        }
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

        if let maxLength, trimmed.count > maxLength {
            return "\(label) must be at most \(maxLength) characters."
        }

        if fieldType == .boolean, trimmed != "true", trimmed != "false" {
            return "\(label) must be either true or false."
        }

        // The server is authoritative for regex validation because Foundation
        // and Rust's regex engines do not accept exactly the same syntax.
        return nil
    }
}

extension SkillSettingsField.FieldType: Codable {
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let rawValue = try container.decode(String.self)

        switch rawValue {
        case "text":
            self = .text
        case "secret":
            self = .secret
        case "boolean":
            self = .boolean
        default:
            self = .unknown(rawValue)
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()

        switch self {
        case .text:
            try container.encode("text")
        case .secret:
            try container.encode("secret")
        case .boolean:
            try container.encode("boolean")
        case .unknown(let rawValue):
            try container.encode(rawValue)
        }
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

    enum CodingKeys: String, CodingKey {
        case key
        case value
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(key, forKey: .key)

        if let value {
            try container.encode(value, forKey: .value)
        } else {
            try container.encodeNil(forKey: .value)
        }
    }
}

struct UpdateSkillSettingsResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let settings: SkillSettingsResponse
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
