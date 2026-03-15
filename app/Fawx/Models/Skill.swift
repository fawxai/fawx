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
}

struct SkillsResponse: Codable, Sendable, Hashable {
    let skills: [SkillSummary]
    let total: Int
}
