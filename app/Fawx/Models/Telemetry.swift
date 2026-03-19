import Foundation

struct TelemetryConsentResponse: Codable, Sendable, Hashable {
    let enabled: Bool
    let categories: [String: TelemetryCategoryInfo]
    let updatedAt: String

    enum CodingKeys: String, CodingKey {
        case enabled
        case categories
        case updatedAt = "updated_at"
    }
}

struct TelemetryCategoryInfo: Codable, Sendable, Hashable {
    let enabled: Bool
    let description: String
}

struct TelemetryConsentPatchRequest: Encodable, Sendable {
    let enabled: Bool?
    let categories: [String: Bool]?
}

struct TelemetryCategory: Sendable, Hashable, Identifiable {
    private static let preferredOrder: [String: Int] = [
        "tool_usage": 0,
        "proposal_gate": 1,
        "experiments": 2,
        "errors": 3,
        "model_usage": 4,
        "performance": 5
    ]

    let name: String
    var enabled: Bool
    let description: String

    var id: String { name }

    var title: String {
        name
            .replacingOccurrences(of: "_", with: " ")
            .split(separator: " ")
            .map { $0.capitalized }
            .joined(separator: " ")
    }

    var sortOrder: Int {
        Self.preferredOrder[name] ?? Int.max
    }
}
