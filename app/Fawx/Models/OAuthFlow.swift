import Foundation

struct OAuthStartResponse: Codable, Sendable, Hashable {
    let provider: String
    let authorizeUrl: String
    let flowToken: String
    let redirectUri: String

    enum CodingKeys: String, CodingKey {
        case provider
        case authorizeUrl = "authorize_url"
        case flowToken = "flow_token"
        case redirectUri = "redirect_uri"
    }
}

struct OAuthCallbackRequest: Encodable, Sendable {
    let code: String
    let flowToken: String

    enum CodingKeys: String, CodingKey {
        case code
        case flowToken = "flow_token"
    }
}

struct OAuthCallbackResponse: Codable, Sendable, Hashable {
    let provider: String
    let status: String
    let authMethod: String
    let verified: Bool

    enum CodingKeys: String, CodingKey {
        case provider
        case status
        case verified
        case authMethod = "auth_method"
    }
}
