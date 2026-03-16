import Foundation

struct PermissionEntry: Codable, Sendable, Hashable, Identifiable {
    let action: String
    let level: String
    let title: String

    var id: String { action }
}

struct PermissionsResponse: Codable, Sendable, Hashable {
    let preset: String
    let permissions: [PermissionEntry]
    let availablePresets: [String]

    enum CodingKeys: String, CodingKey {
        case preset
        case permissions
        case availablePresets = "available_presets"
    }
}

struct PermissionsPatchRequest: Encodable, Sendable {
    let preset: String?
    let changes: [PermissionChange]?
}

struct PermissionChange: Codable, Sendable, Hashable {
    let action: String
    let level: String
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
