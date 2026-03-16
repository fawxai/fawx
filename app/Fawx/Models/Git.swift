import Foundation

struct GitStatusResponse: Codable, Sendable, Hashable {
    let branch: String
    let files: [GitFileEntry]
    let clean: Bool
}

struct GitFileEntry: Codable, Sendable, Hashable, Identifiable {
    let path: String
    let status: GitFileState
    let staged: Bool

    var id: String { path }
}

enum GitFileState: String, Codable, Sendable, Hashable {
    case modified
    case added
    case deleted
    case untracked
    case renamed

    var shortLabel: String {
        switch self {
        case .modified:
            "M"
        case .added:
            "A"
        case .deleted:
            "D"
        case .untracked:
            "?"
        case .renamed:
            "R"
        }
    }
}

struct GitLogResponse: Codable, Sendable, Hashable {
    let commits: [GitCommitEntry]
}

struct GitCommitEntry: Codable, Sendable, Hashable, Identifiable {
    let hash: String
    let shortHash: String
    let message: String
    let author: String
    let timestamp: String

    enum CodingKeys: String, CodingKey {
        case hash
        case shortHash = "short_hash"
        case message
        case author
        case timestamp
    }

    var id: String { hash }
}

struct GitDiffResponse: Codable, Sendable, Hashable {
    let diff: String
    let filesChanged: Int
    let insertions: Int
    let deletions: Int

    enum CodingKeys: String, CodingKey {
        case diff
        case filesChanged = "files_changed"
        case insertions
        case deletions
    }
}

struct GitStageResponse: Codable, Sendable, Hashable {
    let staged: Bool
    let paths: [String]
}

struct GitUnstageResponse: Codable, Sendable, Hashable {
    let unstaged: Bool
    let paths: [String]
}

struct GitCommitResponse: Codable, Sendable, Hashable {
    let committed: Bool
    let hash: String
    let message: String
}

struct GitPushResponse: Codable, Sendable, Hashable {
    let pushed: Bool
    let remote: String
    let branch: String
}

struct GitPullResponse: Codable, Sendable, Hashable {
    let pulled: Bool
    let summary: String
    let conflicts: Bool
}

struct GitFetchResponse: Codable, Sendable, Hashable {
    let fetched: Bool
    let summary: String
}
