import Foundation

struct AuthProvider: Codable, Identifiable, Sendable, Hashable {
    let provider: String
    let authMethods: [String]
    let modelCount: Int
    let status: String

    var id: String { provider }
    var displayName: String {
        switch provider.lowercased() {
        case "github":
            return "GitHub"
        case "openai":
            return "OpenAI"
        case "anthropic":
            return "Anthropic"
        case "google":
            return "Google"
        case "openrouter":
            return "OpenRouter"
        case "fireworks":
            return "Fireworks"
        default:
            return provider
                .replacingOccurrences(of: "-", with: " ")
                .split(separator: " ")
                .map { $0.capitalized }
                .joined(separator: " ")
        }
    }

    private var normalizedStatus: String {
        status.lowercased()
    }

    var isConfigured: Bool {
        switch normalizedStatus {
        case "not_configured", "unconfigured", "missing", "disabled", "unauthenticated":
            return false
        default:
            return true
        }
    }

    var displayStatus: String {
        switch normalizedStatus {
        case "saved":
            "Saved"
        case "authenticated", "verified":
            "Verified"
        case "invalid":
            "Invalid"
        case "registered":
            "Configured"
        case "not_configured", "unconfigured", "missing", "disabled", "unauthenticated":
            "Not configured"
        default:
            isConfigured ? "Configured" : "Not configured"
        }
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
