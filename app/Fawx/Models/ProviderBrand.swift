import Foundation

enum ProviderBrand: String, CaseIterable, Sendable {
    case anthropic
    case openai
    case openrouter
    case fireworks
    case google

    var companyName: String {
        switch self {
        case .anthropic:
            "Anthropic"
        case .openai:
            "OpenAI"
        case .openrouter:
            "OpenRouter"
        case .fireworks:
            "Fireworks AI"
        case .google:
            "Google"
        }
    }

    var setupDisplayName: String {
        switch self {
        case .anthropic:
            "Claude"
        case .openai:
            "ChatGPT"
        case .openrouter:
            "OpenRouter"
        case .fireworks:
            "Fireworks"
        case .google:
            "Google"
        }
    }

    static func resolve(_ providerID: String) -> ProviderBrand? {
        let normalizedProviderID = providerID
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        return ProviderBrand(rawValue: normalizedProviderID)
    }
}
