import Foundation

struct AuthProvider: Codable, Identifiable, Sendable, Hashable {
    let provider: String
    let authMethods: [String]
    let modelCount: Int
    let status: String

    var id: String { provider }
    var displayName: String {
        switch provider.lowercased() {
        case "openai":
            return "OpenAI"
        case "anthropic":
            return "Anthropic"
        case "google":
            return "Google"
        case "openrouter":
            return "OpenRouter"
        default:
            return provider
                .replacingOccurrences(of: "-", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }
    }

    var isConfigured: Bool {
        switch status.lowercased() {
        case "not_configured", "unconfigured", "missing", "disabled", "unauthenticated":
            return false
        default:
            return true
        }
    }

    var displayStatus: String {
        isConfigured ? "Configured" : "Not configured"
    }

    var authMethodsSummary: String {
        let labels = authMethods.map { method in
            method
                .replacingOccurrences(of: "_", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }

        return labels.isEmpty ? "No auth methods reported" : labels.joined(separator: ", ")
    }

    enum CodingKeys: String, CodingKey {
        case provider
        case authMethods = "auth_methods"
        case modelCount = "model_count"
        case status
    }
}

struct AuthProvidersResponse: Codable, Sendable, Hashable {
    let providers: [AuthProvider]
}
