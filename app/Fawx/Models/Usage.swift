import Foundation

struct UsageResponse: Codable, Sendable, Hashable {
    let session: SessionUsage
    let today: PeriodUsage
    let providers: [ProviderUsage]
}

struct SessionUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}

struct PeriodUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}

struct ProviderUsage: Codable, Sendable, Hashable {
    let provider: String
    let model: String
    let inputTokens: Int
    let outputTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case provider
        case model
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }
}
