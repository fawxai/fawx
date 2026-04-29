import Foundation

struct SkillSummary: Codable, Identifiable, Sendable, Hashable {
    let name: String
    let description: String?
    let tools: [String]
    let capabilities: [String]
    let version: String?
    let source: String?
    let revisionHash: String?
    let activatedAtMs: UInt64?
    let signatureStatus: String?
    /// Opaque drift detail from the server. Non-nil means the installed source
    /// no longer matches the active loaded revision. UI should treat this as an
    /// update-available signal and must not depend on the exact string shape.
    let staleSource: String?

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
        case version
        case source
        case revisionHash = "revision_hash"
        case activatedAtMs = "activated_at_ms"
        case signatureStatus = "signature_status"
        case staleSource = "stale_source"
    }

    init(
        name: String,
        description: String?,
        tools: [String],
        capabilities: [String],
        version: String? = nil,
        source: String? = nil,
        revisionHash: String? = nil,
        activatedAtMs: UInt64? = nil,
        signatureStatus: String? = nil,
        staleSource: String? = nil
    ) {
        self.name = name
        self.description = description
        self.tools = tools
        self.capabilities = capabilities
        self.version = version
        self.source = source
        self.revisionHash = revisionHash
        self.activatedAtMs = activatedAtMs
        self.signatureStatus = signatureStatus
        self.staleSource = staleSource
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        description = try container.decodeIfPresent(String.self, forKey: .description)
        tools = try container.decodeIfPresent([String].self, forKey: .tools) ?? []
        capabilities = try container.decodeIfPresent([String].self, forKey: .capabilities) ?? []
        version = try container.decodeIfPresent(String.self, forKey: .version)
        source = try container.decodeIfPresent(String.self, forKey: .source)
        revisionHash = try container.decodeIfPresent(String.self, forKey: .revisionHash)
        activatedAtMs = try container.decodeIfPresent(UInt64.self, forKey: .activatedAtMs)
        signatureStatus = try container.decodeIfPresent(String.self, forKey: .signatureStatus)
        staleSource = try container.decodeIfPresent(String.self, forKey: .staleSource)
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

    var displayName: String {
        switch name.lowercased() {
        case "github":
            return "GitHub"
        case "stt":
            return "STT"
        case "tts":
            return "TTS"
        default:
            return name
                .replacingOccurrences(of: "-", with: " ")
                .replacingOccurrences(of: "_", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }
    }

    var isBuiltin: Bool {
        source?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "builtin"
    }

    var isInstallableSkill: Bool {
        !isBuiltin
    }

    var hasStaleSource: Bool {
        staleSource?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
    }

    var staleSourceMessage: String? {
        guard hasStaleSource else {
            return nil
        }

        return "Installed source changed since this revision was loaded. Restart the server to activate the latest skill version."
    }

    var loadedStatusLabel: String {
        if isBuiltin {
            return "Built-in"
        }
        if hasStaleSource {
            return "Update available"
        }
        return "Installed"
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

    // Identifiable conformance: key is guaranteed unique by server-side validation.
    // Using key alone is sufficient; label is included only for extra safety against
    // hypothetical server regressions (duplicate keys would assert in debug SwiftUI lists).
    var id: String { key }

    enum CodingKeys: String, CodingKey {
        case key
        case label
        case fieldType = "field_type"
        case legacyFieldType = "type"
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

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        key = try container.decode(String.self, forKey: .key)
        label = try container.decode(String.self, forKey: .label)

        if let fieldType = try container.decodeIfPresent(FieldType.self, forKey: .fieldType) {
            self.fieldType = fieldType
        } else if let legacyFieldType = try container.decodeIfPresent(FieldType.self, forKey: .legacyFieldType) {
            self.fieldType = legacyFieldType
        } else {
            // Graceful degradation: default to unknown instead of crashing
            self.fieldType = .unknown("missing")
        }

        placeholder = try container.decodeIfPresent(String.self, forKey: .placeholder)
        helpText = try container.decodeIfPresent(String.self, forKey: .helpText)
        required = try container.decodeIfPresent(Bool.self, forKey: .required) ?? false
        minLength = try container.decodeIfPresent(Int.self, forKey: .minLength)
        maxLength = try container.decodeIfPresent(Int.self, forKey: .maxLength)
        pattern = try container.decodeIfPresent(String.self, forKey: .pattern)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(key, forKey: .key)
        try container.encode(label, forKey: .label)
        try container.encode(fieldType, forKey: .fieldType)
        try container.encodeIfPresent(placeholder, forKey: .placeholder)
        try container.encodeIfPresent(helpText, forKey: .helpText)
        try container.encode(required, forKey: .required)
        try container.encodeIfPresent(minLength, forKey: .minLength)
        try container.encodeIfPresent(maxLength, forKey: .maxLength)
        try container.encodeIfPresent(pattern, forKey: .pattern)
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
