import Foundation

enum SidebarSelection: Hashable, RawRepresentable, Sendable {
    case session(String)
    case skills
    case settings

    private static let sessionPrefix = "session:"
    private static let skillsLiteral = "nav:skills"
    private static let settingsLiteral = "nav:settings"

    init?(rawValue: String) {
        if rawValue == Self.skillsLiteral {
            self = .skills
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
        case .settings:
            return Self.settingsLiteral
        }
    }
}
