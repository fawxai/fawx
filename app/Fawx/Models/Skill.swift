import Foundation

struct SkillSummary: Codable, Identifiable, Sendable, Hashable {
    let name: String
    let description: String?
    let tools: [String]

    var id: String { name }

    var displayDescription: String? {
        description?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    var isEnabled: Bool {
        true
    }
}

struct SkillsResponse: Codable, Sendable, Hashable {
    let skills: [SkillSummary]
    let total: Int
}

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
