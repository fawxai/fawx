import Foundation

private struct UsageTokenBuckets: Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let cachedInputTokens: Int
    let cacheCreationInputTokens: Int
}

private extension KeyedDecodingContainer {
    func decodeUsageTokenBuckets(
        input inputTokensKey: Key,
        output outputTokensKey: Key,
        cached cachedInputTokensKey: Key,
        cacheCreation cacheCreationInputTokensKey: Key
    ) throws -> UsageTokenBuckets {
        UsageTokenBuckets(
            inputTokens: try decode(Int.self, forKey: inputTokensKey),
            outputTokens: try decode(Int.self, forKey: outputTokensKey),
            cachedInputTokens: try decodeIfPresent(Int.self, forKey: cachedInputTokensKey) ?? 0,
            cacheCreationInputTokens: try decodeIfPresent(Int.self, forKey: cacheCreationInputTokensKey) ?? 0
        )
    }
}

struct UsageResponse: Codable, Sendable, Hashable {
    let session: SessionUsage
    let today: PeriodUsage
    let providers: [ProviderUsage]
}

struct SessionUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let cachedInputTokens: Int
    let cacheCreationInputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case cachedInputTokens = "cached_input_tokens"
        case cacheCreationInputTokens = "cache_creation_input_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let tokens = try container.decodeUsageTokenBuckets(
            input: .inputTokens,
            output: .outputTokens,
            cached: .cachedInputTokens,
            cacheCreation: .cacheCreationInputTokens
        )
        inputTokens = tokens.inputTokens
        outputTokens = tokens.outputTokens
        cachedInputTokens = tokens.cachedInputTokens
        cacheCreationInputTokens = tokens.cacheCreationInputTokens
        totalTokens = try container.decode(Int.self, forKey: .totalTokens)
        estimatedCostUsd = try container.decode(Double.self, forKey: .estimatedCostUsd)
    }
}

struct PeriodUsage: Codable, Sendable, Hashable {
    let inputTokens: Int
    let outputTokens: Int
    let cachedInputTokens: Int
    let cacheCreationInputTokens: Int
    let totalTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case cachedInputTokens = "cached_input_tokens"
        case cacheCreationInputTokens = "cache_creation_input_tokens"
        case totalTokens = "total_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let tokens = try container.decodeUsageTokenBuckets(
            input: .inputTokens,
            output: .outputTokens,
            cached: .cachedInputTokens,
            cacheCreation: .cacheCreationInputTokens
        )
        inputTokens = tokens.inputTokens
        outputTokens = tokens.outputTokens
        cachedInputTokens = tokens.cachedInputTokens
        cacheCreationInputTokens = tokens.cacheCreationInputTokens
        totalTokens = try container.decode(Int.self, forKey: .totalTokens)
        estimatedCostUsd = try container.decode(Double.self, forKey: .estimatedCostUsd)
    }
}

struct ProviderUsage: Codable, Sendable, Hashable {
    let provider: String
    let model: String
    let inputTokens: Int
    let outputTokens: Int
    let cachedInputTokens: Int
    let cacheCreationInputTokens: Int
    let estimatedCostUsd: Double

    enum CodingKeys: String, CodingKey {
        case provider
        case model
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case cachedInputTokens = "cached_input_tokens"
        case cacheCreationInputTokens = "cache_creation_input_tokens"
        case estimatedCostUsd = "estimated_cost_usd"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let tokens = try container.decodeUsageTokenBuckets(
            input: .inputTokens,
            output: .outputTokens,
            cached: .cachedInputTokens,
            cacheCreation: .cacheCreationInputTokens
        )
        provider = try container.decode(String.self, forKey: .provider)
        model = try container.decode(String.self, forKey: .model)
        inputTokens = tokens.inputTokens
        outputTokens = tokens.outputTokens
        cachedInputTokens = tokens.cachedInputTokens
        cacheCreationInputTokens = tokens.cacheCreationInputTokens
        estimatedCostUsd = try container.decode(Double.self, forKey: .estimatedCostUsd)
    }
}
