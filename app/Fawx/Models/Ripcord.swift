import Foundation

struct RipcordStatusResponse: Codable, Sendable, Hashable {
    let active: Bool
    let tripwireId: String?
    let tripwireDescription: String?
    let activatedAt: String?
    let entryCount: Int

    enum CodingKeys: String, CodingKey {
        case active
        case tripwireId = "tripwire_id"
        case tripwireDescription = "tripwire_description"
        case activatedAt = "activated_at"
        case entryCount = "entry_count"
    }

    static let inactive = RipcordStatusResponse(
        active: false,
        tripwireId: nil,
        tripwireDescription: nil,
        activatedAt: nil,
        entryCount: 0
    )

    var displayDescription: String {
        let trimmed = tripwireDescription?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmed.isEmpty ? "Tripwire crossed" : trimmed
    }

    var entryCountLabel: String {
        "\(entryCount) action\(entryCount == 1 ? "" : "s") journaled"
    }

    var activatedDate: Date? {
        activatedAt.flatMap(parseRipcordTimestamp)
    }
}

struct RipcordJournalResponse: Codable, Sendable, Hashable {
    let entries: [JournalEntry]
}

struct JournalEntry: Codable, Sendable, Hashable, Identifiable {
    let id: Int
    let timestamp: String
    let toolName: String
    let toolCallId: String
    let action: JournalAction
    let reversible: Bool

    enum CodingKeys: String, CodingKey {
        case id, timestamp, action, reversible
        case toolName = "tool_name"
        case toolCallId = "tool_call_id"
    }

    var timestampDate: Date? {
        parseRipcordTimestamp(timestamp)
    }

    var displayTime: String {
        guard let timestampDate else {
            return timestamp
        }
        return makeRipcordTimeFormatter().string(from: timestampDate)
    }

    var actionSummary: String? {
        action.summary
    }

    var actionContext: String? {
        action.context
    }

    var metadataLabels: [String] {
        var labels = [displayTime]
        labels.append(reversible ? "Reversible" : "Audit only")
        if let sizeLabel = action.sizeLabel {
            labels.append(sizeLabel)
        }
        if let extraLabel = action.metadataLabel {
            labels.append(extraLabel)
        }
        return labels
    }
}

struct JournalAction: Codable, Sendable, Hashable {
    let type: String
    let payload: JSONValue

    init(from decoder: Decoder) throws {
        let payload = try JSONValue(from: decoder)
        guard let object = payload.objectValue else {
            throw DecodingError.dataCorrupted(
                DecodingError.Context(
                    codingPath: decoder.codingPath,
                    debugDescription: "Expected journal action object."
                )
            )
        }

        self.type = object["type"]?.stringValue ?? "unknown"
        self.payload = payload
    }

    func encode(to encoder: Encoder) throws {
        try payload.encode(to: encoder)
    }

    var summary: String? {
        switch normalizedType {
        case "file_write", "file_delete":
            return compactPath(stringValue(for: "path"))
        case "file_move":
            return compactPath(stringValue(for: "to"))
        case "git_commit":
            return stringValue(for: "commit_sha")
        case "git_branch_create":
            return stringValue(for: "branch")
        case "git_push":
            guard
                let remote = stringValue(for: "remote"),
                let branch = stringValue(for: "branch")
            else {
                return nil
            }
            return "\(remote)/\(branch)"
        case "shell_command":
            return stringValue(for: "command")
        case "network_request":
            guard
                let method = stringValue(for: "method"),
                let url = stringValue(for: "url")
            else {
                return compactURL(stringValue(for: "url"))
            }
            return "\(method.uppercased()) \(compactURL(url))"
        default:
            return nil
        }
    }

    var context: String? {
        switch normalizedType {
        case "file_write":
            if boolValue(for: "created") == true {
                return "Created file"
            }
            return snapshotState
        case "file_delete":
            return "Deleted file"
        case "file_move":
            guard
                let from = compactPath(stringValue(for: "from")),
                let to = compactPath(stringValue(for: "to"))
            else {
                return nil
            }
            return "\(from) -> \(to)"
        case "git_commit":
            if let preRef = stringValue(for: "pre_ref") {
                return "Previous ref: \(preRef)"
            }
            return nil
        case "git_branch_create":
            return compactPath(stringValue(for: "repo"))
        case "git_push":
            if let preRef = stringValue(for: "pre_ref") {
                return "Previous ref: \(preRef)"
            }
            return compactPath(stringValue(for: "repo"))
        case "shell_command":
            if let exitCode = intValue(for: "exit_code") {
                return "Exit code \(exitCode)"
            }
            return nil
        case "network_request":
            if let statusCode = intValue(for: "status_code") {
                return "Status \(statusCode)"
            }
            return nil
        default:
            return nil
        }
    }

    var sizeLabel: String? {
        guard normalizedType == "file_write", let sizeBytes = intValue(for: "size_bytes") else {
            return nil
        }
        return ByteCountFormatter.string(fromByteCount: Int64(sizeBytes), countStyle: .file)
    }

    var metadataLabel: String? {
        switch normalizedType {
        case "network_request":
            return stringValue(for: "method")?.uppercased()
        default:
            return nil
        }
    }

    private var normalizedType: String {
        type.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    private var snapshotState: String? {
        if let snapshotHash = stringValue(for: "snapshot_hash"), !snapshotHash.isEmpty {
            return "Snapshot \(snapshotHash)"
        }
        return "No snapshot"
    }

    private func stringValue(for key: String) -> String? {
        payload.value(at: [key])?.stringValue
    }

    private func intValue(for key: String) -> Int? {
        guard case .number(let value)? = payload.value(at: [key]), value.isFinite else {
            return nil
        }
        return Int(value.rounded(.towardZero))
    }

    private func boolValue(for key: String) -> Bool? {
        guard case .bool(let value)? = payload.value(at: [key]) else {
            return nil
        }
        return value
    }
}

struct RipcordReport: Codable, Sendable, Hashable {
    let reverted: [RevertedEntry]
    let skipped: [SkippedEntry]
    let total: Int
}

struct RevertedEntry: Codable, Sendable, Hashable, Identifiable {
    let id: Int
    let toolName: String
    let description: String

    enum CodingKeys: String, CodingKey {
        case id, description
        case toolName = "tool_name"
    }
}

struct SkippedEntry: Codable, Sendable, Hashable, Identifiable {
    let id: Int
    let toolName: String
    let reason: String

    enum CodingKeys: String, CodingKey {
        case id, reason
        case toolName = "tool_name"
    }
}

struct RipcordApproveResponse: Codable, Sendable, Hashable {
    let cleared: Bool
}

private func makeRipcordTimeFormatter() -> DateFormatter {
    let formatter = DateFormatter()
    formatter.timeStyle = .short
    formatter.dateStyle = .none
    return formatter
}

private func makeRipcordTimestampFormatter(fractionalSeconds: Bool) -> ISO8601DateFormatter {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = fractionalSeconds
        ? [.withInternetDateTime, .withFractionalSeconds]
        : [.withInternetDateTime]
    return formatter
}

private func parseRipcordTimestamp(_ timestamp: String) -> Date? {
    makeRipcordTimestampFormatter(fractionalSeconds: true).date(from: timestamp)
        ?? makeRipcordTimestampFormatter(fractionalSeconds: false).date(from: timestamp)
}

private func compactPath(_ path: String?) -> String? {
    guard let path else {
        return nil
    }

    let trimmed = path.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        return nil
    }

    if trimmed.hasPrefix("/") {
        let components = URL(fileURLWithPath: trimmed).pathComponents.filter { $0 != "/" }
        if components.count >= 3 {
            return components.suffix(3).joined(separator: "/")
        }
        return components.joined(separator: "/")
    }

    return trimmed
}

private func compactURL(_ rawURL: String?) -> String {
    guard let rawURL else {
        return "Network request"
    }

    let trimmed = rawURL.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        return "Network request"
    }

    guard let url = URL(string: trimmed) else {
        return trimmed
    }

    let host = url.host ?? trimmed
    let path = url.path == "/" ? "" : url.path
    return host + path
}
