import Foundation

enum APIError: LocalizedError, Sendable {
    case notConfigured
    case authenticationRequired
    case invalidURL(String)
    case invalidResponse
    case httpStatus(Int, String?)
    case decoding(String)
    case streamError(String)

    var errorDescription: String? {
        switch self {
        case .notConfigured:
            return "Server URL is not configured."
        case .authenticationRequired:
            return "This device is not paired yet."
        case .invalidURL(let value):
            return "Invalid URL: \(value)"
        case .invalidResponse:
            return "The server returned an invalid response."
        case .httpStatus(let code, let message):
            if let message, !message.isEmpty {
                return message
            }
            return "The server returned HTTP \(code)."
        case .decoding(let message):
            return "Failed to decode server response: \(message)"
        case .streamError(let message):
            return message
        }
    }

    var statusCode: Int? {
        switch self {
        case .httpStatus(let code, _):
            code
        default:
            nil
        }
    }
}
