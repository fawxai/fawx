import Foundation

enum SidebarSelection: Hashable, RawRepresentable, Sendable {
    case session(String)
    case skills
    case fleet
    case experiments
    case git
    case settings

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
        } else if rawValue.hasPrefix(Self.sessionPrefix) {
            self = .session(String(rawValue.dropFirst(Self.sessionPrefix.count)))
        } else {
            return nil
        }
    }

    var rawValue: String {
        switch self {
        case .session(let sessionID):
            return Self.sessionPrefix + sessionID
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
}
