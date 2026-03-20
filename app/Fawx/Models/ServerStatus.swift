import Foundation

struct HealthResponse: Codable, Sendable, Hashable {
    let status: String
    let model: String
    let uptimeSeconds: Int
    let skillsLoaded: Int
    let httpsEnabled: Bool?

    enum CodingKeys: String, CodingKey {
        case status
        case model
        case uptimeSeconds = "uptime_seconds"
        case skillsLoaded = "skills_loaded"
        case httpsEnabled = "https_enabled"
    }
}

struct SetupStatusResponse: Codable, Sendable, Hashable {
    let mode: String
    let setupComplete: Bool
    let hasValidConfig: Bool
    let serverRunning: Bool
    let launchagent: SetupLaunchAgentStatus
    let localServer: SetupLocalServerStatus
    let auth: SetupAuthStatus
    let tailscale: SetupTailscaleStatus

    enum CodingKeys: String, CodingKey {
        case mode
        case setupComplete = "setup_complete"
        case hasValidConfig = "has_valid_config"
        case serverRunning = "server_running"
        case launchagent
        case localServer = "local_server"
        case auth
        case tailscale
    }
}

struct SetupLaunchAgentStatus: Codable, Sendable, Hashable {
    let installed: Bool
    let loaded: Bool
    let autoStartEnabled: Bool

    enum CodingKeys: String, CodingKey {
        case installed
        case loaded
        case autoStartEnabled = "auto_start_enabled"
    }
}

struct SetupLocalServerStatus: Codable, Sendable, Hashable {
    let host: String
    let port: Int
    let httpsEnabled: Bool

    enum CodingKeys: String, CodingKey {
        case host
        case port
        case httpsEnabled = "https_enabled"
    }
}

struct SetupAuthStatus: Codable, Sendable, Hashable {
    let bearerTokenPresent: Bool
    let providersConfigured: [String]

    enum CodingKeys: String, CodingKey {
        case bearerTokenPresent = "bearer_token_present"
        case providersConfigured = "providers_configured"
    }
}

struct SetupTailscaleStatus: Codable, Sendable, Hashable {
    let installed: Bool
    let running: Bool
    let loggedIn: Bool
    let hostname: String?
    let certReady: Bool

    enum CodingKeys: String, CodingKey {
        case installed
        case running
        case loggedIn = "logged_in"
        case hostname
        case certReady = "cert_ready"
    }
}

struct QrPairingResponse: Codable, Sendable, Hashable {
    let schemeURL: String
    let displayHost: String
    let port: Int
    let transport: String
    let sameNetworkOnly: Bool

    enum CodingKeys: String, CodingKey {
        case schemeURL = "scheme_url"
        case displayHost = "display_host"
        case port
        case transport
        case sameNetworkOnly = "same_network_only"
    }
}

struct PairingCodeResponse: Codable, Sendable, Hashable {
    let code: String
    let expiresAt: Int
    let ttlSeconds: Int

    enum CodingKeys: String, CodingKey {
        case code
        case expiresAt = "expires_at"
        case ttlSeconds = "ttl_seconds"
    }
}

struct LocalServerRuntimeStatus: Codable, Sendable, Hashable {
    let status: String
    let version: String
    let uptimeSeconds: Int
    let pid: UInt32
    let host: String
    let port: Int
    let httpsEnabled: Bool

    enum CodingKeys: String, CodingKey {
        case status
        case version
        case uptimeSeconds = "uptime_seconds"
        case pid
        case host
        case port
        case httpsEnabled = "https_enabled"
    }
}

struct ServerRestartControlResponse: Codable, Sendable, Hashable {
    let accepted: Bool
    let restartVia: String
    let message: String

    enum CodingKeys: String, CodingKey {
        case accepted
        case restartVia = "restart_via"
        case message
    }
}

struct ServerStopControlResponse: Codable, Sendable, Hashable {
    let stopped: Bool
    let message: String
}

struct LaunchAgentStatusResponse: Codable, Sendable, Hashable {
    let installed: Bool
    let loaded: Bool
}

struct LaunchAgentInstallResponse: Codable, Sendable, Hashable {
    let installed: Bool
    let message: String
}

struct LaunchAgentUninstallResponse: Codable, Sendable, Hashable {
    let uninstalled: Bool
    let message: String
}

struct ConfigPatchResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let restartRequired: Bool
    let changedKeys: [String]

    enum CodingKeys: String, CodingKey {
        case updated
        case restartRequired = "restart_required"
        case changedKeys = "changed_keys"
    }
}

struct ProviderAuthActionResponse: Codable, Sendable, Hashable {
    let provider: String
    let status: String
    let authMethod: String
    let modelCount: Int
    let verified: Bool

    enum CodingKeys: String, CodingKey {
        case provider
        case status
        case authMethod = "auth_method"
        case modelCount = "model_count"
        case verified
    }
}

struct ProviderVerificationResponse: Codable, Sendable, Hashable {
    let provider: String
    let verified: Bool
    let status: String
    let message: String
    let checkedAt: Int

    enum CodingKeys: String, CodingKey {
        case provider
        case verified
        case status
        case message
        case checkedAt = "checked_at"
    }
}

struct DeleteProviderResponse: Codable, Sendable, Hashable {
    let provider: String
    let removed: Bool
}

struct TailscaleCertResponse: Codable, Sendable, Hashable {
    let success: Bool
    let hostname: String
    let certPath: String
    let keyPath: String
    let httpsEnabled: Bool

    enum CodingKeys: String, CodingKey {
        case success
        case hostname
        case certPath = "cert_path"
        case keyPath = "key_path"
        case httpsEnabled = "https_enabled"
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

    var normalizedPercentage: Double {
        let reportedPercentage = percentage <= 1 ? percentage * 100 : percentage
        if reportedPercentage.isFinite {
            return max(0, min(reportedPercentage, 100))
        }

        guard usedTokens > 0, maxTokens > 0 else {
            return 0
        }

        let derivedPercentage = (Double(usedTokens) / Double(maxTokens)) * 100
        return max(0, min(derivedPercentage, 100))
    }
}
