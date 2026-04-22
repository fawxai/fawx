import Foundation

enum ThreadSidebarOrganizationMode: String, CaseIterable, Sendable, Hashable {
  case byProject = "by_project"
  case chronologicalList = "chronological_list"
}

enum ThreadSidebarSortMode: String, CaseIterable, Sendable, Hashable {
  case created
  case updated
}

struct WorkspaceThreadGroup: Identifiable, Sendable, Equatable {
  let workspace: WorkspaceSummary
  let threads: [ThreadSummary]
  let isExpanded: Bool
  let isActiveContext: Bool
  let showsStartThreadRow: Bool

  var id: String { workspace.id }
}

struct ChronologicalThreadEntry: Identifiable, Sendable, Equatable {
  let workspace: WorkspaceSummary
  let thread: ThreadSummary

  var id: String { thread.id }
}

struct ThreadManagementEntry: Identifiable, Sendable, Equatable {
  let workspace: WorkspaceSummary?
  let thread: ThreadSummary
  let worktree: WorktreeSummary?

  var id: String { thread.id }
}

struct ThreadContextSnapshot: Equatable, Sendable {
  enum Binding: String, Sendable {
    case general
    case workspace
    case worktree
  }

  struct ThreadDetails: Equatable, Sendable {
    let id: String
    let sessionID: String
    let displayTitle: String
    let kind: ThreadKind
    let status: ThreadStatus
    let model: String
    let messageCount: Int
  }

  struct WorkspaceDetails: Equatable, Sendable {
    let name: String?
    let path: String?
    let kind: WorkspaceKind
  }

  struct RepositoryDetails: Equatable, Sendable {
    let branchName: String?
    let worktreeLabel: String?
    let worktreePath: String?
    let baseRef: String?
    let origin: String?
    let isClean: Bool?
    let divergenceLabel: String?
    let worktreeStatusLabel: String?
  }

  let thread: ThreadDetails
  let workspace: WorkspaceDetails
  let repository: RepositoryDetails
  let binding: Binding

  init(
    thread: ThreadDetails,
    workspace: WorkspaceDetails,
    repository: RepositoryDetails,
    binding: Binding
  ) {
    self.thread = thread
    self.workspace = workspace
    self.repository = repository
    self.binding = binding
  }

  init(
    threadID: String,
    sessionID: String,
    displayTitle: String,
    threadKind: ThreadKind,
    threadStatus: ThreadStatus,
    workspaceName: String?,
    workspacePath: String?,
    workspaceKind: WorkspaceKind,
    branchName: String?,
    worktreeLabel: String?,
    worktreePath: String?,
    baseRef: String?,
    repositoryOrigin: String?,
    model: String,
    messageCount: Int,
    binding: Binding,
    isClean: Bool?,
    divergenceLabel: String?,
    worktreeStatusLabel: String?
  ) {
    self.init(
      thread: ThreadDetails(
        id: threadID,
        sessionID: sessionID,
        displayTitle: displayTitle,
        kind: threadKind,
        status: threadStatus,
        model: model,
        messageCount: messageCount
      ),
      workspace: WorkspaceDetails(
        name: workspaceName,
        path: workspacePath,
        kind: workspaceKind
      ),
      repository: RepositoryDetails(
        branchName: branchName,
        worktreeLabel: worktreeLabel,
        worktreePath: worktreePath,
        baseRef: baseRef,
        origin: repositoryOrigin,
        isClean: isClean,
        divergenceLabel: divergenceLabel,
        worktreeStatusLabel: worktreeStatusLabel
      ),
      binding: binding
    )
  }

  var threadID: String { thread.id }
  var sessionID: String { thread.sessionID }
  var displayTitle: String { thread.displayTitle }
  var threadKind: ThreadKind { thread.kind }
  var threadStatus: ThreadStatus { thread.status }
  var workspaceName: String? { workspace.name }
  var workspacePath: String? { workspace.path }
  var workspaceKind: WorkspaceKind { workspace.kind }
  var branchName: String? { repository.branchName }
  var worktreeLabel: String? { repository.worktreeLabel }
  var worktreePath: String? { repository.worktreePath }
  var baseRef: String? { repository.baseRef }
  var repositoryOrigin: String? { repository.origin }
  var model: String { thread.model }
  var messageCount: Int { thread.messageCount }
  var isClean: Bool? { repository.isClean }
  var divergenceLabel: String? { repository.divergenceLabel }
  var worktreeStatusLabel: String? { repository.worktreeStatusLabel }

  var rootPath: String? {
    worktreePath ?? workspacePath
  }

  var hasRepositoryContext: Bool {
    branchName != nil || repositoryOrigin != nil || binding == .worktree
  }

  var showsHeaderIdentity: Bool {
    workspaceKind != .general || worktreeLabel != nil || branchName != nil
  }

  // This key only rebinds Git to a different thread/workspace lane.
  // It intentionally excludes mutable repo-status details so live working-tree
  // churn does not retrigger task-based refresh work.
  var refreshIdentity: String {
    [
      threadID,
      sessionID,
      binding.rawValue,
      workspaceName ?? "",
      workspacePath ?? "",
      branchName ?? "",
      worktreeLabel ?? "",
      worktreePath ?? "",
      baseRef ?? "",
    ].joined(separator: "|")
  }

  // Inspector context uses ownership and root identity only, so the Git header
  // stays stable while live repository state changes underneath the thread.
  func contextLine(includeWorkspace: Bool) -> String? {
    contextLabel(style: .inspector, includeWorkspace: includeWorkspace)
  }

  // Sidebar labels optimize for scanning in the thread list, so they append the
  // compact branch and repository state that the inspector context line omits.
  func sidebarLabel(includeWorkspace: Bool) -> String? {
    contextLabel(style: .sidebar, includeWorkspace: includeWorkspace)
  }

  private enum ContextLabelStyle: Equatable {
    case inspector
    case sidebar

    var includeRootPath: Bool {
      switch self {
      case .inspector:
        return true
      case .sidebar:
        return false
      }
    }

    var excludeBranchDuplicateFromWorktree: Bool {
      switch self {
      case .inspector:
        return true
      case .sidebar:
        return false
      }
    }
  }

  private func contextLabel(style: ContextLabelStyle, includeWorkspace: Bool) -> String? {
    var parts = identityParts(
      includeWorkspace: includeWorkspace,
      includeRootPath: style.includeRootPath,
      excludeBranchDuplicateFromWorktree: style.excludeBranchDuplicateFromWorktree
    )

    if style == .sidebar {
      if let branchLabel = sidebarBranchLabel {
        parts.append(branchLabel)
      }
      if let repositoryStateLabel = compactRepositoryStateLabel {
        parts.append(repositoryStateLabel)
      }
      if let worktreeStatusBadgeLabel = supplementalWorktreeStatusLabel {
        parts.append(worktreeStatusBadgeLabel)
      }
    }

    return parts.nonEmptyJoined(separator: " · ")
  }

  private var sidebarBranchLabel: String? {
    guard let branchName else {
      return nil
    }
    guard branchName != displayTitle, branchName != worktreeLabel else {
      return nil
    }
    return branchName
  }

  private var compactRepositoryStateLabel: String? {
    var parts: [String] = []

    if let isClean {
      parts.append(isClean ? "Clean" : "Dirty")
    }
    if let divergenceLabel, divergenceLabel != "Up to date" {
      parts.append(divergenceLabel)
    }

    return parts.nonEmptyJoined(separator: " ")
  }

  private var supplementalWorktreeStatusLabel: String? {
    guard let worktreeStatusLabel else {
      return nil
    }
    guard worktreeStatusLabel != WorktreeStatus.active.displayLabel,
      worktreeStatusLabel != WorktreeStatus.available.displayLabel
    else {
      return nil
    }

    return worktreeStatusLabel
  }

  private func identityParts(
    includeWorkspace: Bool,
    includeRootPath: Bool,
    excludeBranchDuplicateFromWorktree: Bool
  ) -> [String] {
    var parts: [String] = []

    if includeWorkspace, let workspaceName, workspaceKind != .general {
      parts.append(workspaceName)
    }
    if let worktreeLabel = worktreeIdentityLabel(
      excludeBranchDuplicate: excludeBranchDuplicateFromWorktree
    ) {
      parts.append(worktreeLabel)
    }
    if includeRootPath, let rootPath {
      parts.append(rootPath)
    }

    return parts
  }

  private func worktreeIdentityLabel(excludeBranchDuplicate: Bool) -> String? {
    guard let worktreeLabel else {
      return nil
    }
    guard worktreeLabel != displayTitle, worktreeLabel != workspaceName else {
      return nil
    }
    if excludeBranchDuplicate, worktreeLabel == branchName {
      return nil
    }
    return worktreeLabel
  }
}

@MainActor
extension SessionViewModel {
  // MARK: - Thread Shell Interface

  var sessions: [Session] {
    displaySessions(for: selectedWorkspaceID)
  }

  var selectedWorkspaceID: String? {
    navigationState.selectedWorkspaceID
  }

  var selectedThreadID: String? {
    navigationState.selectedThreadID
  }

  var selectedWorkspace: WorkspaceSummary? {
    selectedWorkspaceID.flatMap(workspace)
  }

  var selectedThread: ThreadSummary? {
    selectedThreadID.flatMap { thread($0) }
  }

  var selectedThreadContextSnapshot: ThreadContextSnapshot? {
    selectedThread.map(threadContextSnapshot(for:))
  }

  var gitRepositoryTargets: [GitRepositoryTarget] {
    workspaces.flatMap { workspace in
      guard workspace.kind == .repository, workspace.path.nonEmpty != nil else {
        return [GitRepositoryTarget]()
      }

      var targets: [GitRepositoryTarget] = [
        workspaceGitTarget(for: workspace)
      ]

      targets.append(
        contentsOf: (worktreesByWorkspaceID[workspace.id] ?? []).map {
          worktreeGitTarget($0, workspace: workspace)
        }
      )

      targets.append(
        contentsOf: sortedThreads(for: workspace.id).map { thread in
          threadGitTarget(for: thread)
        }
      )

      return targets
    }
  }

  var defaultGitRepositoryTarget: GitRepositoryTarget? {
    if let selectedThread {
      return threadGitTarget(for: selectedThread)
    }
    if let selectedWorkspace, selectedWorkspace.kind == .repository {
      return workspaceGitTarget(for: selectedWorkspace)
    }
    return gitRepositoryTargets.first
  }

  var selectedThreadActivitySnapshot: ThreadActivitySnapshot? {
    selectedThread.map(threadActivitySnapshot(for:))
  }

  var backgroundActivityOverviewNotice: BackgroundThreadActivityNotice? {
    makeBackgroundActivityNotice(excluding: nil)
  }

  var selectedBackgroundActivityNotice: BackgroundThreadActivityNotice? {
    guard let selectedThread else {
      return nil
    }

    let selectedActivity = threadActivitySnapshot(for: selectedThread)
    guard selectedActivity.isRunning == false else {
      return nil
    }

    return makeBackgroundActivityNotice(excluding: selectedThread.id)
  }

  var selectedSessionID: String? {
    selectedThread?.activeSessionID ?? navigationState.pendingSessionSelectionID
  }

  // Bridge the thread-first shell back into the existing transcript/runtime path,
  // which still loads chat detail and memory state by active session ID.
  var selectedSession: Session? {
    guard let selectedSessionID else {
      return nil
    }

    if let session = allSessionsByID[selectedSessionID] {
      return session
    }

    if let selectedThread {
      return Session(threadSummary: selectedThread)
    }

    return nil
  }

  var groupedSections: [SessionSection] {
    let calendar = Calendar.current
    let now = Date()
    let groups = Dictionary(grouping: sessions) { session in
      let updatedDate = Date(timeIntervalSince1970: TimeInterval(session.updatedAt))
      if calendar.isDateInToday(updatedDate) {
        return "Today"
      }
      if calendar.isDateInYesterday(updatedDate) {
        return "Yesterday"
      }

      let days =
        calendar.dateComponents(
          [.day],
          from: calendar.startOfDay(for: updatedDate),
          to: calendar.startOfDay(for: now)
        ).day ?? 0
      return days < 7 ? "Previous 7 Days" : "Older"
    }

    return Self.orderedSectionTitles.compactMap { title in
      guard let sessions = groups[title], !sessions.isEmpty else {
        return nil
      }
      return SessionSection(title: title, sessions: sessions)
    }
  }

  var selectedWorktrees: [WorktreeSummary] {
    guard let selectedWorkspaceID else {
      return []
    }

    return worktreesByWorkspaceID[selectedWorkspaceID] ?? []
  }

  var currentChatSelection: SidebarSelection? {
    navigationState.currentChatSelection
  }

  var areAllWorkspacesExpanded: Bool {
    guard workspaces.isEmpty == false else {
      return false
    }

    return Set(workspaces.map(\.id)).isSubset(of: expandedWorkspaceIDs)
  }

  var workspaceThreadGroups: [WorkspaceThreadGroup] {
    workspaceThreadGroups(matching: "")
  }

  var chronologicalThreadEntries: [ChronologicalThreadEntry] {
    chronologicalThreadEntries(matching: "")
  }

  var activeThreadManagementEntries: [ThreadManagementEntry] {
    workspaces.flatMap { workspace in
      sortedThreads(for: workspace.id).map { thread in
        ThreadManagementEntry(
          workspace: workspace,
          thread: thread,
          worktree: worktree(for: thread)
        )
      }
    }
  }

  func isWorkspaceExpanded(_ workspaceID: String) -> Bool {
    expandedWorkspaceIDs.contains(workspaceID)
  }

  // MARK: - Thread Shell Presentation

  func workspaceThreadGroups(matching query: String) -> [WorkspaceThreadGroup] {
    let normalizedSearchQuery = normalizedQuery(query)

    return workspaces.compactMap { workspace in
      let threads = sortedThreads(for: workspace.id)
      let matchingThreads = threads.filter {
        matchesSearch(query: normalizedSearchQuery, thread: $0, workspace: workspace)
      }
      let workspaceMatches = matchesSearch(query: normalizedSearchQuery, workspace: workspace)

      guard normalizedSearchQuery.isEmpty || workspaceMatches || matchingThreads.isEmpty == false
      else {
        return nil
      }

      return WorkspaceThreadGroup(
        workspace: workspace,
        threads: normalizedSearchQuery.isEmpty ? threads : matchingThreads,
        isExpanded: isWorkspaceExpanded(workspace.id),
        isActiveContext: selectedWorkspaceID == workspace.id,
        showsStartThreadRow: normalizedSearchQuery.isEmpty && matchingThreads.isEmpty
      )
    }
  }

  func chronologicalThreadEntries(matching query: String) -> [ChronologicalThreadEntry] {
    let normalizedSearchQuery = normalizedQuery(query)
    let entries = workspaces.flatMap { workspace in
      sortedThreads(for: workspace.id)
        .filter { matchesSearch(query: normalizedSearchQuery, thread: $0, workspace: workspace) }
        .map { ChronologicalThreadEntry(workspace: workspace, thread: $0) }
    }

    return sortChronologicalEntries(entries)
  }

  func workspace(_ workspaceID: String) -> WorkspaceSummary? {
    workspaces.first(where: { $0.id == workspaceID })
  }

  func thread(_ threadID: String) -> ThreadSummary? {
    navigationCatalog.thread(id: threadID)
  }

  func thread(forSessionID sessionID: String) -> ThreadSummary? {
    navigationCatalog.thread(activeSessionID: sessionID)
  }

  func worktree(for thread: ThreadSummary) -> WorktreeSummary? {
    guard let worktreeID = thread.worktreeID else {
      return nil
    }

    return worktreesByWorkspaceID[thread.workspaceID]?.first(where: { $0.id == worktreeID })
  }

  func contextRootPath(for workspaceID: String, worktreeID: String? = nil) -> String? {
    if let worktreeID,
      let worktree = worktreesByWorkspaceID[workspaceID]?.first(where: { $0.id == worktreeID })
    {
      return worktree.path.nonEmpty
    }

    return workspace(workspaceID)?.path.nonEmpty
  }

  func contextRootPath(for thread: ThreadSummary) -> String? {
    contextRootPath(for: thread.workspaceID, worktreeID: thread.worktreeID)
  }

  func contextRootPath(forSessionID sessionID: String) -> String? {
    guard let thread = thread(forSessionID: sessionID) else {
      return nil
    }

    return contextRootPath(for: thread)
  }

  func threadDisplayTitle(_ thread: ThreadSummary) -> String {
    if let customTitle = shellState.customThreadTitle(for: thread.activeSessionID)?.nonEmpty {
      return customTitle
    }

    let title = thread.displayTitle
    guard let worktree = worktree(for: thread) else {
      return title
    }

    if title == "New Thread" || title == thread.activeSessionID || title == worktree.branch {
      return worktree.label.nonEmpty ?? worktree.branch
    }

    return title
  }

  func threadDisplayTitle(for session: Session) -> String {
    shellState.customThreadTitle(for: session.id)?.nonEmpty ?? session.displayTitle
  }

  func threadContextSnapshot(for thread: ThreadSummary) -> ThreadContextSnapshot {
    let workspace = workspace(thread.workspaceID)
    let worktree = worktree(for: thread)

    return ThreadContextSnapshot(
      thread: .init(
        id: thread.id,
        sessionID: thread.activeSessionID,
        displayTitle: threadDisplayTitle(thread),
        kind: thread.kind,
        status: thread.status,
        model: thread.model,
        messageCount: allSessionsByID[thread.activeSessionID]?.messageCount ?? 0
      ),
      workspace: .init(
        name: workspace?.name.nonEmpty,
        path: workspace?.path.nonEmpty,
        kind: workspace?.kind ?? .general
      ),
      repository: .init(
        branchName: repositoryBranchName(workspace: workspace, worktree: worktree),
        worktreeLabel: worktree?.label.nonEmpty,
        worktreePath: worktree?.path.nonEmpty,
        baseRef: worktree?.baseRef?.nonEmpty,
        origin: workspace?.repo?.origin?.nonEmpty,
        isClean: worktree?.clean ?? workspace?.repo?.clean,
        divergenceLabel: threadContextDivergenceLabel(for: worktree),
        worktreeStatusLabel: threadContextWorktreeStatusLabel(for: worktree)
      ),
      binding: threadContextBinding(workspace: workspace, worktree: worktree),
    )
  }

  func threadGitTarget(for thread: ThreadSummary) -> GitRepositoryTarget {
    let context = threadContextSnapshot(for: thread)
    return GitRepositoryTarget(
      kind: .thread,
      id: "thread:\(thread.id)",
      title: context.displayTitle,
      subtitle: context.contextLine(includeWorkspace: true),
      sessionID: thread.activeSessionID,
      workspaceID: nil,
      workspacePath: nil,
      worktreeID: nil,
      branchName: context.branchName ?? context.worktreeLabel
    )
  }

  private func workspaceGitTarget(for workspace: WorkspaceSummary) -> GitRepositoryTarget {
    GitRepositoryTarget(
      kind: .workspace,
      id: "workspace:\(workspace.id)",
      title: workspace.name,
      subtitle: workspace.path.nonEmpty,
      sessionID: nil,
      workspaceID: workspace.id,
      workspacePath: workspace.path.nonEmpty,
      worktreeID: nil,
      branchName: workspace.repo?.currentBranch.nonEmpty
    )
  }

  private func worktreeGitTarget(
    _ worktree: WorktreeSummary,
    workspace: WorkspaceSummary
  ) -> GitRepositoryTarget {
    let title = worktree.label.nonEmpty ?? worktree.branch
    return GitRepositoryTarget(
      kind: .worktree,
      id: "worktree:\(worktree.id)",
      title: title,
      subtitle: [workspace.name.nonEmpty, worktree.path.nonEmpty]
        .compactMap(\.self)
        .joined(separator: " · ")
        .nonEmpty,
      sessionID: nil,
      workspaceID: workspace.id,
      workspacePath: workspace.path.nonEmpty,
      worktreeID: worktree.id,
      branchName: worktree.branch.nonEmpty
    )
  }

  func threadContextLabel(_ thread: ThreadSummary, includeWorkspace: Bool) -> String? {
    threadContextSnapshot(for: thread).sidebarLabel(includeWorkspace: includeWorkspace)
  }

  func hasUnreadActivity(for thread: ThreadSummary) -> Bool {
    guard selectedThreadID != thread.id else {
      return false
    }

    return thread.updatedAt > (viewedThreadUpdateAtByID[thread.id] ?? 0)
  }

  func threadActivitySnapshot(for thread: ThreadSummary) -> ThreadActivitySnapshot {
    ThreadActivitySnapshot(
      threadID: thread.id,
      sessionID: thread.activeSessionID,
      kind: thread.kind,
      status: thread.status,
      runtime: runtimeActivityBySessionID[thread.activeSessionID],
      hasUnreadActivity: hasUnreadActivity(for: thread)
    )
  }

  func activitySnapshot(for session: Session) -> ThreadActivitySnapshot? {
    if let thread = thread(forSessionID: session.id) {
      return threadActivitySnapshot(for: thread)
    }

    guard let runtime = runtimeActivityBySessionID[session.id] else {
      return nil
    }

    return ThreadActivitySnapshot(
      threadID: stableEntityID(prefix: "thread", value: session.id),
      sessionID: session.id,
      kind: fallbackThreadKind(for: session),
      status: ThreadStatus(sessionStatus: session.status),
      runtime: runtime,
      hasUnreadActivity: false
    )
  }

  func worktreeLifecycleSummary(_ worktree: WorktreeSummary) -> String {
    var parts: [String] = []
    parts.append(worktree.clean ? "Clean" : "Dirty")

    if worktree.aheadCount > 0 || worktree.behindCount > 0 {
      parts.append(divergenceLabel(for: worktree))
    }

    if worktree.status != .available {
      parts.append(worktree.status.displayLabel)
    }

    if let baseRef = worktree.baseRef?.nonEmpty {
      parts.append(baseRef)
    }

    return parts.joined(separator: " · ")
  }

  func divergenceLabel(for worktree: WorktreeSummary) -> String {
    switch (worktree.aheadCount, worktree.behindCount) {
    case (0, let behind) where behind > 0:
      return "↓\(behind)"
    case (let ahead, 0) where ahead > 0:
      return "↑\(ahead)"
    case (let ahead, let behind) where ahead > 0 || behind > 0:
      return "↑\(ahead) ↓\(behind)"
    default:
      return "Up to date"
    }
  }

  private func repositoryBranchName(
    workspace: WorkspaceSummary?,
    worktree: WorktreeSummary?
  ) -> String? {
    worktree?.branch.nonEmpty ?? workspace?.repo?.currentBranch.nonEmpty
  }

  private func activeThreads(excluding threadID: String?) -> [ThreadSummary] {
    activeThreadsInDisplayOrder.filter { $0.id != threadID }
  }

  private func makeBackgroundActivityNotice(excluding threadID: String?) -> BackgroundThreadActivityNotice? {
    makeBackgroundActivityNotice(from: activeThreads(excluding: threadID))
  }

  private func makeBackgroundActivityNotice(
    from threads: [ThreadSummary]
  ) -> BackgroundThreadActivityNotice? {
    guard let primaryThread = threads.first else {
      return nil
    }

    let primaryActivity = threadActivitySnapshot(for: primaryThread)
    return BackgroundThreadActivityNotice(
      primaryThreadID: primaryThread.id,
      primaryThreadTitle: threadDisplayTitle(primaryThread),
      primaryBadgeLabel: primaryActivity.badgeLabel,
      activeThreadCount: threads.count,
      subagentThreadCount: threads.filter { $0.kind == .subagent }.count
    )
  }

  private func fallbackThreadKind(for session: Session) -> ThreadKind {
    switch session.kind {
    case .main, .channel:
      return .coding
    case .subagent:
      return .subagent
    case .cron:
      return .automation
    }
  }

  private func threadContextBinding(
    workspace: WorkspaceSummary?,
    worktree: WorktreeSummary?
  ) -> ThreadContextSnapshot.Binding {
    if worktree != nil {
      return .worktree
    }
    if workspace?.isGeneral == false {
      return .workspace
    }
    return .general
  }

  private func threadContextDivergenceLabel(for worktree: WorktreeSummary?) -> String? {
    guard let worktree, worktree.aheadCount > 0 || worktree.behindCount > 0 else {
      return nil
    }

    return divergenceLabel(for: worktree)
  }

  private func threadContextWorktreeStatusLabel(for worktree: WorktreeSummary?) -> String? {
    guard let worktree, worktree.status != .available else {
      return nil
    }

    return worktree.status.displayLabel
  }
}

extension WorktreeStatus {
  fileprivate var displayLabel: String {
    switch self {
    case .active:
      "Active"
    case .available:
      "Available"
    case .detached:
      "Detached"
    }
  }
}

extension Array where Element == String {
  fileprivate func nonEmptyJoined(separator: String) -> String? {
    let filtered = filter { $0.isEmpty == false }
    return filtered.isEmpty ? nil : filtered.joined(separator: separator)
  }
}
