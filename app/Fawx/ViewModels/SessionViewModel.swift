import Foundation
import Observation

struct SessionSection: Identifiable, Sendable {
  let title: String
  let sessions: [Session]

  var id: String { title }
}

private struct WorkspaceSnapshot: Sendable {
  let workspace: WorkspaceSummary
  let threads: [ThreadSummary]
  let worktrees: [WorktreeSummary]
}

struct CreatedWorktreeThread: Sendable {
  let thread: ThreadSummary
  let worktree: WorktreeSummary
}

@MainActor
@Observable
final class SessionViewModel {
  static let orderedSectionTitles = ["Today", "Yesterday", "Previous 7 Days", "Older"]

  private(set) var workspaces: [WorkspaceSummary] = [] {
    didSet { scheduleActiveThreadRosterRebuild() }
  }
  private(set) var threadsByWorkspaceID: [String: [ThreadSummary]] = [:] {
    didSet { scheduleActiveThreadRosterRebuild() }
  }
  private(set) var worktreesByWorkspaceID: [String: [WorktreeSummary]] = [:]
  private(set) var archivedSessions: [Session] = []
  var isLoading = false
  var isLoadingArchivedThreads = false
  var isMutatingSession = false
  var errorMessage: String?
  var organizationMode: ThreadSidebarOrganizationMode = .byProject
  var sortMode: ThreadSidebarSortMode = .updated {
    didSet { scheduleActiveThreadRosterRebuild() }
  }
  private(set) var expandedWorkspaceIDs: Set<String> = []

  private let appState: AppState
  private(set) var navigationState: WorkspaceNavigationState
  @ObservationIgnored
  private(set) var shellState: WorkspaceShellState
  private var allSessions: [Session] = []
  private var hasInitializedExpansionState = false
  private(set) var viewedThreadUpdateAtByID: [String: Int] = [:]
  private(set) var runtimeActivityBySessionID: [String: ThreadRuntimeActivity] = [:] {
    didSet { scheduleActiveThreadRosterRebuild() }
  }
  private var archivedThreadContextBySessionID: [String: ThreadSummary] = [:]
  @ObservationIgnored
  private(set) var activeThreadIDsInDisplayOrder: [String] = []
  @ObservationIgnored
  private(set) var activeThreadsInDisplayOrder: [ThreadSummary] = []
  @ObservationIgnored
  private var activeThreadRosterRebuildDepth = 0
  @ObservationIgnored
  private var activeThreadRosterRebuildPending = false

  init(
    appState: AppState,
    userDefaults: UserDefaults? = nil
  ) {
    let resolvedDefaults = userDefaults ?? WorkspaceNavigationState.defaultNavigationDefaults()
    self.appState = appState
    self.navigationState = WorkspaceNavigationState(userDefaults: resolvedDefaults)
    self.shellState = WorkspaceShellState(userDefaults: resolvedDefaults)
  }

  nonisolated static func filterSessionSections(_ sections: [SessionSection], query: String)
    -> [SessionSection]
  {
    let normalizedQuery =
      query
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .localizedLowercase
    guard normalizedQuery.isEmpty == false else {
      return sections
    }

    return sections.compactMap { section in
      let matchingSessions = section.sessions.filter { session in
        searchFields(for: session).contains { value in
          value.localizedLowercase.contains(normalizedQuery)
        }
      }

      guard matchingSessions.isEmpty == false else {
        return nil
      }

      return SessionSection(title: section.title, sessions: matchingSessions)
    }
  }

  func refresh() async {
    guard appState.isConfigured else {
      performActiveThreadRosterMutation {
        workspaces = []
        threadsByWorkspaceID = [:]
        worktreesByWorkspaceID = [:]
        runtimeActivityBySessionID = [:]
      }
      archivedSessions = []
      allSessions = []
      expandedWorkspaceIDs = []
      hasInitializedExpansionState = false
      viewedThreadUpdateAtByID = [:]
      archivedThreadContextBySessionID = [:]
      navigationState.reset()
      return
    }

    isLoading = true
    defer { isLoading = false }

    do {
      let client = appState.client
      async let sessionsResponse = client.listSessions(limit: 200, archived: .active)
      let workspacesResponse = try await client.listWorkspaces()
      let visibleWorkspaces = shellState.syncVisibleWorkspaces(with: workspacesResponse.workspaces)
      let snapshots = try await loadWorkspaceSnapshots(
        workspaces: visibleWorkspaces,
        client: client
      )
      let activeSessions = try await sessionsResponse.sessions.sorted(by: Session.sidebarSort)
      let rebuiltThreadsByWorkspaceID = rebuildThreads(
        sessions: activeSessions,
        snapshots: snapshots,
        visibleWorkspaces: visibleWorkspaces
      )
      let rebuiltWorktreesByWorkspaceID = rebuildWorktrees(
        snapshots: snapshots,
        visibleWorkspaces: visibleWorkspaces
      )

      performActiveThreadRosterMutation {
        workspaces = visibleWorkspaces
        threadsByWorkspaceID = rebuiltThreadsByWorkspaceID
        worktreesByWorkspaceID = rebuiltWorktreesByWorkspaceID
      }
      shellState.sanitizeManualThreadOrder(using: rebuiltThreadsByWorkspaceID)
      allSessions = activeSessions
      normalizeExpandedWorkspaces()
      restoreSelectionAfterRefresh()
      markSelectedThreadViewed()
      errorMessage = nil
    } catch {
      errorMessage = error.localizedDescription
      await appState.noteRecoverableRequestFailure(error)
    }
  }

  func refreshArchivedSessions() async {
    guard appState.isConfigured else {
      archivedSessions = []
      return
    }

    isLoadingArchivedThreads = true
    defer { isLoadingArchivedThreads = false }

    do {
      archivedSessions = try await appState.client
        .listSessions(limit: 200, archived: .archivedOnly)
        .sessions
        .sorted(by: Session.sidebarSort)
      errorMessage = nil
    } catch {
      errorMessage = error.localizedDescription
      await appState.noteRecoverableRequestFailure(error)
    }
  }

  func loadArchivedThreadsIfNeeded() async {
    guard archivedSessions.isEmpty else {
      return
    }

    await refreshArchivedSessions()
  }

  // MARK: - Thread Shell Mutations

  func selectWorkspace(_ workspaceID: String, ensureExpanded: Bool = true) {
    guard let selection = navigationState.selectWorkspace(workspaceID, in: navigationCatalog) else {
      return
    }

    if ensureExpanded {
      expandedWorkspaceIDs.insert(workspaceID)
    }
    appState.sidebarSelection = selection
    markSelectedThreadViewed()
  }

  func activateWorkspaceRow(_ workspaceID: String) -> Bool {
    guard let workspace = workspace(workspaceID) else {
      return false
    }

    let shouldSwitchContext = selectedWorkspaceID != workspaceID
    if expandedWorkspaceIDs.contains(workspaceID) {
      expandedWorkspaceIDs.remove(workspaceID)
    } else {
      expandedWorkspaceIDs.insert(workspaceID)
    }

    selectWorkspace(workspaceID, ensureExpanded: false)
    return shouldSwitchContext && workspace.path.isEmpty == false
  }

  func toggleWorkspaceExpansion(_ workspaceID: String) {
    guard workspaces.contains(where: { $0.id == workspaceID }) else {
      return
    }

    if expandedWorkspaceIDs.contains(workspaceID) {
      expandedWorkspaceIDs.remove(workspaceID)
    } else {
      expandedWorkspaceIDs.insert(workspaceID)
    }
  }

  func collapseAllWorkspaces() {
    expandedWorkspaceIDs = []
  }

  func expandAllWorkspaces() {
    expandedWorkspaceIDs = Set(workspaces.map(\.id))
  }

  func select(_ sessionID: String?) {
    appState.sidebarSelection = navigationState.selectSession(sessionID, in: navigationCatalog)
    markSelectedThreadViewed()
  }

  func selectThread(id threadID: String) {
    guard let thread = thread(threadID) else {
      return
    }

    shellState.rememberWorkspaceOwner(thread.workspaceID, for: thread.activeSessionID)
    appState.sidebarSelection = navigationState.applySelectedThread(thread, persistSelection: true)
    expandedWorkspaceIDs.insert(thread.workspaceID)
    markThreadViewed(thread)
  }

  func openWorkspace(path: String) async -> WorkspaceSummary? {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let openedWorkspace = try await appState.client.openWorkspace(path: path)
      let workspace = shellState.addWorkspace(openedWorkspace)
      workspaces = shellState.visibleWorkspaces(merging: workspaces + [workspace])
      expandedWorkspaceIDs.insert(workspace.id)
      selectWorkspace(workspace.id)
      errorMessage = nil
      return workspace
    } catch {
      errorMessage = error.localizedDescription
      return nil
    }
  }

  func removeWorkspace(id workspaceID: String) {
    guard let workspace = workspace(workspaceID) else {
      return
    }
    guard workspace.isGeneral == false else {
      return
    }

    let removedThreads = threadsByWorkspaceID[workspaceID] ?? []
    shellState.removeWorkspace(workspace)
    workspaces.removeAll { $0.id == workspaceID }
    threadsByWorkspaceID.removeValue(forKey: workspaceID)
    worktreesByWorkspaceID.removeValue(forKey: workspaceID)
    expandedWorkspaceIDs.remove(workspaceID)

    for thread in removedThreads {
      viewedThreadUpdateAtByID.removeValue(forKey: thread.id)
    }

    navigationState.forgetRememberedThreadSelections(in: [workspaceID])
    restoreSelectionAfterRefresh()
    markSelectedThreadViewed()
  }

  func createNewThread(
    in workspaceID: String?,
    modelID: String? = nil,
    thinkingLevel: ThinkingLevel? = nil
  ) async -> String? {
    guard appState.isConfigured else {
      return nil
    }

    guard
      let resolvedWorkspaceID = resolvedThreadCreationWorkspaceID(preferredWorkspaceID: workspaceID)
    else {
      return nil
    }

    selectWorkspace(resolvedWorkspaceID)

    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let createdThread = try await appState.client.createThread(
        workspaceID: resolvedWorkspaceID,
        workspaceScope: workspaceScope(for: resolvedWorkspaceID),
        model: modelID ?? appState.activeModel?.modelID,
        thinking: thinkingLevel
      )
      materializeCreatedThread(createdThread)
      errorMessage = nil
      return createdThread.activeSessionID
    } catch {
      errorMessage = error.localizedDescription
      return nil
    }
  }

  func createPermanentWorktree(
    in workspaceID: String,
    branch: String,
    baseRef: String?
  ) async -> WorktreeSummary? {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let createdWorktree = try await appState.client.createWorktree(
        workspaceID: workspaceID,
        workspaceScope: workspaceScope(for: workspaceID),
        branch: branch,
        baseRef: baseRef
      )
      upsertWorktree(createdWorktree, in: workspaceID)
      errorMessage = nil
      return createdWorktree
    } catch {
      errorMessage = error.localizedDescription
      return nil
    }
  }

  func createWorktreeThread(
    in workspaceID: String,
    title: String,
    branch: String,
    baseRef: String?,
    modelID: String? = nil
  ) async -> CreatedWorktreeThread? {
    isMutatingSession = true
    defer { isMutatingSession = false }

    var createdWorktree: WorktreeSummary?

    do {
      let worktree = try await appState.client.createWorktree(
        workspaceID: workspaceID,
        workspaceScope: workspaceScope(for: workspaceID),
        branch: branch,
        baseRef: baseRef
      )
      createdWorktree = worktree
      let createdThread = try await appState.client.createThread(
        workspaceID: workspaceID,
        workspaceScope: workspaceScope(for: workspaceID),
        title: title,
        model: modelID ?? appState.activeModel?.modelID,
        thinking: nil,
        worktreeID: worktree.id
      )
      upsertWorktree(worktree, in: workspaceID)
      materializeCreatedThread(createdThread, worktree: worktree)
      errorMessage = nil
      return CreatedWorktreeThread(thread: createdThread, worktree: worktree)
    } catch {
      if let createdWorktree {
        upsertWorktree(createdWorktree, in: workspaceID)
      }
      errorMessage = error.localizedDescription
      return nil
    }
  }

  func archiveWorktree(id worktreeID: String) async -> Int {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let response = try await appState.client.archiveWorktree(
        id: worktreeID,
        workspaceScope: workspaceScope(forWorktreeID: worktreeID)
      )
      await refresh()
      await refreshArchivedSessions()
      errorMessage = nil
      return response.archivedThreadCount
    } catch {
      errorMessage = error.localizedDescription
      return 0
    }
  }

  func deleteWorktree(id worktreeID: String) async -> Bool {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let response = try await appState.client.deleteWorktree(
        id: worktreeID,
        workspaceScope: workspaceScope(forWorktreeID: worktreeID)
      )
      if response.deleted {
        await refresh()
        await refreshArchivedSessions()
      }
      errorMessage = nil
      return response.deleted
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func clearSession(id: String) async -> Bool {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      _ = try await appState.client.clearSession(id: id)
      if let index = allSessions.firstIndex(where: { $0.id == id }) {
        allSessions[index].preview = nil
        allSessions[index].messageCount = 0
        allSessions[index].updatedAt = Int(Date().timeIntervalSince1970)
      }
      updateThread(activeSessionID: id) { thread in
        thread.preview = nil
        thread.updatedAt = Int(Date().timeIntervalSince1970)
      }
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func archiveThread(id threadID: String) async -> Bool {
    guard let thread = thread(threadID) else {
      return false
    }

    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let archivedSession = try await appState.client.archiveSession(id: thread.activeSessionID)
      archivedThreadContextBySessionID[archivedSession.id] = thread
      removeSession(archivedSession.id)
      upsertArchivedSession(archivedSession)
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func archiveThreads(in workspaceID: String) async -> Int {
    let threads = sortedThreads(for: workspaceID)
    guard threads.isEmpty == false else {
      return 0
    }

    isMutatingSession = true
    defer { isMutatingSession = false }

    let client = appState.client
    let results = await withTaskGroup(
      of: (String, Session?, String?).self,
      returning: [(String, Session?, String?)].self
    ) { group in
      for thread in threads {
        group.addTask {
          do {
            let archivedSession = try await client.archiveSession(id: thread.activeSessionID)
            return (thread.id, archivedSession, nil)
          } catch {
            return (thread.id, nil, error.localizedDescription)
          }
        }
      }

      var collectedResults: [(String, Session?, String?)] = []
      for await result in group {
        collectedResults.append(result)
      }
      return collectedResults
    }

    let resultsByThreadID = Dictionary(
      uniqueKeysWithValues: results.map { ($0.0, ($0.1, $0.2)) }
    )
    var archivedCount = 0
    var lastErrorMessage: String?

    for thread in threads {
      let (archivedSession, errorMessage) = resultsByThreadID[thread.id] ?? (nil, nil)
      if let archivedSession {
        archivedThreadContextBySessionID[archivedSession.id] = thread
        removeSession(archivedSession.id)
        upsertArchivedSession(archivedSession)
        archivedCount += 1
      } else if let errorMessage {
        lastErrorMessage = errorMessage
      }
    }

    errorMessage = lastErrorMessage
    return archivedCount
  }

  func unarchiveSession(id: String) async -> Bool {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      let restoredSession = try await appState.client.unarchiveSession(id: id)
      archivedSessions.removeAll { $0.id == id }
      if let threadContext = archivedThreadContextBySessionID.removeValue(forKey: id) {
        upsert(restoredSession)
        restoreArchivedThread(threadContext, with: restoredSession)
        select(restoredSession.id)
      } else {
        upsert(restoredSession)
        await refresh()
      }
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func deleteSession(id: String) async -> Bool {
    isMutatingSession = true
    defer { isMutatingSession = false }

    do {
      _ = try await appState.client.deleteSession(id: id)
      archivedSessions.removeAll { $0.id == id }
      archivedThreadContextBySessionID.removeValue(forKey: id)
      removeSession(id)
      shellState.forgetDeletedSession(id)
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func deleteSessions(ids: [String]) async -> [String] {
    guard ids.isEmpty == false else {
      return []
    }

    isMutatingSession = true
    defer { isMutatingSession = false }

    let client = appState.client
    let deletionResults = await withTaskGroup(
      of: (String, String?).self,
      returning: [(String, String?)].self
    ) { group in
      for id in ids {
        group.addTask {
          do {
            _ = try await client.deleteSession(id: id)
            return (id, nil)
          } catch {
            return (id, error.localizedDescription)
          }
        }
      }

      var results: [(String, String?)] = []
      for await result in group {
        results.append(result)
      }
      return results
    }

    let deletedIDs = Set(
      deletionResults.compactMap { id, errorMessage in
        errorMessage == nil ? id : nil
      }
    )

    for id in ids where deletedIDs.contains(id) {
      archivedSessions.removeAll { $0.id == id }
      archivedThreadContextBySessionID.removeValue(forKey: id)
      removeSession(id)
      shellState.forgetDeletedSession(id)
    }

    errorMessage = deletionResults.compactMap(\.1).last

    return ids.filter { deletedIDs.contains($0) }
  }

  func moveWorkspaces(fromOffsets: IndexSet, toOffset: Int) {
    workspaces = shellState.moveWorkspaces(
      currentWorkspaces: workspaces,
      fromOffsets: fromOffsets,
      toOffset: toOffset
    )
  }

  func moveThreads(
    in workspaceID: String,
    fromOffsets: IndexSet,
    toOffset: Int
  ) {
    let orderedThreadIDs = sortedThreads(for: workspaceID).map(\.id)
    shellState.moveThreads(
      orderedThreadIDs,
      in: workspaceID,
      fromOffsets: fromOffsets,
      toOffset: toOffset
    )
    rebuildActiveThreadIDsInDisplayOrder()
  }

  func renameThread(id threadID: String, title: String) {
    guard let thread = thread(threadID) else {
      return
    }

    let normalizedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
    shellState.renameThread(sessionID: thread.activeSessionID, title: normalizedTitle)

    if let index = allSessions.firstIndex(where: { $0.id == thread.activeSessionID }) {
      allSessions[index].title = normalizedTitle.isEmpty ? nil : normalizedTitle
    }

    updateThread(activeSessionID: thread.activeSessionID) { thread in
      thread.title = normalizedTitle.isEmpty ? "New Thread" : normalizedTitle
    }
  }

  func upsert(_ session: Session) {
    archivedSessions.removeAll { $0.id == session.id }

    if let index = allSessions.firstIndex(where: { $0.id == session.id }) {
      allSessions[index] = session
    } else {
      allSessions.append(session)
    }
    allSessions.sort(by: Session.sidebarSort)

    updateThread(activeSessionID: session.id) { thread in
      if let customTitle = shellState.customThreadTitle(for: session.id)?.nonEmpty {
        thread.title = customTitle
      } else if let title = session.title?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
      {
        thread.title = title
      } else if let label = session.label?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
      {
        thread.title = label
      }
      thread.preview = session.preview
      thread.model = session.model
      thread.thinking = session.thinking
      thread.updatedAt = session.updatedAt
      thread.status = ThreadStatus(sessionStatus: session.status)
    }
  }

  func removeSession(_ sessionID: String) {
    allSessions.removeAll { $0.id == sessionID }
    runtimeActivityBySessionID.removeValue(forKey: sessionID)

    var removedSelectedThread = false
    var removedThreadIDsByWorkspaceID: [String: String] = [:]
    for workspaceID in threadsByWorkspaceID.keys.sorted() {
      guard
        var workspaceThreads = threadsByWorkspaceID[workspaceID],
        let threadIndex = workspaceThreads.firstIndex(where: { $0.activeSessionID == sessionID })
      else {
        continue
      }

      let removedThreadID = workspaceThreads[threadIndex].id
      removedThreadIDsByWorkspaceID[workspaceID] = removedThreadID
      viewedThreadUpdateAtByID.removeValue(forKey: removedThreadID)
      workspaceThreads.remove(at: threadIndex)
      threadsByWorkspaceID[workspaceID] = workspaceThreads

      if selectedThreadID == removedThreadID {
        removedSelectedThread = true
      }
    }

    navigationState.forgetRememberedThreadSelections(matching: removedThreadIDsByWorkspaceID)
    navigationState.clearPendingSessionSelection(matching: sessionID)

    if removedSelectedThread {
      let replacementSelection = navigationState.restoreSelectionAfterRemovingSelectedThread(
        in: navigationCatalog
      )
      if appState.sidebarSelection?.isChatSelection == true {
        appState.sidebarSelection = replacementSelection
      }
      markSelectedThreadViewed()
    }
  }

  func updatePreview(for sessionID: String, text: String, model: String?) {
    guard let index = allSessions.firstIndex(where: { $0.id == sessionID }) else {
      return
    }

    allSessions[index].applyPreview(text, model: model)
    if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false {
      allSessions[index].messageCount += 1
    }
    allSessions.sort(by: Session.sidebarSort)

    updateThread(activeSessionID: sessionID) { thread in
      let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
      if trimmed.isEmpty == false {
        thread.title = summarizedSessionTitle(from: trimmed)
      }
      thread.preview = trimmed.isEmpty ? nil : trimmed
      if let model, !model.isEmpty {
        thread.model = model
      }
      thread.updatedAt = Int(Date().timeIntervalSince1970)
    }
  }

  #if DEBUG
    func setSidebarDataForTesting(
      workspaces: [WorkspaceSummary],
      threadsByWorkspaceID: [String: [ThreadSummary]],
      worktreesByWorkspaceID: [String: [WorktreeSummary]],
      sessions: [Session] = [],
      archivedSessions: [Session] = [],
      selectedWorkspaceID: String? = nil,
      selectedThreadID: String? = nil,
      expandedWorkspaceIDs: Set<String>? = nil,
      viewedThreadUpdateAtByID: [String: Int] = [:],
      runtimeActivityBySessionID: [String: ThreadRuntimeActivity] = [:]
    ) {
      performActiveThreadRosterMutation {
        self.workspaces = workspaces
        self.threadsByWorkspaceID = threadsByWorkspaceID
        self.worktreesByWorkspaceID = worktreesByWorkspaceID
        self.runtimeActivityBySessionID = runtimeActivityBySessionID
      }
      self.allSessions = sessions.sorted(by: Session.sidebarSort)
      self.archivedSessions = archivedSessions.sorted(by: Session.sidebarSort)
      self.expandedWorkspaceIDs = expandedWorkspaceIDs ?? Set(workspaces.map(\.id))
      hasInitializedExpansionState = true
      shellState.resetForTesting()
      _ = shellState.syncVisibleWorkspaces(with: workspaces)
      shellState.seedOwnership(using: threadsByWorkspaceID)
      self.viewedThreadUpdateAtByID = viewedThreadUpdateAtByID

      if let selectedWorkspaceID {
        appState.sidebarSelection = .workspace(selectedWorkspaceID)
      }
      if let selectedThreadID, let thread = thread(selectedThreadID) {
        appState.sidebarSelection = .thread(.threadID(thread.id))
        _ = navigationState.applySelectedThread(thread, persistSelection: false)
        markThreadViewed(thread)
      } else if let selectedWorkspaceID {
        _ = navigationState.selectWorkspace(selectedWorkspaceID, in: navigationCatalog)
      }
    }
  #endif

  var allSessionsByID: [String: Session] {
    Dictionary(uniqueKeysWithValues: allSessions.map { ($0.id, $0) })
  }

  func modelID(for sessionID: String?) -> String? {
    guard let sessionID else {
      return nil
    }

    if let session = allSessionsByID[sessionID] {
      return session.model
    }

    return threadsByWorkspaceID
      .values
      .flatMap { $0 }
      .first(where: { $0.activeSessionID == sessionID })?
      .model
  }

  func thinkingLevel(for sessionID: String?) -> ThinkingLevel? {
    guard let sessionID else {
      return nil
    }

    if let session = allSessionsByID[sessionID] {
      return session.thinking
    }

    return threadsByWorkspaceID
      .values
      .flatMap { $0 }
      .first(where: { $0.activeSessionID == sessionID })?
      .thinking
  }

  func updateModel(for sessionID: String, modelID: String) async -> Bool {
    do {
      let session = try await appState.client.updateSessionModel(id: sessionID, model: modelID)
      upsert(session)
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  func updateThinking(for sessionID: String, level: ThinkingLevel) async -> Bool {
    do {
      let session = try await appState.client.updateSessionThinking(id: sessionID, level: level)
      upsert(session)
      errorMessage = nil
      return true
    } catch {
      errorMessage = error.localizedDescription
      return false
    }
  }

  var navigationCatalog: WorkspaceNavigationCatalog {
    WorkspaceNavigationCatalog(
      workspaces: workspaces,
      threadsByWorkspaceID: threadsByWorkspaceID,
      allSessionsByID: allSessionsByID
    )
  }

  func syncRuntimeActivity(_ runtimeActivityBySessionID: [String: ThreadRuntimeActivity]) {
    let validSessionIDs = Set(
      threadsByWorkspaceID
        .values
        .flatMap { $0.map(\.activeSessionID) }
        + allSessions.map(\.id)
    )
    let sanitized = runtimeActivityBySessionID.filter { validSessionIDs.contains($0.key) }
    guard self.runtimeActivityBySessionID != sanitized else {
      return
    }

    self.runtimeActivityBySessionID = sanitized
  }

  func displaySessions(for workspaceID: String?) -> [Session] {
    guard let workspaceID else {
      return []
    }

    let threadSessions = (threadsByWorkspaceID[workspaceID] ?? []).map { thread in
      allSessionsByID[thread.activeSessionID] ?? Session(threadSummary: thread)
    }

    guard
      let pendingSessionSelectionID = navigationState.pendingSessionSelectionID,
      selectedWorkspaceID == workspaceID,
      selectedThreadID == nil,
      let pendingSession = allSessionsByID[pendingSessionSelectionID],
      threadSessions.contains(where: { $0.id == pendingSession.id }) == false
    else {
      return threadSessions
    }

    return [pendingSession] + threadSessions
  }

  private func loadWorkspaceSnapshots(
    workspaces: [WorkspaceSummary],
    client: FawxClient
  ) async throws -> [WorkspaceSnapshot] {
    try await withThrowingTaskGroup(
      of: (Int, WorkspaceSnapshot).self,
      returning: [WorkspaceSnapshot].self
    ) { group in
      for (index, workspace) in workspaces.enumerated() {
        group.addTask {
          let workspaceScope = workspace.path.nonEmpty.map(WorkspaceScope.init(explicitPath:))
          async let threadsResponse = client.workspaceThreads(
            id: workspace.id,
            workspaceScope: workspaceScope
          )
          async let worktreesResponse = client.workspaceWorktrees(
            id: workspace.id,
            workspaceScope: workspaceScope
          )

          let threadsResponseValue = try await threadsResponse
          let worktreesResponseValue = try await worktreesResponse
          let threads = threadsResponseValue.threads.sorted()
          let worktrees = worktreesResponseValue.worktrees

          return (
            index,
            WorkspaceSnapshot(
              workspace: workspace,
              threads: threads,
              worktrees: worktrees
            )
          )
        }
      }

      var indexedSnapshots: [(Int, WorkspaceSnapshot)] = []
      for try await snapshot in group {
        indexedSnapshots.append(snapshot)
      }

      return
        indexedSnapshots
        .sorted(by: { $0.0 < $1.0 })
        .map(\.1)
    }
  }

  private func rebuildThreads(
    sessions: [Session],
    snapshots: [WorkspaceSnapshot],
    visibleWorkspaces: [WorkspaceSummary]
  ) -> [String: [ThreadSummary]] {
    let visibleWorkspacesByID = Dictionary(
      uniqueKeysWithValues: visibleWorkspaces.map { ($0.id, $0) }
    )
    let orderedVisibleWorkspaceIDs = visibleWorkspaces.map(\.id)
    let serverThreadsBySessionID = threadsByActiveSessionID(snapshots.flatMap(\.threads))
    let cachedThreadsBySessionID = threadsByActiveSessionID(
      threadsByWorkspaceID
        .values
        .flatMap { $0 }
    )
    let currentRepositoryWorkspaceID = snapshots.first(where: { $0.workspace.isGeneral == false })?
      .workspace.id
    var rebuiltThreadsByWorkspaceID = Dictionary(
      uniqueKeysWithValues: visibleWorkspaces.map { ($0.id, [ThreadSummary]()) }
    )
    var materializedSessionIDs: Set<String> = []

    for session in sessions {
      guard
        let workspaceID = resolvedWorkspaceID(
          for: session.id,
          visibleWorkspacesByID: visibleWorkspacesByID,
          orderedVisibleWorkspaceIDs: orderedVisibleWorkspaceIDs,
          currentRepositoryWorkspaceID: currentRepositoryWorkspaceID,
          serverThreadsBySessionID: serverThreadsBySessionID,
          cachedThreadsBySessionID: cachedThreadsBySessionID
        )
      else {
        continue
      }
      guard let workspace = visibleWorkspacesByID[workspaceID] else {
        continue
      }

      shellState.rememberWorkspaceOwner(workspaceID, for: session.id)
      materializedSessionIDs.insert(session.id)

      let thread: ThreadSummary
      if let serverThread = serverThreadsBySessionID[session.id] {
        thread = hydratedThread(
          from: serverThread,
          session: session,
          workspace: workspace
        )
      } else if let cachedThread = cachedThreadsBySessionID[session.id] {
        thread = hydratedThread(
          from: cachedThread,
          session: session,
          workspace: workspace
        )
      } else {
        thread = makeCompatibilityThread(for: session, workspaceID: workspaceID)
      }

      rebuiltThreadsByWorkspaceID[workspaceID, default: []].append(thread)
    }

    for thread in serverThreadsBySessionID.values.sorted(by: threadSort) {
      guard materializedSessionIDs.contains(thread.activeSessionID) == false else {
        continue
      }
      guard
        let workspaceID = resolvedWorkspaceID(
          for: thread.activeSessionID,
          visibleWorkspacesByID: visibleWorkspacesByID,
          orderedVisibleWorkspaceIDs: orderedVisibleWorkspaceIDs,
          currentRepositoryWorkspaceID: currentRepositoryWorkspaceID,
          serverThreadsBySessionID: serverThreadsBySessionID,
          cachedThreadsBySessionID: cachedThreadsBySessionID
        )
      else {
        continue
      }
      guard let workspace = visibleWorkspacesByID[workspaceID] else {
        continue
      }

      shellState.rememberWorkspaceOwner(workspaceID, for: thread.activeSessionID)
      rebuiltThreadsByWorkspaceID[workspaceID, default: []].append(
        hydratedThreadWithoutSession(from: thread, workspace: workspace)
      )
      materializedSessionIDs.insert(thread.activeSessionID)
    }

    for thread in cachedThreadsBySessionID.values.sorted(by: threadSort) {
      guard materializedSessionIDs.contains(thread.activeSessionID) == false else {
        continue
      }
      guard
        let workspaceID = resolvedWorkspaceID(
          for: thread.activeSessionID,
          visibleWorkspacesByID: visibleWorkspacesByID,
          orderedVisibleWorkspaceIDs: orderedVisibleWorkspaceIDs,
          currentRepositoryWorkspaceID: currentRepositoryWorkspaceID,
          serverThreadsBySessionID: serverThreadsBySessionID,
          cachedThreadsBySessionID: cachedThreadsBySessionID
        )
      else {
        continue
      }
      guard let workspace = visibleWorkspacesByID[workspaceID] else {
        continue
      }

      shellState.rememberWorkspaceOwner(workspaceID, for: thread.activeSessionID)
      rebuiltThreadsByWorkspaceID[workspaceID, default: []].append(
        hydratedThreadWithoutSession(from: thread, workspace: workspace)
      )
      materializedSessionIDs.insert(thread.activeSessionID)
    }

    return rebuiltThreadsByWorkspaceID
  }

  private func rebuildWorktrees(
    snapshots: [WorkspaceSnapshot],
    visibleWorkspaces: [WorkspaceSummary]
  ) -> [String: [WorktreeSummary]] {
    var rebuiltWorktreesByWorkspaceID = Dictionary(
      uniqueKeysWithValues: visibleWorkspaces.map { workspace in
        (workspace.id, worktreesByWorkspaceID[workspace.id] ?? [])
      }
    )

    for snapshot in snapshots
    where visibleWorkspaces.contains(where: { $0.id == snapshot.workspace.id }) {
      rebuiltWorktreesByWorkspaceID[snapshot.workspace.id] = snapshot.worktrees
    }

    return rebuiltWorktreesByWorkspaceID
  }

  private func restoreSelectionAfterRefresh() {
    let resolution = navigationState.restoreSelectionAfterRefresh(
      sidebarSelection: appState.sidebarSelection,
      in: navigationCatalog
    )

    if resolution.shouldRewriteSidebarSelection {
      appState.sidebarSelection = resolution.rewrittenSidebarSelection
    }
  }

  private func normalizeExpandedWorkspaces() {
    let validWorkspaceIDs = Set(workspaces.map(\.id))
    expandedWorkspaceIDs = expandedWorkspaceIDs.intersection(validWorkspaceIDs)

    guard hasInitializedExpansionState == false else {
      return
    }

    expandedWorkspaceIDs = validWorkspaceIDs
    hasInitializedExpansionState = true
  }

  func sortChronologicalEntries(
    _ entries: [ChronologicalThreadEntry]
  ) -> [ChronologicalThreadEntry] {
    entries.sorted { lhs, rhs in
      threadSort(lhs.thread, rhs.thread)
    }
  }

  func sortedThreads(for workspaceID: String) -> [ThreadSummary] {
    shellState.orderedThreads(
      threadsByWorkspaceID[workspaceID] ?? [],
      in: workspaceID,
      fallbackSort: threadSort
    )
  }

  private func rebuildActiveThreadIDsInDisplayOrder() {
    let activeSessionIDs = Set(
      runtimeActivityBySessionID.compactMap { sessionID, runtime in
        runtime.isRunning ? sessionID : nil
      }
    )

    guard activeSessionIDs.isEmpty == false else {
      activeThreadsInDisplayOrder = []
      activeThreadIDsInDisplayOrder = []
      return
    }

    let activeThreads = workspaces.flatMap { workspace in
      sortedThreads(for: workspace.id)
        .filter { activeSessionIDs.contains($0.activeSessionID) }
    }
    activeThreadsInDisplayOrder = activeThreads
    activeThreadIDsInDisplayOrder = activeThreads.map(\.id)
  }

  private func scheduleActiveThreadRosterRebuild() {
    guard activeThreadRosterRebuildDepth == 0 else {
      activeThreadRosterRebuildPending = true
      return
    }

    rebuildActiveThreadIDsInDisplayOrder()
  }

  private func performActiveThreadRosterMutation(_ updates: () -> Void) {
    // Keep this helper synchronous-only: the depth guard is safe for batched writes,
    // but would not survive an `await` inside the mutation closure.
    activeThreadRosterRebuildDepth += 1
    updates()
    activeThreadRosterRebuildDepth -= 1

    guard activeThreadRosterRebuildDepth == 0, activeThreadRosterRebuildPending else {
      return
    }

    activeThreadRosterRebuildPending = false
    rebuildActiveThreadIDsInDisplayOrder()
  }

  private func threadSort(_ lhs: ThreadSummary, _ rhs: ThreadSummary) -> Bool {
    switch sortMode {
    case .created:
      if lhs.createdAt == rhs.createdAt {
        return lhs.id < rhs.id
      }
      return lhs.createdAt > rhs.createdAt
    case .updated:
      return lhs < rhs
    }
  }

  private func threadsByActiveSessionID(_ threads: [ThreadSummary]) -> [String: ThreadSummary] {
    var threadsBySessionID: [String: ThreadSummary] = [:]

    for thread in threads {
      guard let existing = threadsBySessionID[thread.activeSessionID] else {
        threadsBySessionID[thread.activeSessionID] = thread
        continue
      }

      if shouldPreferThread(thread, over: existing, for: thread.activeSessionID) {
        threadsBySessionID[thread.activeSessionID] = thread
      }
    }

    return threadsBySessionID
  }

  private func shouldPreferThread(
    _ candidate: ThreadSummary,
    over existing: ThreadSummary,
    for sessionID: String
  ) -> Bool {
    if let ownedWorkspaceID = shellState.workspaceOwner(for: sessionID) {
      if candidate.workspaceID == ownedWorkspaceID, existing.workspaceID != ownedWorkspaceID {
        return true
      }

      if existing.workspaceID == ownedWorkspaceID, candidate.workspaceID != ownedWorkspaceID {
        return false
      }
    }

    return false
  }

  private func resolvedWorkspaceID(
    for sessionID: String,
    visibleWorkspacesByID: [String: WorkspaceSummary],
    orderedVisibleWorkspaceIDs: [String],
    currentRepositoryWorkspaceID: String?,
    serverThreadsBySessionID: [String: ThreadSummary],
    cachedThreadsBySessionID: [String: ThreadSummary]
  ) -> String? {
    if let ownedWorkspaceID = shellState.workspaceOwner(for: sessionID) {
      if visibleWorkspacesByID[ownedWorkspaceID] != nil {
        return ownedWorkspaceID
      }

      if shellState.isWorkspaceHidden(id: ownedWorkspaceID)
        || shellState.isWorkspaceSuppressedFromShell(id: ownedWorkspaceID)
      {
        return nil
      }
    }

    if let cachedWorkspaceID = cachedThreadsBySessionID[sessionID]?.workspaceID {
      if visibleWorkspacesByID[cachedWorkspaceID] != nil {
        return cachedWorkspaceID
      }

      if shellState.isWorkspaceSuppressedFromShell(id: cachedWorkspaceID) {
        return nil
      }
    }

    if let serverWorkspaceID = serverThreadsBySessionID[sessionID]?.workspaceID {
      if visibleWorkspacesByID[serverWorkspaceID] != nil {
        return serverWorkspaceID
      }

      if shellState.isWorkspaceSuppressedFromShell(id: serverWorkspaceID) {
        return nil
      }
    }

    if let selectedWorkspaceID,
      visibleWorkspacesByID[selectedWorkspaceID] != nil
    {
      return selectedWorkspaceID
    }

    if let currentRepositoryWorkspaceID,
      visibleWorkspacesByID[currentRepositoryWorkspaceID] != nil
    {
      return currentRepositoryWorkspaceID
    }

    return orderedVisibleWorkspaceIDs.first
  }

  private func hydratedThread(
    from baseThread: ThreadSummary,
    session: Session,
    workspace: WorkspaceSummary
  ) -> ThreadSummary {
    let title =
      shellState.customThreadTitle(for: session.id)?.nonEmpty
      ?? session.title?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
      ?? session.label?.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
      ?? baseThread.title

    return ThreadSummary(
      id: baseThread.id.isEmpty
        ? stableEntityID(prefix: "thread", value: session.id) : baseThread.id,
      title: title,
      kind: threadKind(
        for: session.kind,
        workspace: workspace,
        baseKind: baseThread.kind
      ),
      workspaceID: workspace.id,
      worktreeID: baseThread.workspaceID == workspace.id ? baseThread.worktreeID : nil,
      activeSessionID: session.id,
      status: ThreadStatus(sessionStatus: session.status),
      preview: session.preview ?? baseThread.preview,
      model: session.model,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt
    )
  }

  private func hydratedThreadWithoutSession(
    from baseThread: ThreadSummary,
    workspace: WorkspaceSummary
  ) -> ThreadSummary {
    let title =
      shellState.customThreadTitle(for: baseThread.activeSessionID)?.nonEmpty
      ?? baseThread.title.trimmingCharacters(in: .whitespacesAndNewlines).nonEmpty
      ?? "New Thread"

    return ThreadSummary(
      id: baseThread.id.isEmpty
        ? stableEntityID(prefix: "thread", value: baseThread.activeSessionID)
        : baseThread.id,
      title: title,
      kind: normalizedThreadKind(baseKind: baseThread.kind, workspace: workspace),
      workspaceID: workspace.id,
      worktreeID: baseThread.workspaceID == workspace.id ? baseThread.worktreeID : nil,
      activeSessionID: baseThread.activeSessionID,
      status: baseThread.status,
      preview: baseThread.preview,
      model: baseThread.model,
      createdAt: baseThread.createdAt,
      updatedAt: baseThread.updatedAt
    )
  }

  private func updateThread(
    activeSessionID: String,
    mutation: (inout ThreadSummary) -> Void
  ) {
    for workspaceID in threadsByWorkspaceID.keys.sorted() {
      guard
        var workspaceThreads = threadsByWorkspaceID[workspaceID],
        let threadIndex = workspaceThreads.firstIndex(where: {
          $0.activeSessionID == activeSessionID
        }),
        workspaceThreads.indices.contains(threadIndex)
      else {
        continue
      }

      mutation(&workspaceThreads[threadIndex])
      workspaceThreads.sort()
      threadsByWorkspaceID[workspaceID] = workspaceThreads
    }
  }

  private func makeCompatibilityThread(
    for session: Session,
    workspaceID: String
  ) -> ThreadSummary {
    let workspace = workspace(workspaceID)
    let title = shellState.customThreadTitle(for: session.id)?.nonEmpty ?? session.displayTitle
    return ThreadSummary(
      id: stableEntityID(prefix: "thread", value: session.id),
      title: title,
      kind: threadKind(for: session.kind, workspace: workspace, baseKind: nil),
      workspaceID: workspaceID,
      worktreeID: nil,
      activeSessionID: session.id,
      status: ThreadStatus(sessionStatus: session.status),
      preview: session.preview,
      model: session.model,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt
    )
  }

  private func threadKind(
    for sessionKind: SessionKind,
    workspace: WorkspaceSummary?,
    baseKind: ThreadKind?
  ) -> ThreadKind {
    if let baseKind, baseKind == .automation || baseKind == .subagent {
      return baseKind
    }

    switch sessionKind {
    case .main, .channel:
      return workspace?.isGeneral == true ? .general : .coding
    case .subagent:
      return .subagent
    case .cron:
      return .automation
    }
  }

  private func normalizedThreadKind(
    baseKind: ThreadKind,
    workspace: WorkspaceSummary
  ) -> ThreadKind {
    switch baseKind {
    case .automation, .subagent:
      return baseKind
    case .general, .coding:
      return workspace.isGeneral ? .general : .coding
    }
  }

  private func resolvedThreadCreationWorkspaceID(preferredWorkspaceID: String?) -> String? {
    if let preferredWorkspaceID, workspaces.contains(where: { $0.id == preferredWorkspaceID }) {
      return preferredWorkspaceID
    }

    if let selectedWorkspaceID, workspaces.contains(where: { $0.id == selectedWorkspaceID }) {
      return selectedWorkspaceID
    }

    return workspaces.first?.id
  }

  private func workspaceScope(for workspaceID: String) -> WorkspaceScope? {
    workspace(workspaceID)?.path.nonEmpty.map(WorkspaceScope.init(explicitPath:))
  }

  private func workspaceScope(forWorktreeID worktreeID: String) -> WorkspaceScope? {
    worktreesByWorkspaceID.first { _, worktrees in
      worktrees.contains(where: { $0.id == worktreeID })
    }
    .flatMap { workspaceID, _ in
      workspaceScope(for: workspaceID)
    }
  }

  private func materializeCreatedThread(
    _ thread: ThreadSummary,
    worktree: WorktreeSummary? = nil
  ) {
    if let worktree {
      upsertWorktree(worktree, in: thread.workspaceID)
    }

    upsert(Session(threadSummary: thread))
    shellState.rememberWorkspaceOwner(thread.workspaceID, for: thread.activeSessionID)

    var threads = threadsByWorkspaceID[thread.workspaceID] ?? []
    threads.removeAll { candidate in
      candidate.id == thread.id || candidate.activeSessionID == thread.activeSessionID
    }
    threads.append(thread)
    threads.sort()
    threadsByWorkspaceID[thread.workspaceID] = threads
    expandedWorkspaceIDs.insert(thread.workspaceID)
    selectThread(id: thread.id)
  }

  private func upsertWorktree(_ worktree: WorktreeSummary, in workspaceID: String) {
    var worktrees = worktreesByWorkspaceID[workspaceID] ?? []
    worktrees.removeAll { $0.id == worktree.id }
    worktrees.append(worktree)
    worktrees.sort { lhs, rhs in
      if lhs.path == rhs.path {
        return lhs.id < rhs.id
      }
      return lhs.path.localizedStandardCompare(rhs.path) == .orderedAscending
    }
    worktreesByWorkspaceID[workspaceID] = worktrees
  }

  private func restoreArchivedThread(_ threadContext: ThreadSummary, with session: Session) {
    let restoredTitle =
      shellState.customThreadTitle(for: session.id)?.nonEmpty
      ?? session.title?
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .nonEmpty
      ?? session.label?
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .nonEmpty
      ?? threadContext.title
    let restoredThread = ThreadSummary(
      id: threadContext.id,
      title: restoredTitle,
      kind: threadContext.kind,
      workspaceID: threadContext.workspaceID,
      worktreeID: threadContext.worktreeID,
      activeSessionID: session.id,
      status: ThreadStatus(sessionStatus: session.status),
      preview: session.preview,
      model: session.model,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt
    )

    var threads = threadsByWorkspaceID[threadContext.workspaceID] ?? []
    threads.removeAll {
      $0.id == restoredThread.id || $0.activeSessionID == restoredThread.activeSessionID
    }
    threads.append(restoredThread)
    threads.sort()
    threadsByWorkspaceID[threadContext.workspaceID] = threads
    shellState.rememberWorkspaceOwner(threadContext.workspaceID, for: session.id)
    expandedWorkspaceIDs.insert(threadContext.workspaceID)
  }

  private func upsertArchivedSession(_ session: Session) {
    var archivedSession = session
    archivedSession.archived = true
    archivedSession.archivedAt = archivedSession.archivedAt ?? Int(Date().timeIntervalSince1970)

    if let index = archivedSessions.firstIndex(where: { $0.id == archivedSession.id }) {
      archivedSessions[index] = archivedSession
    } else {
      archivedSessions.append(archivedSession)
    }
    archivedSessions.sort(by: Session.sidebarSort)
  }

  func normalizedQuery(_ query: String) -> String {
    query.trimmingCharacters(in: .whitespacesAndNewlines).localizedLowercase
  }

  func matchesSearch(query: String, workspace: WorkspaceSummary) -> Bool {
    guard query.isEmpty == false else {
      return true
    }

    return [
      workspace.id,
      workspace.name,
      workspace.path,
      workspace.repo?.currentBranch ?? "",
      workspace.repo?.origin ?? "",
    ]
    .contains { $0.localizedLowercase.contains(query) }
  }

  func matchesSearch(
    query: String,
    thread: ThreadSummary,
    workspace: WorkspaceSummary
  ) -> Bool {
    guard query.isEmpty == false else {
      return true
    }

    let worktree = worktree(for: thread)
    return [
      thread.id,
      thread.activeSessionID,
      threadDisplayTitle(thread),
      thread.preview ?? "",
      thread.model,
      workspace.name,
      workspace.path,
      worktree?.label ?? "",
      worktree?.branch ?? "",
      worktree?.path ?? "",
    ]
    .contains { $0.localizedLowercase.contains(query) }
  }

  private func markSelectedThreadViewed() {
    guard let selectedThread else {
      return
    }

    markThreadViewed(selectedThread)
  }

  private func markThreadViewed(_ thread: ThreadSummary) {
    // Unread tracking assumes thread.updatedAt is monotonic for a given thread.
    // If backend timestamps can move backwards, this watermark needs a different cursor.
    viewedThreadUpdateAtByID[thread.id] = max(
      viewedThreadUpdateAtByID[thread.id] ?? 0,
      thread.updatedAt
    )
  }

  private nonisolated static func searchFields(for session: Session) -> [String] {
    [
      session.key,
      session.label ?? "",
      session.title ?? "",
      session.displayTitle,
      session.preview ?? "",
      session.model,
    ]
  }
}
