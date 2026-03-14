import Foundation

struct HealthResponse: Codable, Sendable, Hashable {
    let status: String
    let model: String
    let uptimeSeconds: Int
    let skillsLoaded: Int

    enum CodingKeys: String, CodingKey {
        case status
        case model
        case uptimeSeconds = "uptime_seconds"
        case skillsLoaded = "skills_loaded"
    }
}

struct ServerStatusResponse: Codable, Sendable, Hashable {
    let status: String
    let model: String
    let skills: [String]
    let memoryEntries: Int
    let tailscaleIP: String?
    let config: JSONValue?

    enum CodingKeys: String, CodingKey {
        case status
        case model
        case skills
        case memoryEntries = "memory_entries"
        case tailscaleIP = "tailscale_ip"
        case config
    }
}

struct ContextInfo: Codable, Sendable, Hashable {
    let usedTokens: Int
    let maxTokens: Int
    let percentage: Double
    let compactionThreshold: Double

    enum CodingKeys: String, CodingKey {
        case usedTokens = "used_tokens"
        case maxTokens = "max_tokens"
        case percentage
        case compactionThreshold = "compaction_threshold"
    }
}
