import Foundation

struct WorkspaceShellState {
    private struct HiddenWorkspaceDescriptor: Codable, Hashable {
        let id: String
        let path: String

        func matches(_ workspace: WorkspaceSummary) -> Bool {
            id == workspace.id || (path.isEmpty == false && path == workspace.path)
        }
    }

    private struct VisibleWorkspaceSync {
        let visibleWorkspaces: [WorkspaceSummary]
        let pinnedWorkspaces: [WorkspaceSummary]
        let suppressedWorkspaceIDs: Set<String>
        let didChangePinnedWorkspaces: Bool
    }

    private enum PersistenceKey {
        static let pinnedWorkspaces = "workspace_shell_pinned_workspaces"
        static let hiddenWorkspaces = "workspace_shell_hidden_workspaces"
        static let threadWorkspaceOwnership = "workspace_shell_thread_workspace_ownership"
        static let customThreadTitles = "workspace_shell_custom_thread_titles"
        static let manualThreadOrder = "workspace_shell_manual_thread_order"
    }

    private let userDefaults: UserDefaults
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    private(set) var pinnedWorkspaces: [WorkspaceSummary]
    private var hiddenWorkspaces: Set<HiddenWorkspaceDescriptor>
    private var suppressedWorkspaceIDs: Set<String> = []
    private(set) var workspaceIDBySessionID: [String: String]
    private(set) var customThreadTitleBySessionID: [String: String]
    private(set) var manualThreadOrderByWorkspaceID: [String: [String]]

    init(userDefaults: UserDefaults) {
        self.userDefaults = userDefaults
        let loadedPinnedWorkspaces = Self.loadWorkspaces(
            from: userDefaults,
            forKey: PersistenceKey.pinnedWorkspaces
        )
        self.pinnedWorkspaces = Self.deduplicatedWorkspaces(loadedPinnedWorkspaces)
        self.hiddenWorkspaces = Self.loadSet(
            from: userDefaults,
            forKey: PersistenceKey.hiddenWorkspaces
        )
        self.workspaceIDBySessionID = Self.loadMap(
            from: userDefaults,
            forKey: PersistenceKey.threadWorkspaceOwnership
        )
        self.customThreadTitleBySessionID = Self.loadMap(
            from: userDefaults,
            forKey: PersistenceKey.customThreadTitles
        )
        self.manualThreadOrderByWorkspaceID = Self.loadNestedMap(
            from: userDefaults,
            forKey: PersistenceKey.manualThreadOrder
        )

        if pinnedWorkspaces != loadedPinnedWorkspaces {
            persistPinnedWorkspaces()
        }
    }

    func visibleWorkspaces(merging serverWorkspaces: [WorkspaceSummary]) -> [WorkspaceSummary] {
        mergedVisibleWorkspaceSync(with: serverWorkspaces).visibleWorkspaces
    }

    mutating func syncVisibleWorkspaces(with serverWorkspaces: [WorkspaceSummary]) -> [WorkspaceSummary] {
        let sync = mergedVisibleWorkspaceSync(with: serverWorkspaces)
        suppressedWorkspaceIDs = sync.suppressedWorkspaceIDs
        guard sync.didChangePinnedWorkspaces else {
            return sync.visibleWorkspaces
        }

        pinnedWorkspaces = sync.pinnedWorkspaces
        persistPinnedWorkspaces()
        return sync.visibleWorkspaces
    }

    mutating func addWorkspace(path: String) -> WorkspaceSummary {
        let normalizedPath = URL(fileURLWithPath: path, isDirectory: true)
            .standardizedFileURL
            .path
        let workspace = workspaceSummary(
            for: normalizedPath,
            existingWorkspace: pinnedWorkspaces.first(where: { existingWorkspace in
                existingWorkspace.id == stableEntityID(prefix: "workspace", value: normalizedPath)
                    || existingWorkspace.path == normalizedPath
            }),
            lastOpenedAt: Int(Date().timeIntervalSince1970)
        )
        revealWorkspace(workspace)

        if let index = pinnedWorkspaces.firstIndex(where: { existingWorkspace in
            existingWorkspace.id == workspace.id || existingWorkspace.path == normalizedPath
        }) {
            if pinnedWorkspaces[index] != workspace {
                pinnedWorkspaces[index] = workspace
                persistPinnedWorkspaces()
            }
            return workspace
        }

        pinnedWorkspaces.append(workspace)
        persistPinnedWorkspaces()
        return workspace
    }

    mutating func addWorkspace(_ workspace: WorkspaceSummary) -> WorkspaceSummary {
        guard workspace.isGeneral == false else {
            return workspace
        }

        revealWorkspace(workspace)

        if let index = pinnedWorkspaces.firstIndex(where: { candidate in
            candidate.id == workspace.id || candidate.path == workspace.path
        }) {
            if pinnedWorkspaces[index] != workspace {
                pinnedWorkspaces[index] = workspace
                persistPinnedWorkspaces()
            }
            return workspace
        }

        pinnedWorkspaces.append(workspace)
        persistPinnedWorkspaces()
        return workspace
    }

    mutating func removeWorkspace(_ workspace: WorkspaceSummary) {
        guard workspace.isGeneral == false else {
            return
        }

        pinnedWorkspaces.removeAll { candidate in
            candidate.id == workspace.id || candidate.path == workspace.path
        }
        hiddenWorkspaces.insert(hiddenDescriptor(for: workspace))
        persistPinnedWorkspaces()
        persistHiddenWorkspaces()
    }

    func isWorkspaceHidden(id workspaceID: String) -> Bool {
        hiddenWorkspaces.contains { $0.id == workspaceID }
    }

    func isWorkspaceSuppressedFromShell(id workspaceID: String) -> Bool {
        suppressedWorkspaceIDs.contains(workspaceID)
    }

    mutating func moveWorkspaces(
        currentWorkspaces: [WorkspaceSummary],
        fromOffsets: IndexSet,
        toOffset: Int
    ) -> [WorkspaceSummary] {
        let reorderedWorkspaces = Self.moved(
            currentWorkspaces,
            fromOffsets: fromOffsets,
            toOffset: toOffset
        )
        pinnedWorkspaces = reorderedWorkspaces
        persistPinnedWorkspaces()
        return reorderedWorkspaces
    }

    mutating func moveThreads(
        _ threadIDs: [String],
        in workspaceID: String,
        fromOffsets: IndexSet,
        toOffset: Int
    ) {
        manualThreadOrderByWorkspaceID[workspaceID] = Self.moved(
            threadIDs,
            fromOffsets: fromOffsets,
            toOffset: toOffset
        )
        persistManualThreadOrder()
    }

    func orderedThreads(
        _ threads: [ThreadSummary],
        in workspaceID: String,
        fallbackSort: (ThreadSummary, ThreadSummary) -> Bool
    ) -> [ThreadSummary] {
        let fallbackOrderedThreads = threads.sorted(by: fallbackSort)
        guard
            let manuallyOrderedThreadIDs = manualThreadOrderByWorkspaceID[workspaceID],
            manuallyOrderedThreadIDs.isEmpty == false
        else {
            return fallbackOrderedThreads
        }

        let threadsByID = Dictionary(uniqueKeysWithValues: fallbackOrderedThreads.map { ($0.id, $0) })
        var orderedThreads: [ThreadSummary] = []
        var seenThreadIDs: Set<String> = []

        for threadID in manuallyOrderedThreadIDs {
            guard let thread = threadsByID[threadID] else {
                continue
            }
            orderedThreads.append(thread)
            seenThreadIDs.insert(threadID)
        }

        orderedThreads.append(
            contentsOf: fallbackOrderedThreads.filter { seenThreadIDs.contains($0.id) == false }
        )
        return orderedThreads
    }

    mutating func sanitizeManualThreadOrder(using threadsByWorkspaceID: [String: [ThreadSummary]]) {
        let validThreadIDsByWorkspaceID = Dictionary(
            uniqueKeysWithValues: threadsByWorkspaceID.map { workspaceID, threads in
                (workspaceID, Set(threads.map(\.id)))
            }
        )
        var sanitized = manualThreadOrderByWorkspaceID
        var didMutate = false

        for workspaceID in sanitized.keys.sorted() {
            guard let validThreadIDs = validThreadIDsByWorkspaceID[workspaceID] else {
                sanitized.removeValue(forKey: workspaceID)
                didMutate = true
                continue
            }

            let filteredThreadIDs = (sanitized[workspaceID] ?? []).filter { validThreadIDs.contains($0) }
            if filteredThreadIDs.isEmpty {
                if sanitized.removeValue(forKey: workspaceID) != nil {
                    didMutate = true
                }
            } else if filteredThreadIDs != sanitized[workspaceID] {
                sanitized[workspaceID] = filteredThreadIDs
                didMutate = true
            }
        }

        guard didMutate else {
            return
        }

        manualThreadOrderByWorkspaceID = sanitized
        persistManualThreadOrder()
    }

    func workspaceOwner(for sessionID: String) -> String? {
        workspaceIDBySessionID[sessionID]
    }

    mutating func rememberWorkspaceOwner(
        _ workspaceID: String,
        for sessionID: String
    ) {
        guard workspaceIDBySessionID[sessionID] != workspaceID else {
            return
        }

        workspaceIDBySessionID[sessionID] = workspaceID
        persistThreadWorkspaceOwnership()
    }

    func customThreadTitle(for sessionID: String) -> String? {
        customThreadTitleBySessionID[sessionID]
    }

    mutating func renameThread(sessionID: String, title: String) {
        let normalizedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if normalizedTitle.isEmpty {
            if customThreadTitleBySessionID.removeValue(forKey: sessionID) != nil {
                persistCustomThreadTitles()
            }
            return
        }

        guard customThreadTitleBySessionID[sessionID] != normalizedTitle else {
            return
        }

        customThreadTitleBySessionID[sessionID] = normalizedTitle
        persistCustomThreadTitles()
    }

    mutating func forgetDeletedSession(_ sessionID: String) {
        let threadID = stableEntityID(prefix: "thread", value: sessionID)
        var didMutateThreadOrder = false

        if workspaceIDBySessionID.removeValue(forKey: sessionID) != nil {
            persistThreadWorkspaceOwnership()
        }
        if customThreadTitleBySessionID.removeValue(forKey: sessionID) != nil {
            persistCustomThreadTitles()
        }

        for workspaceID in manualThreadOrderByWorkspaceID.keys.sorted() {
            let filteredThreadIDs = (manualThreadOrderByWorkspaceID[workspaceID] ?? []).filter { $0 != threadID }
            if filteredThreadIDs.isEmpty {
                if manualThreadOrderByWorkspaceID.removeValue(forKey: workspaceID) != nil {
                    didMutateThreadOrder = true
                }
            } else if filteredThreadIDs != manualThreadOrderByWorkspaceID[workspaceID] {
                manualThreadOrderByWorkspaceID[workspaceID] = filteredThreadIDs
                didMutateThreadOrder = true
            }
        }

        if didMutateThreadOrder {
            persistManualThreadOrder()
        }
    }

#if DEBUG
    mutating func resetForTesting() {
        pinnedWorkspaces = []
        hiddenWorkspaces = []
        suppressedWorkspaceIDs = []
        workspaceIDBySessionID = [:]
        customThreadTitleBySessionID = [:]
        manualThreadOrderByWorkspaceID = [:]
        persistPinnedWorkspaces()
        persistHiddenWorkspaces()
        persistThreadWorkspaceOwnership()
        persistCustomThreadTitles()
        persistManualThreadOrder()
    }

    mutating func seedOwnership(using threadsByWorkspaceID: [String: [ThreadSummary]]) {
        var ownership: [String: String] = [:]
        for (workspaceID, threads) in threadsByWorkspaceID {
            for thread in threads {
                ownership[thread.activeSessionID] = workspaceID
            }
        }
        workspaceIDBySessionID = ownership
        persistThreadWorkspaceOwnership()
    }
#endif

    private func persistPinnedWorkspaces() {
        persist(pinnedWorkspaces, forKey: PersistenceKey.pinnedWorkspaces)
    }

    private func persistHiddenWorkspaces() {
        persist(hiddenWorkspaces, forKey: PersistenceKey.hiddenWorkspaces)
    }

    private func persistThreadWorkspaceOwnership() {
        persist(workspaceIDBySessionID, forKey: PersistenceKey.threadWorkspaceOwnership)
    }

    private func persistCustomThreadTitles() {
        persist(customThreadTitleBySessionID, forKey: PersistenceKey.customThreadTitles)
    }

    private func persistManualThreadOrder() {
        persist(manualThreadOrderByWorkspaceID, forKey: PersistenceKey.manualThreadOrder)
    }

    private func persist<Value: Encodable>(_ value: Value, forKey key: String) {
        do {
            let data = try encoder.encode(value)
            userDefaults.set(data, forKey: key)
        } catch {
            assertionFailure("Failed to encode workspace shell state for \(key): \(error.localizedDescription)")
        }
    }

    private static func loadWorkspaces(
        from userDefaults: UserDefaults,
        forKey key: String
    ) -> [WorkspaceSummary] {
        guard let data = userDefaults.data(forKey: key) else {
            return []
        }

        do {
            return try JSONDecoder().decode([WorkspaceSummary].self, from: data)
        } catch {
            assertionFailure("Failed to decode workspace shell state for \(key): \(error.localizedDescription)")
            return []
        }
    }

    private static func loadMap(
        from userDefaults: UserDefaults,
        forKey key: String
    ) -> [String: String] {
        guard let data = userDefaults.data(forKey: key) else {
            return [:]
        }

        do {
            return try JSONDecoder().decode([String: String].self, from: data)
        } catch {
            assertionFailure("Failed to decode workspace shell state for \(key): \(error.localizedDescription)")
            return [:]
        }
    }

    private static func loadSet<Value: Decodable & Hashable>(
        from userDefaults: UserDefaults,
        forKey key: String
    ) -> Set<Value> {
        guard let data = userDefaults.data(forKey: key) else {
            return []
        }

        do {
            return try JSONDecoder().decode(Set<Value>.self, from: data)
        } catch {
            assertionFailure("Failed to decode workspace shell state for \(key): \(error.localizedDescription)")
            return []
        }
    }

    private static func loadNestedMap(
        from userDefaults: UserDefaults,
        forKey key: String
    ) -> [String: [String]] {
        guard let data = userDefaults.data(forKey: key) else {
            return [:]
        }

        do {
            return try JSONDecoder().decode([String: [String]].self, from: data)
        } catch {
            assertionFailure("Failed to decode workspace shell state for \(key): \(error.localizedDescription)")
            return [:]
        }
    }

    private static func workspaceName(for path: String) -> String {
        let candidate = URL(fileURLWithPath: path, isDirectory: true)
            .standardizedFileURL
            .lastPathComponent
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return candidate.isEmpty ? path : candidate
    }

    private static func deduplicatedWorkspaces(_ workspaces: [WorkspaceSummary]) -> [WorkspaceSummary] {
        var deduplicated: [WorkspaceSummary] = []
        var seenIDs: Set<String> = []
        var seenPaths: Set<String> = []

        for workspace in workspaces {
            let path = workspace.path.trimmingCharacters(in: .whitespacesAndNewlines)
            guard seenIDs.contains(workspace.id) == false else {
                continue
            }
            guard path.isEmpty || seenPaths.contains(path) == false else {
                continue
            }

            deduplicated.append(workspace)
            seenIDs.insert(workspace.id)
            if path.isEmpty == false {
                seenPaths.insert(path)
            }
        }

        return deduplicated
    }

    private func isWorkspaceHidden(_ workspace: WorkspaceSummary) -> Bool {
        hiddenWorkspaces.contains(hiddenDescriptor(for: workspace))
            || hiddenWorkspaces.contains(where: { $0.matches(workspace) })
    }

    mutating func revealWorkspace(_ workspace: WorkspaceSummary) {
        let descriptor = hiddenDescriptor(for: workspace)
        let updatedHiddenWorkspaces = hiddenWorkspaces.filter { $0 != descriptor && $0.matches(workspace) == false }
        guard updatedHiddenWorkspaces != hiddenWorkspaces else {
            return
        }

        hiddenWorkspaces = updatedHiddenWorkspaces
        persistHiddenWorkspaces()
    }

    private func hiddenDescriptor(for workspace: WorkspaceSummary) -> HiddenWorkspaceDescriptor {
        HiddenWorkspaceDescriptor(
            id: workspace.id,
            path: workspace.path
        )
    }

    private func mergedVisibleWorkspaceSync(
        with serverWorkspaces: [WorkspaceSummary]
    ) -> VisibleWorkspaceSync {
        let suppressedWorkspaceIDs = Set(serverWorkspaces.filter(\.isGeneral).map(\.id))
        let repositoryWorkspaces = Self.deduplicatedWorkspaces(
            serverWorkspaces.filter { workspace in
                workspace.isGeneral == false && isWorkspaceHidden(workspace) == false
            }
        )
        let normalizedPinnedWorkspaces = Self.deduplicatedWorkspaces(pinnedWorkspaces)
        var nextPinnedWorkspaces = normalizedPinnedWorkspaces
        var didMutate = normalizedPinnedWorkspaces != pinnedWorkspaces

        for workspace in repositoryWorkspaces {
            if let index = nextPinnedWorkspaces.firstIndex(where: { candidate in
                candidate.id == workspace.id || candidate.path == workspace.path
            }) {
                guard nextPinnedWorkspaces[index] != workspace else {
                    continue
                }

                nextPinnedWorkspaces[index] = workspace
                didMutate = true
            } else {
                nextPinnedWorkspaces.append(workspace)
                didMutate = true
            }
        }

        let visibleWorkspaces = nextPinnedWorkspaces.isEmpty ? repositoryWorkspaces : nextPinnedWorkspaces

        return VisibleWorkspaceSync(
            visibleWorkspaces: visibleWorkspaces,
            pinnedWorkspaces: nextPinnedWorkspaces,
            suppressedWorkspaceIDs: suppressedWorkspaceIDs,
            didChangePinnedWorkspaces: didMutate
        )
    }

    private func workspaceSummary(
        for normalizedPath: String,
        existingWorkspace: WorkspaceSummary?,
        lastOpenedAt: Int
    ) -> WorkspaceSummary {
        WorkspaceSummary(
            id: stableEntityID(prefix: "workspace", value: normalizedPath),
            name: Self.workspaceName(for: normalizedPath),
            path: normalizedPath,
            kind: .repository,
            repo: existingWorkspace?.repo,
            lastOpenedAt: lastOpenedAt
        )
    }

    private static func moved<Element>(
        _ elements: [Element],
        fromOffsets: IndexSet,
        toOffset: Int
    ) -> [Element] {
        let movingElements = fromOffsets.map { elements[$0] }
        var remainingElements: [Element] = []
        remainingElements.reserveCapacity(elements.count - movingElements.count)

        for (index, element) in elements.enumerated() where fromOffsets.contains(index) == false {
            remainingElements.append(element)
        }

        let adjustedOffset = max(
            0,
            min(
                toOffset - fromOffsets.filter { $0 < toOffset }.count,
                remainingElements.count
            )
        )
        remainingElements.insert(contentsOf: movingElements, at: adjustedOffset)
        return remainingElements
    }
}
