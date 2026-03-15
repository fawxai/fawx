import Foundation

struct ModelCatalogResponse: Codable, Sendable, Hashable {
    let activeModel: String
    let models: [ModelInfo]

    enum CodingKeys: String, CodingKey {
        case activeModel = "active_model"
        case models
    }
}

struct ModelInfo: Codable, Identifiable, Sendable, Hashable {
    let modelID: String
    let provider: String
    let authMethod: String

    var id: String { modelID }

    enum CodingKeys: String, CodingKey {
        case modelID = "model_id"
        case provider
        case authMethod = "auth_method"
    }
}

struct SetModelResponse: Codable, Sendable, Hashable {
    let previousModel: String
    let activeModel: String
    let thinkingAdjusted: ThinkingAdjusted?

    enum CodingKeys: String, CodingKey {
        case previousModel = "previous_model"
        case activeModel = "active_model"
        case thinkingAdjusted = "thinking_adjusted"
    }
}

struct ThinkingAdjusted: Codable, Sendable, Hashable {
    let from: ThinkingLevel
    let to: ThinkingLevel
    let reason: String
}
