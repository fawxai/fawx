import Foundation

struct AuthProvider: Codable, Identifiable, Sendable, Hashable {
    let provider: String
    let authMethods: [String]
    let modelCount: Int
    let status: String

    var id: String { provider }

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
