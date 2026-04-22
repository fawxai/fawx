import Foundation
import Observation

struct GitConfirmationRequest: Identifiable, Equatable {
  enum Action: Equatable {
    case commit(message: String)
    case push
    case pull
  }

  let action: Action

  var id: String {
    switch action {
    case .commit(let message):
      return "commit:\(message)"
    case .push:
      return "push"
    case .pull:
      return "pull"
    }
  }

  var title: String {
    switch action {
    case .commit:
      return "Commit Changes?"
    case .push:
      return "Push Changes?"
    case .pull:
      return "Pull Latest Changes?"
    }
  }

  var message: String {
    switch action {
    case .commit(let message):
      return "Create a commit with this message?\n\n\(message)"
    case .push:
      return "Push the current branch to its remote?"
    case .pull:
      return "Pull the latest changes into the current branch?"
    }
  }

  var confirmButtonTitle: String {
    switch action {
    case .commit:
      return "Commit"
    case .push:
      return "Push"
    case .pull:
      return "Pull"
    }
  }
}

@MainActor
@Observable
final class GitViewModel {
  private enum RefreshTaskIdentifier {
    static let legacy = "legacy"
    static let unboundThreadContext = "bound:none"
  }

  private enum GitScope: Hashable {
    enum DraftKey: Hashable {
      case legacy
      case unboundThreadContext
      case target(String)
    }

    case legacy
    case unboundThreadContext
    case boundRepositoryTarget(refreshIdentity: String)

    var refreshTaskID: String {
      switch self {
      case .legacy:
        return RefreshTaskIdentifier.legacy
      case .unboundThreadContext:
        return RefreshTaskIdentifier.unboundThreadContext
      case .boundRepositoryTarget(let refreshIdentity):
        return refreshIdentity
      }
    }

    var draftKey: DraftKey {
      switch self {
      case .legacy:
        return .legacy
      case .unboundThreadContext:
        return .unboundThreadContext
      case .boundRepositoryTarget(let refreshIdentity):
        return .target(refreshIdentity)
      }
    }
  }

  private struct RefreshState: Equatable {
    let scope: GitScope
    let revision: Int
  }

  private struct LoadedGitState: Sendable {
    let status: GitStatusResponse
    let diff: GitDiffResponse
    let commits: [GitCommitEntry]
  }

  var status: GitStatusResponse?
  var diff: GitDiffResponse?
  var commits: [GitCommitEntry] = []
  var isLoading = false
  var errorMessage: String?
  var selectedFilePath: String?
  var commitMessage = "" {
    didSet {
      persistCommitDraft()
    }
  }
  var isPerformingAction = false
  var lastActionSummary: String?
  var pendingConfirmation: GitConfirmationRequest?
  var threadContext: ThreadContextSnapshot?
  private(set) var repositoryTarget: GitRepositoryTarget?

  private let appState: AppState
  private var isRepositoryTargetBound = false
  // Commit drafts are intentionally ephemeral inspector state.
  // We preserve them while the live Git surface stays open across thread switches,
  // but we do not revive stale commit intents after an app relaunch.
  @ObservationIgnored private var commitDraftsByScope: [GitScope.DraftKey: String] = [:]
  @ObservationIgnored private var refreshRevision = 0
  @ObservationIgnored private var queuedRefreshState: RefreshState?
  @ObservationIgnored private var activeRefreshTask: Task<Void, Never>?
  @ObservationIgnored private var activeRefreshTaskToken: UUID?

  init(appState: AppState) {
    self.appState = appState
  }

  deinit {
    activeRefreshTask?.cancel()
  }

  var refreshTaskID: String {
    gitScope.refreshTaskID
  }

  // Kept as `branchTitle` for compatibility even though the fallback can surface
  // a worktree label or the generic Git inspector title when branch metadata is absent.
  var branchTitle: String {
    status?.branch
      ?? threadContext?.branchName
      ?? threadContext?.worktreeLabel
      ?? repositoryTarget?.branchName
      ?? repositoryTarget?.title
      ?? "Git"
  }

  var contextLine: String? {
    threadContext?.contextLine(includeWorkspace: true) ?? repositoryTarget?.subtitle
  }

  func bindThreadContext(_ context: ThreadContextSnapshot?) {
    bindRepositoryTarget(context.map(Self.threadTarget(for:)), threadContext: context)
  }

  func bindRepositoryTarget(_ target: GitRepositoryTarget?) {
    bindRepositoryTarget(target, threadContext: nil)
  }

  private func bindRepositoryTarget(
    _ target: GitRepositoryTarget?,
    threadContext: ThreadContextSnapshot?
  ) {
    isRepositoryTargetBound = true

    guard repositoryTarget != target || self.threadContext != threadContext else {
      return
    }

    persistCommitDraft()
    refreshRevision += 1
    repositoryTarget = target
    self.threadContext = threadContext
    clearScopedRepositoryState()
    restoreCommitDraft()
  }

  var stagedFiles: [GitFileEntry] {
    (status?.files ?? []).filter(\.staged).sorted {
      $0.path.localizedCaseInsensitiveCompare($1.path) == .orderedAscending
    }
  }

  var unstagedFiles: [GitFileEntry] {
    (status?.files ?? []).filter { !$0.staged }.sorted {
      $0.path.localizedCaseInsensitiveCompare($1.path) == .orderedAscending
    }
  }

  var canCommit: Bool {
    !commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !stagedFiles.isEmpty
  }

  var displayedDiff: String {
    guard let diff else {
      return ""
    }

    guard let selectedFilePath else {
      return diff.diff
    }

    return diffBlock(for: selectedFilePath, in: diff.diff) ?? diff.diff
  }

  // Concurrent callers coalesce onto one in-flight load and only return after the
  // newest visible thread scope has finished refreshing.
  func refresh() async {
    let requestedState = currentRefreshState

    if let activeRefreshTask {
      if activeRefreshTask.isCancelled {
        self.activeRefreshTask = nil
        activeRefreshTaskToken = nil
      } else {
        queuedRefreshState = requestedState
        await activeRefreshTask.value
        return
      }
    }

    isLoading = true
    let taskToken = UUID()
    activeRefreshTaskToken = taskToken

    let refreshTask = Task { @MainActor [weak self] in
      defer {
        self?.finishRefreshTask(token: taskToken)
      }

      while !Task.isCancelled {
        guard let self else {
          return
        }

        let shouldContinueRefreshing = await self.runRefreshPass()
        guard shouldContinueRefreshing else {
          return
        }
      }
    }
    activeRefreshTask = refreshTask
    await refreshTask.value
  }

  func cancelRefresh() {
    activeRefreshTask?.cancel()
  }

  private var gitScope: GitScope {
    guard isRepositoryTargetBound else {
      return .legacy
    }

    guard let repositoryTarget else {
      return .unboundThreadContext
    }

    return .boundRepositoryTarget(refreshIdentity: repositoryTarget.refreshIdentity)
  }

  private var currentRefreshState: RefreshState {
    RefreshState(scope: gitScope, revision: refreshRevision)
  }

  private func persistCommitDraft() {
    let scope = gitScope.draftKey
    guard commitMessage.isEmpty == false else {
      commitDraftsByScope.removeValue(forKey: scope)
      return
    }

    commitDraftsByScope[scope] = commitMessage
  }

  private func restoreCommitDraft() {
    commitMessage = commitDraftsByScope[gitScope.draftKey] ?? ""
  }

  private func clearScopedRepositoryState() {
    // Commit drafts are managed separately so thread switches can restore the
    // in-progress message that belongs to the newly active thread context.
    status = nil
    diff = nil
    commits = []
    errorMessage = nil
    selectedFilePath = nil
    lastActionSummary = nil
    pendingConfirmation = nil
  }

  private func finishRefreshTask(token: UUID) {
    guard activeRefreshTaskToken == token else {
      return
    }

    isLoading = false
    queuedRefreshState = nil
    activeRefreshTask = nil
    activeRefreshTaskToken = nil
  }

  private func runRefreshPass() async -> Bool {
    guard !Task.isCancelled else {
      return false
    }

    let refreshState = currentRefreshState
    queuedRefreshState = nil

    guard appState.isConfigured else {
      clearScopedRepositoryState()
      return shouldContinueRefreshing(after: refreshState)
    }

    if isRepositoryTargetBound {
      guard let repositoryTarget,
        repositoryTarget.workspaceID != nil || repositoryTarget.sessionID != nil
      else {
        clearScopedRepositoryState()
        return shouldContinueRefreshing(after: refreshState)
      }
    }

    let result: Result<LoadedGitState, Error>
    do {
      result = .success(try await loadGitState())
    } catch {
      result = .failure(error)
    }

    guard !Task.isCancelled else {
      return false
    }

    let isStaleRefresh = refreshState != currentRefreshState
    if !isStaleRefresh {
      await applyRefreshResult(result)
    }

    return isStaleRefresh || shouldContinueRefreshing(after: refreshState)
  }

  private func shouldContinueRefreshing(after refreshState: RefreshState) -> Bool {
    guard let queuedRefreshState else {
      return false
    }

    return queuedRefreshState != refreshState
  }

  private func loadGitState() async throws -> LoadedGitState {
    let target = repositoryTarget

    async let statusTask = appState.client.gitStatus(target: target)
    async let diffTask = appState.client.gitDiff(target: target)
    async let logTask = appState.client.gitLog(limit: 10, target: target)

    let (statusResponse, diffResponse, logResponse) = try await (statusTask, diffTask, logTask)
    return LoadedGitState(
      status: statusResponse,
      diff: diffResponse,
      commits: logResponse.commits
    )
  }

  private func applyRefreshResult(_ result: Result<LoadedGitState, Error>) async {
    switch result {
    case .success(let loadedState):
      status = loadedState.status
      diff = loadedState.diff
      commits = loadedState.commits
      if let selectedFilePath, !(loadedState.status.files.contains { $0.path == selectedFilePath })
      {
        self.selectedFilePath = nil
      }
      errorMessage = nil
    case .failure(let error):
      if status == nil {
        diff = nil
        commits = []
        selectedFilePath = nil
      }
      errorMessage = error.localizedDescription
      await appState.noteRecoverableRequestFailure(error)
    }
  }

  func selectFile(_ file: GitFileEntry) {
    selectedFilePath = file.path
  }

  func toggleStage(for file: GitFileEntry) async {
    guard !isPerformingAction else {
      return
    }

    isPerformingAction = true
    defer { isPerformingAction = false }

    do {
      let target = repositoryTarget

      if file.staged {
        _ = try await appState.client.gitUnstage(paths: [file.path], target: target)
        appState.showToast(message: "Unstaged \(file.path).", style: .info)
      } else {
        _ = try await appState.client.gitStage(paths: [file.path], target: target)
        appState.showToast(message: "Staged \(file.path).", style: .success)
      }
      lastActionSummary = nil
      await refresh()
    } catch {
      appState.showToast(message: error.localizedDescription, style: .error)
      await appState.noteRecoverableRequestFailure(error)
    }
  }

  func stageAll() async {
    await runMutation(
      successMessage: "Staged all changes.",
      action: { [target = repositoryTarget] in
        try await appState.client.gitStageAll(target: target)
      }
    )
  }

  func unstageAll() async {
    await runMutation(
      successMessage: "Unstaged all changes.",
      action: { [target = repositoryTarget] in
        try await appState.client.gitUnstageAll(target: target)
      }
    )
  }

  func requestCommitConfirmation() {
    let trimmedMessage = commitMessage.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmedMessage.isEmpty else {
      return
    }
    guard !stagedFiles.isEmpty, !isPerformingAction else {
      return
    }
    pendingConfirmation = GitConfirmationRequest(action: .commit(message: trimmedMessage))
  }

  func requestPushConfirmation() {
    guard !isPerformingAction else {
      return
    }
    pendingConfirmation = GitConfirmationRequest(action: .push)
  }

  func requestPullConfirmation() {
    guard !isPerformingAction else {
      return
    }
    pendingConfirmation = GitConfirmationRequest(action: .pull)
  }

  func cancelPendingConfirmation() {
    pendingConfirmation = nil
  }

  func confirmPendingConfirmation() async {
    guard let pendingConfirmation else {
      return
    }

    self.pendingConfirmation = nil

    switch pendingConfirmation.action {
    case .commit(let message):
      await performCommit(message: message)
    case .push:
      await performPush()
    case .pull:
      await performPull()
    }
  }

  private func performCommit(message: String) async {
    await runMutation(
      successMessage: "Committed changes.",
      action: { [target = repositoryTarget] in
        try await appState.client.gitCommit(message: message, target: target)
      },
      onSuccess: { _ in
        self.commitMessage = ""
      }
    )
  }

  private func performPush() async {
    await runMutation(
      successMessage: nil,
      action: { [target = repositoryTarget] in
        try await appState.client.gitPush(target: target)
      },
      onSuccess: { response in
        self.lastActionSummary = "Pushed \(response.branch) to \(response.remote)."
        self.appState.showToast(
          message: self.lastActionSummary ?? "Pushed changes.", style: .success)
      }
    )
  }

  private func performPull() async {
    await runMutation(
      successMessage: nil,
      action: { [target = repositoryTarget] in
        try await appState.client.gitPull(target: target)
      },
      onSuccess: { response in
        self.lastActionSummary = response.summary
        self.appState.showToast(
          message: response.conflicts
            ? "Pull completed with conflicts."
            : (response.summary.isEmpty ? "Pulled latest changes." : response.summary),
          style: response.conflicts ? .warning : .success
        )
      }
    )
  }

  func fetch() async {
    await runMutation(
      successMessage: nil,
      action: { [target = repositoryTarget] in
        try await appState.client.gitFetch(target: target)
      },
      onSuccess: { response in
        self.lastActionSummary = response.summary
        self.appState.showToast(message: response.summary, style: .info)
      }
    )
  }

  private func runMutation<Response>(
    successMessage: String?,
    action: () async throws -> Response,
    onSuccess: ((Response) -> Void)? = nil
  ) async {
    guard !isPerformingAction else {
      return
    }

    isPerformingAction = true
    defer { isPerformingAction = false }

    do {
      let response = try await action()
      onSuccess?(response)
      if let successMessage {
        appState.showToast(message: successMessage, style: .success)
      }
      await refresh()
    } catch {
      appState.showToast(message: error.localizedDescription, style: .error)
      await appState.noteRecoverableRequestFailure(error)
    }
  }

  private func diffBlock(for path: String, in rawDiff: String) -> String? {
    var blocks: [String] = []
    var currentLines: [String] = []

    for line in rawDiff.split(separator: "\n", omittingEmptySubsequences: false).map(String.init) {
      if line.hasPrefix("diff --git "), !currentLines.isEmpty {
        blocks.append(currentLines.joined(separator: "\n"))
        currentLines = [line]
      } else {
        currentLines.append(line)
      }
    }

    if !currentLines.isEmpty {
      blocks.append(currentLines.joined(separator: "\n"))
    }

    return blocks.first { block in
      guard
        let header = block.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
          .first
      else {
        return false
      }

      let headerLine = String(header)
      if headerLine == "diff --git a/\(path) b/\(path)" {
        return true
      }

      return block.contains("\n--- a/\(path)\n") || block.contains("\n+++ b/\(path)\n")
    }
  }

  private static func threadTarget(for context: ThreadContextSnapshot) -> GitRepositoryTarget {
    GitRepositoryTarget(
      kind: .thread,
      id: "thread:\(context.threadID)",
      title: context.displayTitle,
      subtitle: context.contextLine(includeWorkspace: true),
      sessionID: context.sessionID,
      workspaceID: nil,
      workspacePath: nil,
      worktreeID: nil,
      branchName: context.branchName ?? context.worktreeLabel
    )
  }
}
