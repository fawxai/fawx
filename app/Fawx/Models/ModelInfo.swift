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
    let displayName: String?
    let recommended: Bool
    let thinkingLevels: [ThinkingLevel]

    var id: String { modelID }
    var dataTrust: ModelDataTrust {
        ModelDataTrust.classify(modelID: modelID, provider: provider)
    }

    init(
        modelID: String,
        provider: String,
        authMethod: String,
        displayName: String? = nil,
        recommended: Bool = true,
        thinkingLevels: [ThinkingLevel] = [.off]
    ) {
        self.modelID = modelID
        self.provider = provider
        self.authMethod = authMethod
        self.displayName = displayName
        self.recommended = recommended
        self.thinkingLevels = thinkingLevels
    }

    enum CodingKeys: String, CodingKey {
        case modelID = "model_id"
        case provider
        case authMethod = "auth_method"
        case displayName = "display_name"
        case recommended
        case thinkingLevels = "thinking_levels"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        modelID = try container.decode(String.self, forKey: .modelID)
        provider = try container.decode(String.self, forKey: .provider)
        authMethod = try container.decode(String.self, forKey: .authMethod)
        displayName = try container.decodeIfPresent(String.self, forKey: .displayName)
        recommended = try container.decodeIfPresent(Bool.self, forKey: .recommended) ?? true
        thinkingLevels = try container.decodeIfPresent([ThinkingLevel].self, forKey: .thinkingLevels) ?? [.off]
    }
}

enum ModelDataTrust: String, Codable, Sendable, Hashable, CaseIterable {
    case providerDirect
    case knownRouter
    case freeOrUntrusted
    case unknown

    var title: String {
        switch self {
        case .providerDirect:
            return "Provider Direct"
        case .knownRouter:
            return "Known Router"
        case .freeOrUntrusted:
            return "Free/Untrusted"
        case .unknown:
            return "Unknown Route"
        }
    }

    var shortTitle: String {
        switch self {
        case .providerDirect:
            return "Direct"
        case .knownRouter:
            return "Router"
        case .freeOrUntrusted:
            return "Free/Untrusted"
        case .unknown:
            return "Unknown"
        }
    }

    var detail: String {
        switch self {
        case .providerDirect:
            return "Direct provider account route. Best default for private code and sensitive work."
        case .knownRouter:
            return "Requests go through a router or brokered path. Provider dashboards may report the backing model."
        case .freeOrUntrusted:
            return "Free/community routing can leave the intended provider boundary. Avoid private work unless explicitly trusted."
        case .unknown:
            return "No explicit data route contract is known. Verify before using private code or secrets."
        }
    }

    static func classify(modelID: String, provider: String) -> ModelDataTrust {
        let normalizedModelID = modelID.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let normalizedProvider = provider.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()

        if isFreeOrUntrustedRoute(modelID: normalizedModelID, provider: normalizedProvider) {
            return .freeOrUntrusted
        }

        if normalizedProvider == "openrouter" || normalizedModelID.contains("/routers/") {
            return .knownRouter
        }

        if isDirectProvider(normalizedProvider) {
            return .providerDirect
        }

        return .unknown
    }

    private static func isFreeOrUntrustedRoute(modelID: String, provider: String) -> Bool {
        provider == "openrouter" && (
            modelID.hasSuffix(":free")
                || modelID.contains(":free/")
                || modelID.contains("/free/")
                || modelID.contains("-free")
        )
    }

    private static func isDirectProvider(_ provider: String) -> Bool {
        switch provider {
        case "anthropic", "openai", "fireworks":
            return true
        default:
            return false
        }
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
