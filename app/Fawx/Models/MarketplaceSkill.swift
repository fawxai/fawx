import Foundation

struct MarketplaceSkillSummary: Codable, Sendable, Hashable, Identifiable {
    let name: String
    let title: String
    let description: String
    let publisher: String
    let signed: Bool

    var id: String { name }
}

struct SkillSearchResponse: Codable, Sendable, Hashable {
    let query: String
    let skills: [MarketplaceSkillSummary]
    let total: Int
    let marketplaceAvailable: Bool
    let message: String

    enum CodingKeys: String, CodingKey {
        case query
        case skills
        case total
        case message
        case marketplaceAvailable = "marketplace_available"
    }
}
