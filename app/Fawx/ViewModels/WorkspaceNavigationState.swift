import Foundation

struct WorkspaceNavigationCatalog: Sendable {
  let workspaces: [WorkspaceSummary]
  let threadsByWorkspaceID: [String: [ThreadSummary]]
  let allSessionsByID: [String: Session]
  private let threadByID: [String: ThreadSummary]
  private let threadByActiveSessionID: [String: ThreadSummary]

  init(
    workspaces: [WorkspaceSummary],
    threadsByWorkspaceID: [String: [ThreadSummary]],
    allSessionsByID: [String: Session]
  ) {
    self.workspaces = workspaces
    self.threadsByWorkspaceID = threadsByWorkspaceID
    self.allSessionsByID = allSessionsByID

    let allThreads = threadsByWorkspaceID.values.flatMap { $0 }
    self.threadByID = Dictionary(
      allThreads.map { ($0.id, $0) },
      uniquingKeysWith: { first, _ in first }
    )
    self.threadByActiveSessionID = Dictionary(
      allThreads.map { ($0.activeSessionID, $0) },
      uniquingKeysWith: { first, _ in first }
    )
  }

  func validWorkspaceID(_ workspaceID: String?) -> String? {
    guard let workspaceID, workspaces.contains(where: { $0.id == workspaceID }) else {
      return nil
    }

    return workspaceID
  }

  func validThreadID(_ threadID: String?, in workspaceID: String?) -> String? {
    guard
      let threadID,
      let workspaceID,
      threadByID[threadID]?.workspaceID == workspaceID
    else {
      return nil
    }

    return threadID
  }

  func defaultWorkspaceID() -> String? {
    workspaces.first(where: {
      (threadsByWorkspaceID[$0.id] ?? []).isEmpty == false
    })?.id ?? workspaces.first?.id
  }

  func firstThreadID(in workspaceID: String) -> String? {
    threadsByWorkspaceID[workspaceID]?.first?.id
  }

  func thread(id: String) -> ThreadSummary? {
    threadByID[id]
  }

  func thread(activeSessionID: String) -> ThreadSummary? {
    threadByActiveSessionID[activeSessionID]
  }
}

struct WorkspaceSelectionResolution: Sendable, Equatable {
    let workspaceID: String?
    let threadID: String?
    let pendingSessionSelectionID: String?
    let rewrittenSidebarSelection: SidebarSelection?
    let shouldRewriteSidebarSelection: Bool
}

struct WorkspaceNavigationState {
    private enum PersistenceKey {
        static let rememberedThreadIDs = "remembered_thread_ids_by_workspace"
    }

    private static let testDefaultsSuiteName = "ai.fawx.app.navigation.tests"

    private(set) var selectedWorkspaceID: String?
    private(set) var selectedThreadID: String?
    private(set) var pendingSessionSelectionID: String?

    private let userDefaults: UserDefaults
    private let encoder = JSONEncoder()
    private var rememberedThreadIDByWorkspaceID: [String: String]

    init(userDefaults: UserDefaults? = nil) {
        let resolvedDefaults = userDefaults ?? Self.defaultNavigationDefaults()
        self.userDefaults = resolvedDefaults
        self.rememberedThreadIDByWorkspaceID = Self.loadRememberedThreadSelections(from: resolvedDefaults)
    }

    var currentChatSelection: SidebarSelection? {
        Self.chatSelection(
            workspaceID: selectedWorkspaceID,
            threadID: selectedThreadID,
            pendingSessionSelectionID: pendingSessionSelectionID
        )
    }

    mutating func reset() {
        selectedWorkspaceID = nil
        selectedThreadID = nil
        pendingSessionSelectionID = nil
    }

    mutating func selectWorkspace(
        _ workspaceID: String,
        in catalog: WorkspaceNavigationCatalog
    ) -> SidebarSelection? {
        guard catalog.validWorkspaceID(workspaceID) != nil else {
            return nil
        }

        selectedWorkspaceID = workspaceID
        selectedThreadID = rememberedThreadID(in: workspaceID, catalog: catalog)
            ?? catalog.firstThreadID(in: workspaceID)
        pendingSessionSelectionID = nil

        if let selectedThreadID {
            rememberThread(selectedThreadID, in: workspaceID)
        }

        return .workspace(workspaceID)
    }

    mutating func selectSession(
        _ sessionID: String?,
        in catalog: WorkspaceNavigationCatalog
    ) -> SidebarSelection? {
        guard let sessionID, sessionID.isEmpty == false else {
            pendingSessionSelectionID = nil
            selectedThreadID = nil
            return selectedWorkspaceID.map(SidebarSelection.workspace)
        }

        if let thread = catalog.thread(activeSessionID: sessionID) {
            return applySelectedThread(thread, persistSelection: true)
        }

        pendingSessionSelectionID = sessionID
        selectedThreadID = nil
        if selectedWorkspaceID == nil {
            selectedWorkspaceID = catalog.defaultWorkspaceID()
        }

        return .thread(.activeSessionID(sessionID))
    }

    mutating func applySelectedThread(
        _ thread: ThreadSummary,
        persistSelection: Bool
    ) -> SidebarSelection? {
        selectedWorkspaceID = thread.workspaceID
        selectedThreadID = thread.id
        pendingSessionSelectionID = nil
        rememberThread(thread.id, in: thread.workspaceID)

        guard persistSelection else {
            return nil
        }

        return .thread(.threadID(thread.id))
    }

    mutating func restoreSelectionAfterRefresh(
        sidebarSelection: SidebarSelection?,
        in catalog: WorkspaceNavigationCatalog
    ) -> WorkspaceSelectionResolution {
        guard catalog.workspaces.isEmpty == false else {
            reset()
            return WorkspaceSelectionResolution(
                workspaceID: nil,
                threadID: nil,
                pendingSessionSelectionID: nil,
                rewrittenSidebarSelection: nil,
                shouldRewriteSidebarSelection: sidebarSelection?.isChatSelection == true
            )
        }

        sanitizeRememberedThreadSelections(in: catalog)

        let resolution = Self.resolveSelection(
            from: sidebarSelection,
            currentWorkspaceID: selectedWorkspaceID,
            currentThreadID: selectedThreadID,
            pendingSessionSelectionID: pendingSessionSelectionID,
            rememberedThreadIDByWorkspaceID: rememberedThreadIDByWorkspaceID,
            in: catalog
        )

        selectedWorkspaceID = resolution.workspaceID
        selectedThreadID = resolution.threadID
        pendingSessionSelectionID = resolution.pendingSessionSelectionID

        if let workspaceID = selectedWorkspaceID, let threadID = selectedThreadID {
            pendingSessionSelectionID = nil
            rememberThread(threadID, in: workspaceID)
        }

        return resolution
    }

    mutating func clearPendingSessionSelection(matching sessionID: String) {
        guard pendingSessionSelectionID == sessionID else {
            return
        }

        pendingSessionSelectionID = nil
    }

    mutating func forgetRememberedThreadSelections(matching removedThreadIDsByWorkspaceID: [String: String]) {
        var didMutate = false
        for (workspaceID, removedThreadID) in removedThreadIDsByWorkspaceID {
            guard rememberedThreadIDByWorkspaceID[workspaceID] == removedThreadID else {
                continue
            }

            rememberedThreadIDByWorkspaceID.removeValue(forKey: workspaceID)
            didMutate = true
        }

        if didMutate {
            persistRememberedThreadSelections()
        }
    }

    mutating func forgetRememberedThreadSelections(in workspaceIDs: Set<String>) {
        let filteredSelections = rememberedThreadIDByWorkspaceID.filter { workspaceIDs.contains($0.key) == false }
        guard filteredSelections != rememberedThreadIDByWorkspaceID else {
            return
        }

        rememberedThreadIDByWorkspaceID = filteredSelections
        persistRememberedThreadSelections()
    }

    mutating func restoreSelectionAfterRemovingSelectedThread(
        in catalog: WorkspaceNavigationCatalog
    ) -> SidebarSelection? {
        selectedThreadID = nil
        pendingSessionSelectionID = nil

        guard let selectedWorkspaceID else {
            return currentChatSelection
        }

        selectedThreadID = rememberedThreadID(in: selectedWorkspaceID, catalog: catalog)
            ?? catalog.firstThreadID(in: selectedWorkspaceID)
        if let selectedThreadID {
            rememberThread(selectedThreadID, in: selectedWorkspaceID)
        }

        return currentChatSelection
    }

    static func resolveSelection(
        from selection: SidebarSelection?,
        currentWorkspaceID: String?,
        currentThreadID: String?,
        pendingSessionSelectionID: String?,
        rememberedThreadIDByWorkspaceID: [String: String],
        in catalog: WorkspaceNavigationCatalog
    ) -> WorkspaceSelectionResolution {
        var resolvedWorkspaceID = catalog.validWorkspaceID(currentWorkspaceID)
        var resolvedThreadID = catalog.validThreadID(currentThreadID, in: resolvedWorkspaceID)
        var resolvedPendingSessionSelectionID = pendingSessionSelectionID
        var shouldRewriteChatSelection = false
        var rewriteAsResolvedWorkspaceSelection = false
        var preservesWorkspaceOnlySelection = false
        var rewrittenSidebarSelection: SidebarSelection?

        switch selection {
        case .workspace(let workspaceID):
            let restoredWorkspaceID = catalog.validWorkspaceID(workspaceID)
            resolvedWorkspaceID = restoredWorkspaceID
            resolvedThreadID = catalog.validThreadID(currentThreadID, in: restoredWorkspaceID)
            resolvedPendingSessionSelectionID = nil
            rewriteAsResolvedWorkspaceSelection = restoredWorkspaceID == nil
            preservesWorkspaceOnlySelection = restoredWorkspaceID != nil
                && currentWorkspaceID == restoredWorkspaceID
                && currentThreadID == nil
                && resolvedThreadID == nil

        case .thread(let reference):
            if let threadID = reference.threadID {
                if let thread = catalog.thread(id: threadID) {
                    resolvedWorkspaceID = thread.workspaceID
                    resolvedThreadID = thread.id
                    resolvedPendingSessionSelectionID = nil
                } else {
                    resolvedPendingSessionSelectionID = nil
                    resolvedThreadID = nil
                    shouldRewriteChatSelection = true
                }
            } else if let sessionID = reference.sessionID {
                if let thread = catalog.thread(activeSessionID: sessionID) {
                    resolvedWorkspaceID = thread.workspaceID
                    resolvedThreadID = thread.id
                    resolvedPendingSessionSelectionID = nil
                    rewrittenSidebarSelection = .thread(.threadID(thread.id))
                } else if catalog.allSessionsByID[sessionID] != nil {
                    resolvedPendingSessionSelectionID = sessionID
                    resolvedThreadID = nil
                    resolvedWorkspaceID = resolvedWorkspaceID ?? catalog.defaultWorkspaceID()
                } else {
                    resolvedPendingSessionSelectionID = nil
                    resolvedThreadID = nil
                    shouldRewriteChatSelection = true
                }
            }

        case .skills, .fleet, .experiments, .git, .settings, .none:
            break
        }

        resolvedWorkspaceID = resolvedWorkspaceID ?? catalog.defaultWorkspaceID()

        if let resolvedWorkspaceID {
            let validThreadID = catalog.validThreadID(resolvedThreadID, in: resolvedWorkspaceID)
            if preservesWorkspaceOnlySelection {
                resolvedThreadID = validThreadID
            } else {
                resolvedThreadID = validThreadID
                    ?? rememberedThreadID(
                        in: resolvedWorkspaceID,
                        rememberedThreadIDByWorkspaceID: rememberedThreadIDByWorkspaceID,
                        catalog: catalog
                    )
                    ?? catalog.firstThreadID(in: resolvedWorkspaceID)
            }
        }

        if resolvedWorkspaceID != nil, resolvedThreadID != nil {
            resolvedPendingSessionSelectionID = nil
        }

        if rewriteAsResolvedWorkspaceSelection, let resolvedWorkspaceID {
            rewrittenSidebarSelection = .workspace(resolvedWorkspaceID)
        } else if shouldRewriteChatSelection, selection?.isChatSelection == true {
            rewrittenSidebarSelection = chatSelection(
                workspaceID: resolvedWorkspaceID,
                threadID: resolvedThreadID,
                pendingSessionSelectionID: resolvedPendingSessionSelectionID
            )
        }

        return WorkspaceSelectionResolution(
            workspaceID: resolvedWorkspaceID,
            threadID: resolvedThreadID,
            pendingSessionSelectionID: resolvedPendingSessionSelectionID,
            rewrittenSidebarSelection: rewrittenSidebarSelection,
            shouldRewriteSidebarSelection: rewrittenSidebarSelection != nil
                || (shouldRewriteChatSelection && selection?.isChatSelection == true)
        )
    }

    private mutating func sanitizeRememberedThreadSelections(in catalog: WorkspaceNavigationCatalog) {
        let sanitized = rememberedThreadIDByWorkspaceID.filter { workspaceID, threadID in
            catalog.threadsByWorkspaceID[workspaceID]?.contains(where: { $0.id == threadID }) == true
        }

        guard sanitized != rememberedThreadIDByWorkspaceID else {
            return
        }

        rememberedThreadIDByWorkspaceID = sanitized
        persistRememberedThreadSelections()
    }

    private func rememberedThreadID(
        in workspaceID: String,
        catalog: WorkspaceNavigationCatalog
    ) -> String? {
        Self.rememberedThreadID(
            in: workspaceID,
            rememberedThreadIDByWorkspaceID: rememberedThreadIDByWorkspaceID,
            catalog: catalog
        )
    }

    private mutating func rememberThread(_ threadID: String, in workspaceID: String) {
        guard rememberedThreadIDByWorkspaceID[workspaceID] != threadID else {
            return
        }

        rememberedThreadIDByWorkspaceID[workspaceID] = threadID
        persistRememberedThreadSelections()
    }

    private func persistRememberedThreadSelections() {
        do {
            let data = try encoder.encode(rememberedThreadIDByWorkspaceID)
            userDefaults.set(data, forKey: PersistenceKey.rememberedThreadIDs)
        } catch {
            assertionFailure(
                "Failed to encode remembered workspace thread selections: \(error.localizedDescription)"
            )
        }
    }

    private static func rememberedThreadID(
        in workspaceID: String,
        rememberedThreadIDByWorkspaceID: [String: String],
        catalog: WorkspaceNavigationCatalog
    ) -> String? {
        guard let threadID = rememberedThreadIDByWorkspaceID[workspaceID] else {
            return nil
        }

        return catalog.threadsByWorkspaceID[workspaceID]?.contains(where: { $0.id == threadID }) == true
            ? threadID
            : nil
    }

    private static func chatSelection(
        workspaceID: String?,
        threadID: String?,
        pendingSessionSelectionID: String?
    ) -> SidebarSelection? {
        if let threadID {
            return .thread(.threadID(threadID))
        }

        if let pendingSessionSelectionID {
            return .thread(.activeSessionID(pendingSessionSelectionID))
        }

        if let workspaceID {
            return .workspace(workspaceID)
        }

        return nil
    }

    private static func loadRememberedThreadSelections(from userDefaults: UserDefaults) -> [String: String] {
        guard let data = userDefaults.data(forKey: PersistenceKey.rememberedThreadIDs) else {
            return [:]
        }

        do {
            return try JSONDecoder().decode([String: String].self, from: data)
        } catch {
            assertionFailure(
                "Failed to decode remembered workspace thread selections: \(error.localizedDescription)"
            )
            return [:]
        }
    }

    static func defaultNavigationDefaults() -> UserDefaults {
        if
            UITestLaunchOptions.defaultsSuiteOverride != nil,
            let defaults = UserDefaults(suiteName: UITestLaunchOptions.defaultsSuiteOverride)
        {
            return defaults
        }

        if
            ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil,
            let defaults = UserDefaults(suiteName: testDefaultsSuiteName)
        {
            defaults.removePersistentDomain(forName: testDefaultsSuiteName)
            return defaults
        }

        return .standard
    }
}
