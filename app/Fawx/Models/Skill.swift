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
