import Foundation

enum PermissionMode: String, Codable, Sendable, Hashable, CaseIterable {
    case capability
    case prompt

    var showsPermissionPrompts: Bool {
        self == .prompt
    }
}

struct PermissionEntry: Codable, Sendable, Hashable, Identifiable {
    let action: String
    let level: String
    let title: String

    var id: String { action }
}

struct PermissionsResponse: Codable, Sendable, Hashable {
    let preset: String
    let mode: PermissionMode
    let permissions: [PermissionEntry]
    let availablePresets: [String]

    enum CodingKeys: String, CodingKey {
        case preset
        case mode
        case permissions
        case availablePresets = "available_presets"
    }

    init(
        preset: String,
        mode: PermissionMode,
        permissions: [PermissionEntry],
        availablePresets: [String]
    ) {
        self.preset = preset
        self.mode = mode
        self.permissions = permissions
        self.availablePresets = availablePresets
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        preset = try container.decode(String.self, forKey: .preset)

        let rawMode = try container.decodeIfPresent(String.self, forKey: .mode)?.lowercased()
        mode = rawMode.flatMap(PermissionMode.init(rawValue:)) ?? .prompt

        permissions = try container.decode([PermissionEntry].self, forKey: .permissions)
        availablePresets = try container.decode([String].self, forKey: .availablePresets)
    }
}

struct PermissionsPatchRequest: Encodable, Sendable {
    let preset: String?
    let mode: PermissionMode?
    let changes: [PermissionChange]?

    var legacyCompatibleRequest: PermissionsPatchRequest? {
        guard mode != .capability else {
            return nil
        }

        let translatedChanges = changes?.map { change in
            PermissionChange(
                action: change.action,
                level: change.level.lowercased() == "ask" ? "propose" : change.level
            )
        }
        let translatedAskLevel = changes?.contains(where: { $0.level.lowercased() == "ask" }) == true

        let droppedPromptMode = mode == .prompt && (preset != nil || translatedChanges?.isEmpty == false)
        guard translatedAskLevel || droppedPromptMode else {
            return nil
        }

        return PermissionsPatchRequest(
            preset: preset,
            mode: droppedPromptMode ? nil : mode,
            changes: translatedChanges
        )
    }
}

struct PermissionChange: Codable, Sendable, Hashable {
    let action: String
    let level: String
}

func editablePermissionLevel(_ level: String) -> String {
    switch level.lowercased() {
    case "allow":
        "allow"
    case "deny":
        "deny"
    case "ask", "propose", "denied":
        "ask"
    default:
        "ask"
    }
}

struct PermissionsPatchResponse: Codable, Sendable, Hashable {
    let updated: Bool
    let preset: String
    let changedActions: [String]

    enum CodingKeys: String, CodingKey {
        case updated
        case preset
        case changedActions = "changed_actions"
    }
}

enum PermissionPromptDecision: String, Encodable, Sendable, Hashable {
    case allow
    case deny
    case allowSession = "allow_session"

    var buttonTitle: String {
        switch self {
        case .allow:
            return "Allow"
        case .deny:
            return "Deny"
        case .allowSession:
            return "Allow for Session"
        }
    }
}

struct PermissionPrompt: Decodable, Sendable, Hashable, Identifiable {
    let id: String
    let action: String
    let path: String
    let tier: Int?
    let sessionScopedAllowAvailable: Bool
    let expiresAt: UInt64?

    init(
        id: String,
        action: String,
        path: String,
        tier: Int? = nil,
        sessionScopedAllowAvailable: Bool = true,
        expiresAt: UInt64? = nil
    ) {
        self.id = id
        self.action = action
        self.path = path
        self.tier = tier
        self.sessionScopedAllowAvailable = sessionScopedAllowAvailable
        self.expiresAt = expiresAt
    }

    var summaryText: String {
        if displayPath.isEmpty {
            return "Fawx wants to \(displayAction)."
        }

        return "Fawx wants to \(displayAction) \(displayPath)."
    }

    var indicatorText: String {
        if displayPath.isEmpty {
            return "Approval needed: \(displayAction)"
        }

        return "Approval needed: \(displayAction) \(displayPath)"
    }

    var tierLabel: String? {
        guard let tier else {
            return nil
        }

        return "Tier \(tier)"
    }

    var displayAction: String {
        let trimmed = action.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "perform this action" : trimmed
    }

    var displayPath: String {
        path.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private enum CodingKeys: String, CodingKey {
        case id
        case action
        case path
        case tier
        case tool
        case title
        case reason
        case requestSummary = "request_summary"
        case sessionScopedAllowAvailable = "session_scoped_allow_available"
        case expiresAt = "expires_at"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        id = try container.decode(String.self, forKey: .id)

        let explicitAction = try container.decodeIfPresent(String.self, forKey: .action)
        let explicitPath = try container.decodeIfPresent(String.self, forKey: .path)
        let tool = try container.decodeIfPresent(String.self, forKey: .tool)
        let title = try container.decodeIfPresent(String.self, forKey: .title)
        let reason = try container.decodeIfPresent(String.self, forKey: .reason)
        let requestSummary = try container.decodeIfPresent(String.self, forKey: .requestSummary)

        action = Self.resolveAction(
            explicitAction: explicitAction,
            tool: tool,
            title: title,
            reason: reason
        )
        path = Self.resolvePath(
            explicitPath: explicitPath,
            requestSummary: requestSummary,
            reason: reason
        )
        tier = try container.decodeIfPresent(Int.self, forKey: .tier)
        sessionScopedAllowAvailable = try container.decodeIfPresent(
            Bool.self,
            forKey: .sessionScopedAllowAvailable
        ) ?? true
        expiresAt = try container.decodeIfPresent(UInt64.self, forKey: .expiresAt)
    }

    private static func resolveAction(
        explicitAction: String?,
        tool: String?,
        title: String?,
        reason: String?
    ) -> String {
        if let explicitAction = explicitAction?.nonEmptyTrimmed {
            return explicitAction
        }

        if let title = title?.nonEmptyTrimmed {
            let stripped = title.replacingOccurrences(
                of: #"^Allow\s+"#,
                with: "",
                options: .regularExpression
            )
            if let stripped = stripped.nonEmptyTrimmed {
                return stripped.prefix(1).lowercased() + stripped.dropFirst()
            }
        }

        if let tool = tool?.nonEmptyTrimmed {
            return tool == "shell" ? "run a shell command" : tool
        }

        if let reason = reason?.nonEmptyTrimmed {
            return reason
        }

        return "perform this action"
    }

    private static func resolvePath(
        explicitPath: String?,
        requestSummary: String?,
        reason: String?
    ) -> String {
        if let explicitPath = explicitPath?.nonEmptyTrimmed {
            return explicitPath
        }

        if let requestSummary = requestSummary?.nonEmptyTrimmed {
            return requestSummary
        }

        if let reason = reason?.nonEmptyTrimmed {
            return reason
        }

        return ""
    }
}

private extension String {
    var nonEmptyTrimmed: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
