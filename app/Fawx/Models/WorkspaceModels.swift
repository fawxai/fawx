import Foundation

enum WorkspaceKind: String, Codable, CaseIterable, Sendable, Hashable {
  case general
  case repository
}

struct RepositorySummary: Codable, Sendable, Hashable {
  let root: String
  let vcs: String
  let currentBranch: String
  let defaultBranch: String?
  let origin: String?
  let clean: Bool

  enum CodingKeys: String, CodingKey {
    case root
    case vcs
    case currentBranch = "current_branch"
    case defaultBranch = "default_branch"
    case origin
    case clean
  }
}

struct WorkspaceSummary: Codable, Identifiable, Sendable, Hashable {
  let id: String
  let name: String
  let path: String
  let kind: WorkspaceKind
  let repo: RepositorySummary?
  let lastOpenedAt: Int

  enum CodingKeys: String, CodingKey {
    case id
    case name
    case path
    case kind
    case repo
    case lastOpenedAt = "last_opened_at"
  }

  var isGeneral: Bool {
    kind == .general
  }
}

struct WorkspacesResponse: Codable, Sendable, Hashable {
  let workspaces: [WorkspaceSummary]
  let total: Int
}

struct WorkspaceScope: Codable, Sendable, Hashable {
  private let storage: String?

  init() {
    storage = nil
  }

  init(explicitPath: String) {
    storage = explicitPath
  }

  var requestedPath: String? {
    storage
  }

  init(from decoder: Decoder) throws {
    let container = try decoder.singleValueContainer()
    if container.decodeNil() {
      storage = nil
    } else {
      storage = try container.decode(String.self)
    }
  }

  func encode(to encoder: Encoder) throws {
    var container = encoder.singleValueContainer()
    if let requestedPath {
      try container.encode(requestedPath)
    } else {
      try container.encodeNil()
    }
  }
}

enum WorktreeStatus: String, Codable, CaseIterable, Sendable, Hashable {
  case active
  case available
  case detached
}

struct WorktreeSummary: Codable, Identifiable, Sendable, Hashable {
  let id: String
  let workspaceID: String
  let label: String
  let path: String
  let branch: String
  let baseRef: String?
  let status: WorktreeStatus
  let clean: Bool
  let aheadCount: Int
  let behindCount: Int

  enum CodingKeys: String, CodingKey {
    case id
    case workspaceID = "workspace_id"
    case label
    case path
    case branch
    case baseRef = "base_ref"
    case status
    case clean
    case aheadCount = "ahead_count"
    case behindCount = "behind_count"
  }
}

struct WorktreesResponse: Codable, Sendable, Hashable {
  let worktrees: [WorktreeSummary]
  let total: Int
}

struct AttachWorktreeThreadResponse: Codable, Sendable, Hashable {
  let worktreeID: String
  let threadID: String
  let activeSessionID: String

  enum CodingKeys: String, CodingKey {
    case worktreeID = "worktree_id"
    case threadID = "thread_id"
    case activeSessionID = "active_session_id"
  }
}

struct ArchiveWorktreeResponse: Codable, Sendable, Hashable {
  let worktreeID: String
  let archivedThreadCount: Int

  enum CodingKeys: String, CodingKey {
    case worktreeID = "worktree_id"
    case archivedThreadCount = "archived_thread_count"
  }
}

struct DeleteWorktreeResponse: Codable, Sendable, Hashable {
  let deleted: Bool
  let worktreeID: String

  enum CodingKeys: String, CodingKey {
    case deleted
    case worktreeID = "worktree_id"
  }
}

enum ThreadKind: String, Codable, CaseIterable, Sendable, Hashable {
  case general
  case coding
  case automation
  case subagent
}

enum ThreadStatus: String, Codable, CaseIterable, Sendable, Hashable {
  case active
  case idle
  case completed
  case failed
  case paused
}

struct ThreadSummary: Codable, Identifiable, Sendable, Hashable, Comparable {
  let id: String
  var title: String
  let kind: ThreadKind
  let workspaceID: String
  let worktreeID: String?
  let activeSessionID: String
  var status: ThreadStatus
  var preview: String?
  var model: String
  var thinking: ThinkingLevel?
  let createdAt: Int
  var updatedAt: Int

  enum CodingKeys: String, CodingKey {
    case id
    case title
    case kind
    case workspaceID = "workspace_id"
    case worktreeID = "worktree_id"
    case activeSessionID = "active_session_id"
    case status
    case preview
    case model
    case thinking
    case createdAt = "created_at"
    case updatedAt = "updated_at"
  }

  var displayTitle: String {
    title
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .nonEmpty
      ?? "New Thread"
  }

  static func < (lhs: ThreadSummary, rhs: ThreadSummary) -> Bool {
    if lhs.updatedAt == rhs.updatedAt {
      return lhs.id < rhs.id
    }

    return lhs.updatedAt > rhs.updatedAt
  }
}

struct ThreadsResponse: Codable, Sendable, Hashable {
  let threads: [ThreadSummary]
  let total: Int
}

struct ThreadRuntimeActivity: Sendable, Hashable {
  let isStreaming: Bool
  let liveToolCallCount: Int
  let runningToolCallCount: Int
  let completedToolCallCount: Int
  let erroredToolCallCount: Int
  let progressLabel: String?
  let progressMessage: String?
  let startedAt: Date?

  init(
    isStreaming: Bool = false,
    liveToolCallCount: Int = 0,
    runningToolCallCount: Int = 0,
    completedToolCallCount: Int = 0,
    erroredToolCallCount: Int = 0,
    progressLabel: String? = nil,
    progressMessage: String? = nil,
    startedAt: Date? = nil
  ) {
    self.isStreaming = isStreaming
    self.liveToolCallCount = liveToolCallCount
    self.runningToolCallCount = runningToolCallCount
    self.completedToolCallCount = completedToolCallCount
    self.erroredToolCallCount = erroredToolCallCount
    self.progressLabel = progressLabel?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
    self.progressMessage = progressMessage?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
    self.startedAt = startedAt
  }

  var isRunning: Bool {
    isStreaming || runningToolCallCount > 0
  }

  var hasLiveToolActivity: Bool {
    liveToolCallCount > 0
  }

  var badgeLabel: String? {
    if let progressLabel {
      return progressLabel
    }
    if runningToolCallCount > 0 {
      return runningToolCallCount == 1 ? "1 tool" : "\(runningToolCallCount) tools"
    }
    if isStreaming {
      return "Running"
    }
    if liveToolCallCount > 0 {
      return liveToolCallCount == 1 ? "1 tool" : "\(liveToolCallCount) tools"
    }
    return nil
  }

  var summaryLine: String? {
    var parts: [String] = []
    if let progressLabel {
      parts.append(progressLabel)
    } else if isStreaming {
      parts.append("Running")
    }

    if runningToolCallCount > 0 {
      parts.append(
        runningToolCallCount == 1 ? "1 tool running" : "\(runningToolCallCount) tools running")
    } else if liveToolCallCount > 0 {
      parts.append(liveToolCallCount == 1 ? "1 tool used" : "\(liveToolCallCount) tools used")
    }

    return parts.isEmpty ? nil : parts.joined(separator: " · ")
  }
}

struct ThreadActivitySnapshot: Identifiable, Sendable, Hashable {
  let threadID: String
  let sessionID: String
  let kind: ThreadKind
  let status: ThreadStatus
  let runtime: ThreadRuntimeActivity?
  let hasUnreadActivity: Bool

  var id: String { threadID }

  var isRunning: Bool {
    runtime?.isRunning ?? false
  }

  var isStreaming: Bool {
    runtime?.isStreaming ?? false
  }

  var badgeLabel: String? {
    runtime?.badgeLabel
  }

  var summaryLine: String? {
    runtime?.summaryLine
  }

  var progressLabel: String? {
    runtime?.progressLabel
  }

  var progressMessage: String? {
    runtime?.progressMessage
  }

  var startedAt: Date? {
    runtime?.startedAt
  }

  var liveToolCallCount: Int {
    runtime?.liveToolCallCount ?? 0
  }

  var runningToolCallCount: Int {
    runtime?.runningToolCallCount ?? 0
  }

  var completedToolCallCount: Int {
    runtime?.completedToolCallCount ?? 0
  }

  var erroredToolCallCount: Int {
    runtime?.erroredToolCallCount ?? 0
  }

  var showsUnreadIndicator: Bool {
    isRunning == false && hasUnreadActivity
  }
}

struct BackgroundThreadActivityNotice: Sendable, Hashable {
  let primaryThreadID: String
  let primaryThreadTitle: String
  let primaryBadgeLabel: String?
  let activeThreadCount: Int
  let subagentThreadCount: Int

  var message: String {
    if activeThreadCount == 1 {
      return "Running in another thread: \(primaryThreadTitle)"
    }

    return "\(activeThreadCount) other threads running"
  }

  var overviewMessage: String {
    if activeThreadCount == 1 {
      return "\(primaryThreadTitle) is running"
    }

    return "\(activeThreadCount) threads running"
  }

  var detail: String {
    var parts = [primaryThreadTitle]
    if let primaryBadgeLabel {
      parts.append(primaryBadgeLabel)
    }
    if activeThreadCount > 1 {
      parts.append("+\(activeThreadCount - 1) more")
    }
    if subagentThreadCount > 0 {
      parts.append(subagentThreadCount == 1 ? "1 subagent" : "\(subagentThreadCount) subagents")
    }

    return parts.joined(separator: " · ")
  }

  var compactLabel: String {
    activeThreadCount == 1 ? "1 other running" : "\(activeThreadCount) running"
  }
}
