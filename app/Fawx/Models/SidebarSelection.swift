import Foundation

enum ThreadReference: Hashable, Sendable {
    case threadID(String)
    case activeSessionID(String)

    var threadID: String? {
        guard case .threadID(let threadID) = self else {
            return nil
        }

        return threadID
    }

    var sessionID: String? {
        guard case .activeSessionID(let sessionID) = self else {
            return nil
        }

        return sessionID
    }
}

enum SidebarSelection: Hashable, RawRepresentable, Sendable {
    case workspace(String)
    case thread(ThreadReference)
    case skills
    case fleet
    case experiments
    case git
    case settings

    private static let workspacePrefix = "workspace:"
    private static let threadPrefix = "thread:"
    private static let sessionPrefix = "session:"
    private static let skillsLiteral = "nav:skills"
    private static let fleetLiteral = "nav:fleet"
    private static let experimentsLiteral = "nav:experiments"
    private static let gitLiteral = "nav:git"
    private static let settingsLiteral = "nav:settings"

    init?(rawValue: String) {
        if rawValue == Self.skillsLiteral {
            self = .skills
        } else if rawValue == Self.fleetLiteral {
            self = .fleet
        } else if rawValue == Self.experimentsLiteral {
            self = .experiments
        } else if rawValue == Self.gitLiteral {
            self = .git
        } else if rawValue == Self.settingsLiteral {
            self = .settings
        } else if rawValue.hasPrefix(Self.workspacePrefix) {
            self = .workspace(String(rawValue.dropFirst(Self.workspacePrefix.count)))
        } else if rawValue.hasPrefix(Self.threadPrefix) {
            self = .thread(.threadID(String(rawValue.dropFirst(Self.threadPrefix.count))))
        } else if rawValue.hasPrefix(Self.sessionPrefix) {
            self = .thread(.activeSessionID(String(rawValue.dropFirst(Self.sessionPrefix.count))))
        } else {
            return nil
        }
    }

    var rawValue: String {
        switch self {
        case .workspace(let workspaceID):
            return Self.workspacePrefix + workspaceID
        case .thread(let reference):
            switch reference {
            case .threadID(let threadID):
                return Self.threadPrefix + threadID
            case .activeSessionID(let sessionID):
                return Self.sessionPrefix + sessionID
            }
        case .skills:
            return Self.skillsLiteral
        case .fleet:
            return Self.fleetLiteral
        case .experiments:
            return Self.experimentsLiteral
        case .git:
            return Self.gitLiteral
        case .settings:
            return Self.settingsLiteral
        }
    }

    var isChatSelection: Bool {
        switch self {
        case .workspace, .thread:
            true
        case .skills, .fleet, .experiments, .git, .settings:
            false
        }
    }

    var threadReference: ThreadReference? {
        if case .thread(let reference) = self {
            return reference
        }
        return nil
    }
}
