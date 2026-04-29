import Observation
import SwiftUI

#if os(macOS)
  import AppKit
#endif

struct ContentView: View {
  enum Layout {
    static let sidebarMinWidth = FawxSpacing.sidebarWidth
    static let sidebarIdealWidth = FawxSpacing.sidebarWidth + FawxSpacing.paddingXL
    static let sidebarMaxWidth = FawxSpacing.sidebarWidth + (FawxSpacing.paddingXL * 2)
    static let compactGitPanelMinWidth: CGFloat = 280
    static let compactGitPanelIdealWidth: CGFloat = 340
    static let compactGitPanelMaxWidth: CGFloat = 420
  }

  fileprivate enum WorktreeDraftMode: String, Identifiable {
    case thread
    case worktree

    var id: String { rawValue }
  }

  private struct WorktreeDraft: Identifiable {
    let workspaceID: String
    let mode: WorktreeDraftMode
    var title = ""
    var branch = ""
    var baseRef = ""

    var id: String {
      "\(workspaceID)-\(mode.rawValue)"
    }
  }

  @Bindable var appState: AppState
  @Bindable var sessionViewModel: SessionViewModel
  @Bindable var chatViewModel: ChatViewModel
  @Bindable var skillsViewModel: SkillsViewModel
  @Bindable var fleetViewModel: FleetViewModel
  @Bindable var experimentsViewModel: ExperimentsViewModel
  @Bindable var gitViewModel: GitViewModel
  @Bindable var settingsViewModel: SettingsViewModel
  @Bindable var permissionsViewModel: PermissionsViewModel
  @Bindable var telemetryViewModel: TelemetryViewModel
  @Bindable var synthesisViewModel: SynthesisViewModel
  @Bindable var usageViewModel: UsageViewModel

  @SceneStorage("sidebar_selection") private var sidebarSelectionRawValue: String?
  @AppStorage("show_git_panel") private var showInspectorPanel = false
  @AppStorage("show_threads_sidebar") private var showThreadsSidebar = true
  @State private var presentedSessionMemory: Session?
  @State private var worktreeDraft: WorktreeDraft?

  var body: some View {
    VStack(spacing: 0) {
      connectionBannerView
      mainContent
      statusBarView
    }
    .background(Color.fawxBackground)
    .overlay(alignment: .top) {
      toastOverlay
    }
    .task {
      await loadInitialContent()
    }
    .onChange(of: sessionViewModel.selectedSessionID) { _, newValue in
      handleSelectedSessionChange(newValue)
    }
    .onChange(of: selectedThreadContextRefreshID) { _, _ in
      syncThreadInspectorContext()
    }
    .onChange(of: appState.sidebarSelection) { _, newValue in
      handleSidebarSelectionChange(newValue)
    }
    .sessionMemoryPresentation(appState: appState, presentedSession: $presentedSessionMemory)
    .sheet(item: $worktreeDraft) { draft in
      WorktreeLifecycleSheet(
        workspaceName: sessionViewModel.workspace(draft.workspaceID)?.name ?? "Workspace",
        mode: draft.mode,
        initialTitle: draft.title,
        initialBranch: draft.branch,
        initialBaseRef: draft.baseRef,
        onCancel: {
          worktreeDraft = nil
        },
        onCreate: { title, branch, baseRef in
          handleWorktreeDraftSubmission(
            workspaceID: draft.workspaceID,
            mode: draft.mode,
            title: title,
            branch: branch,
            baseRef: baseRef
          )
        }
      )
      .frame(minWidth: 420, idealWidth: 480)
      .fawxOpaqueModalPresentation()
    }
  }

  @ViewBuilder
  private var connectionBannerView: some View {
    if let banner = appState.connectionBanner {
      ConnectionBannerView(banner: banner) {
        Task {
          await appState.retryConnection()
        }
      }
    }
  }

  private var mainContent: some View {
#if os(macOS)
    macOSMainContent
#else
    NavigationSplitView {
      sidebarView
    } detail: {
      detailView
    }
    .navigationSplitViewStyle(.balanced)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .layoutPriority(1)
    .toolbar {
      gitPanelToolbar
    }
#endif
  }

#if os(macOS)
  private var macOSMainContent: some View {
    HSplitView {
      if showThreadsSidebar {
        sidebarView
          .frame(
            minWidth: Layout.sidebarMinWidth,
            idealWidth: Layout.sidebarIdealWidth,
            maxWidth: Layout.sidebarMaxWidth,
            maxHeight: .infinity
          )
      }

      detailShell
    }
    .background(Color.fawxBackground)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .layoutPriority(1)
  }

  private var detailShell: some View {
    VStack(spacing: 0) {
      HStack(alignment: .center, spacing: 0) {
        Spacer(minLength: 0)
        shellDropdownCluster
      }
      .frame(maxWidth: .infinity, minHeight: 28, alignment: .trailing)
      .padding(.horizontal, FawxSpacing.paddingSM)
      .background(Color.fawxBackground)

      detailView
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }

  private var shellDropdownCluster: some View {
    HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
      BranchContextMenuButton(
        title: branchMenuTitle,
        context: selectedThreadContext,
        repositoryTargets: sessionViewModel.gitRepositoryTargets,
        selectedRepositoryTarget: gitViewModel.repositoryTarget,
        selectRepositoryTarget: bindGitRepositoryTarget,
        openGitView: openGitView
      )

      ShellPanelMenuButton(
        showsThreadsSidebar: showThreadsSidebar,
        canShowInspector: isChatSectionSelected,
        showsInspector: shouldShowInspector,
        toggleThreadsSidebar: toggleThreadsSidebar,
        toggleInspector: toggleInspector
      )
    }
    .accessibilityIdentifier("shellDropdownCluster")
  }
#endif

  private var sidebarView: some View {
    Sidebar(
      sessionViewModel: sessionViewModel,
      selection: sidebarSelection,
      streamingSessionIDs: chatViewModel.activeStreamSessionIDs,
      actions: sidebarActions
    )
    .navigationSplitViewColumnWidth(
      min: Layout.sidebarMinWidth,
      ideal: Layout.sidebarIdealWidth,
      max: Layout.sidebarMaxWidth
    )
  }

  @ViewBuilder
  private var detailView: some View {
    switch sidebarSelection {
    case .skills:
      SkillsView(skillsViewModel: skillsViewModel)
        .navigationTitle("Skills")
    case .fleet:
      FleetView(viewModel: fleetViewModel)
        .navigationTitle("Fleet")
    case .experiments:
      ExperimentsView(viewModel: experimentsViewModel)
        .navigationTitle("Experiments")
    case .git:
      GitView(
        viewModel: gitViewModel,
        repositoryTargets: sessionViewModel.gitRepositoryTargets,
        defaultRepositoryTarget: sessionViewModel.defaultGitRepositoryTarget,
        selectRepositoryTarget: bindGitRepositoryTarget
      )
        .navigationTitle("Git")
    case .settings:
      SettingsView(
        settingsViewModel: settingsViewModel,
        appState: appState,
        sessionViewModel: sessionViewModel,
        chatViewModel: chatViewModel,
        permissionsViewModel: permissionsViewModel,
        telemetryViewModel: telemetryViewModel,
        synthesisViewModel: synthesisViewModel,
        usageViewModel: usageViewModel
      )
      .navigationTitle("Settings")
    case .thread, .workspace, .none:
      chatShellContainer
        .navigationTitle(detailTitle)
    }
  }

  private var chatDetail: some View {
    ChatDetailView(
      appState: appState,
      sessionViewModel: sessionViewModel,
      chatViewModel: chatViewModel,
      emptyStateTitle: "Let's go!",
      emptyStateMessage:
        "Start typing and Fawx will create a new thread, or pick one from the Threads sidebar."
    )
  }

  @ViewBuilder
  private var chatDetailContainer: some View {
    #if os(macOS)
      if shouldShowInspector {
        HSplitView {
          chatDetail
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .layoutPriority(1)

          CompactGitPanel(
            viewModel: gitViewModel,
            threadContext: selectedThreadContext,
            threadActivity: selectedThreadActivity,
            backgroundActivityNotice: selectedBackgroundActivityNotice,
            openSessionMemoryAction: presentSessionMemoryPanel,
            openFullViewAction: openGitView,
            dismissAction: hideInspector
          )
          .frame(
            minWidth: Layout.compactGitPanelMinWidth,
            idealWidth: Layout.compactGitPanelIdealWidth,
            maxWidth: Layout.compactGitPanelMaxWidth,
            maxHeight: .infinity
          )
        }
      } else {
        chatDetail
      }
    #else
      chatDetail
    #endif
  }

  private var chatShellContainer: some View {
    VStack(spacing: 0) {
      if let selectedThreadContext,
        selectedThreadContext.showsHeaderIdentity
      {
        ThreadContextHeader(context: selectedThreadContext)
      }

      chatDetailContainer
    }
  }

  private var statusBarView: some View {
    StatusBar(
      connectionStatus: appState.connectionStatus,
      permissionPreset: appState.permissionPresetName,
      modelName: appState.activeModel?.modelID ?? appState.lastHealth?.model,
      context: appState.currentContext,
      selectedSessionMessageCount: sessionViewModel.selectedSession?.messageCount ?? 0,
      canShowSessionMemory: sessionViewModel.selectedSession != nil,
      openSessionMemoryAction: presentSessionMemoryPanel
    )
    .fixedSize(horizontal: false, vertical: true)
    .frame(maxWidth: .infinity)
  }

  @ViewBuilder
  private var toastOverlay: some View {
    if let toast = appState.toast {
      ToastView(toast: toast)
        .padding(.top, FawxSpacing.paddingLG)
    }
  }

#if !os(macOS)
  @ToolbarContentBuilder
  private var gitPanelToolbar: some ToolbarContent {
    if isChatSectionSelected {
      ToolbarItem(placement: .primaryAction) {
        Button(action: toggleInspector) {
          Label(inspectorButtonTitle, systemImage: "sidebar.right")
        }
        .help(inspectorButtonHelp)
      }
    }
  }
#endif

  private var detailTitle: String {
    if let thread = sessionViewModel.selectedThread {
      return sessionViewModel.threadDisplayTitle(thread)
    }
    if let session = sessionViewModel.selectedSession {
      return session.displayTitle
    }
    return "New Thread"
  }

  private var selectedThreadContext: ThreadContextSnapshot? {
    sessionViewModel.selectedThreadContextSnapshot
  }

  private var branchMenuTitle: String {
    selectedThreadContext?.branchName?.nonEmpty
      ?? gitViewModel.repositoryTarget?.branchName?.nonEmpty
      ?? selectedThreadContext?.worktreeLabel?.nonEmpty
      ?? "No branch"
  }

  private var selectedThreadActivity: ThreadActivitySnapshot? {
    sessionViewModel.selectedThreadActivitySnapshot
  }

  private var selectedBackgroundActivityNotice: BackgroundThreadActivityNotice? {
    sessionViewModel.selectedBackgroundActivityNotice
  }

  private var selectedThreadContextRefreshID: String {
    selectedThreadContext?.refreshIdentity ?? "none"
  }

  private var sidebarSelection: SidebarSelection? {
    get {
      appState.sidebarSelection
        ?? sidebarSelectionRawValue.flatMap(SidebarSelection.init(rawValue:))
    }
    nonmutating set {
      sidebarSelectionRawValue = newValue?.rawValue
      appState.sidebarSelection = newValue
    }
  }

  private var sidebarActions: Sidebar.ActionHandlers {
    Sidebar.ActionHandlers(
      newThread: beginNewThread,
      newWorktreeThread: presentNewWorktreeThread,
      createWorktree: presentCreateWorktree,
      toggleWorkspaceExpansion: toggleWorkspaceExpansion,
      selectThread: selectThread,
      archiveThread: archiveThread,
      archiveWorktree: archiveWorktree,
      deleteWorktree: deleteWorktree,
      renameThread: renameThread,
      moveWorkspaces: moveWorkspaces,
      moveThreads: moveThreads,
      archiveWorkspaceThreads: archiveWorkspaceThreads,
      removeWorkspace: removeWorkspace,
      addWorkspace: addWorkspace,
      showSkills: showSkills,
      showGit: showGit,
      showFleet: showFleet,
      showExperiments: showExperiments,
      openSettings: showSettings
    )
  }

  private var gitPanelSelectionID: String? {
    sessionViewModel.selectedSessionID
  }

  private var inspectorButtonTitle: String {
    shouldShowInspector ? "Hide Inspector" : "Show Inspector"
  }

  private var inspectorButtonHelp: String {
    shouldShowInspector ? "Hide the thread inspector" : "Show the thread inspector"
  }

  private func loadInitialContent() async {
    if appState.sidebarSelection == nil {
      appState.sidebarSelection = sidebarSelectionRawValue.flatMap(SidebarSelection.init(rawValue:))
    }

    await sessionViewModel.refresh()
    await restoreSelectionAfterRefresh()
    syncThreadInspectorContext()
  }

  private func handleSelectedSessionChange(_ newValue: String?) {
    if let newValue {
      chatViewModel.prepareToDisplaySession(newValue)
      chatViewModel.scheduleLoadMessages(for: newValue, force: true)
    } else if isChatSectionSelected {
      chatViewModel.cancelScheduledLoad()
      chatViewModel.showEmptyState()
    }
  }

  private func handleSidebarSelectionChange(_ newValue: SidebarSelection?) {
    sidebarSelectionRawValue = newValue?.rawValue
  }

  private func beginNewThread(_ workspaceID: String? = nil) {
    Task {
      chatViewModel.cancelScheduledLoad()
      if let createdID = await sessionViewModel.createNewThread(in: workspaceID) {
        chatViewModel.prepareToDisplaySession(createdID)
        chatViewModel.scheduleLoadMessages(for: createdID, force: true)
      } else if isChatSectionSelected {
        chatViewModel.showEmptyState()
      }
    }
  }

  private func toggleWorkspaceExpansion(_ workspaceID: String) {
    _ = sessionViewModel.activateWorkspaceRow(workspaceID)
  }

  private func selectThread(_ threadID: String) {
    guard let thread = sessionViewModel.thread(threadID) else {
      return
    }

    chatViewModel.prepareToDisplaySession(thread.activeSessionID)
    sessionViewModel.selectThread(id: threadID)
  }

  private func showSkills() {
    switchToNonChatSection(.skills)
  }

  private func showGit() {
    GitPanelPresentation.hide(showGitPanel: $showInspectorPanel)
    bindDefaultGitRepositoryTargetIfNeeded()
    sidebarSelection = .git
  }

  private func showFleet() {
    switchToNonChatSection(.fleet)
  }

  private func showExperiments() {
    switchToNonChatSection(.experiments)
  }

  private func showSettings() {
    switchToNonChatSection(.settings)
  }

  private func restoreSelectionAfterRefresh() async {
    if let selectedSessionID = sessionViewModel.selectedSessionID {
      chatViewModel.prepareToDisplaySession(selectedSessionID)
      chatViewModel.scheduleLoadMessages(for: selectedSessionID, force: true)
    } else if isChatSectionSelected {
      chatViewModel.showEmptyState()
    }
  }

  private func presentSessionMemoryPanel() {
    presentedSessionMemory = sessionViewModel.selectedSession
  }

  private func archiveThread(_ threadID: String) {
    Task {
      let sessionID = sessionViewModel.thread(threadID)?.activeSessionID
      if let sessionID, chatViewModel.activeStreamSessionIDs.contains(sessionID) {
        chatViewModel.stopStreaming(sessionID: sessionID)
      }

      let didArchive = await sessionViewModel.archiveThread(id: threadID)
      if didArchive, let sessionID {
        chatViewModel.invalidateSession(sessionID)
        if let nextSessionID = sessionViewModel.selectedSessionID {
          chatViewModel.prepareToDisplaySession(nextSessionID)
          chatViewModel.scheduleLoadMessages(for: nextSessionID, force: true)
        } else if isChatSectionSelected {
          chatViewModel.showEmptyState()
        }
      }
    }
  }

  private func archiveWorkspaceThreads(_ workspaceID: String) {
    Task {
      let sessionIDs = (sessionViewModel.threadsByWorkspaceID[workspaceID] ?? []).map(
        \.activeSessionID)
      for sessionID in sessionIDs where chatViewModel.activeStreamSessionIDs.contains(sessionID) {
        chatViewModel.stopStreaming(sessionID: sessionID)
      }

      let archivedCount = await sessionViewModel.archiveThreads(in: workspaceID)
      if archivedCount > 0 {
        for sessionID in sessionIDs {
          chatViewModel.invalidateSession(sessionID)
        }
        if let nextSessionID = sessionViewModel.selectedSessionID {
          chatViewModel.prepareToDisplaySession(nextSessionID)
          chatViewModel.scheduleLoadMessages(for: nextSessionID, force: true)
        } else if isChatSectionSelected {
          chatViewModel.showEmptyState()
        }
      }
    }
  }

  private func addWorkspace() {
    #if os(macOS)
      let panel = NSOpenPanel()
      panel.canChooseFiles = false
      panel.canChooseDirectories = true
      panel.allowsMultipleSelection = false
      panel.directoryURL = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
      panel.prompt = "Open Workspace"
      panel.message = "Choose a directory to use as the active workspace."

      guard panel.runModal() == .OK, let url = panel.url else {
        return
      }

      Task {
        guard await sessionViewModel.openWorkspace(path: url.path) != nil else {
          return
        }
      }
    #endif
  }

  private func presentNewWorktreeThread(_ workspaceID: String) {
    worktreeDraft = WorktreeDraft(
      workspaceID: workspaceID,
      mode: .thread,
      title: sessionViewModel.workspace(workspaceID)?.name ?? ""
    )
  }

  private func presentCreateWorktree(_ workspaceID: String) {
    worktreeDraft = WorktreeDraft(
      workspaceID: workspaceID,
      mode: .worktree
    )
  }

  private func handleWorktreeDraftSubmission(
    workspaceID: String,
    mode: WorktreeDraftMode,
    title: String,
    branch: String,
    baseRef: String
  ) {
    let normalizedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
    let normalizedBaseRef = baseRef.trimmingCharacters(in: .whitespacesAndNewlines)

    Task {
      switch mode {
      case .thread:
        guard
          let created = await sessionViewModel.createWorktreeThread(
            in: workspaceID,
            title: normalizedTitle,
            branch: branch,
            baseRef: normalizedBaseRef.nonEmpty
          )
        else {
          return
        }

        sessionViewModel.selectThread(id: created.thread.id)
        chatViewModel.prepareToDisplaySession(created.thread.activeSessionID)
        chatViewModel.scheduleLoadMessages(for: created.thread.activeSessionID, force: true)
        worktreeDraft = nil
      case .worktree:
        guard
          await sessionViewModel.createPermanentWorktree(
            in: workspaceID,
            branch: branch,
            baseRef: normalizedBaseRef.nonEmpty
          ) != nil
        else {
          return
        }

        worktreeDraft = nil
      }
    }
  }

  private func renameThread(_ threadID: String, _ title: String) {
    sessionViewModel.renameThread(id: threadID, title: title)
  }

  private func moveWorkspaces(fromOffsets: IndexSet, toOffset: Int) {
    sessionViewModel.moveWorkspaces(fromOffsets: fromOffsets, toOffset: toOffset)
  }

  private func moveThreads(
    in workspaceID: String,
    fromOffsets: IndexSet,
    toOffset: Int
  ) {
    sessionViewModel.moveThreads(
      in: workspaceID,
      fromOffsets: fromOffsets,
      toOffset: toOffset
    )
  }

  private func removeWorkspace(_ workspaceID: String) {
    sessionViewModel.removeWorkspace(id: workspaceID)
  }

  private func archiveWorktree(_ worktreeID: String) {
    Task {
      let didArchive = await sessionViewModel.archiveWorktree(id: worktreeID)
      if didArchive > 0 {
        if let nextSessionID = sessionViewModel.selectedSessionID {
          chatViewModel.prepareToDisplaySession(nextSessionID)
          chatViewModel.scheduleLoadMessages(for: nextSessionID, force: true)
        } else if isChatSectionSelected {
          chatViewModel.showEmptyState()
        }
      }
    }
  }

  private func deleteWorktree(_ worktreeID: String) {
    Task {
      let deleted = await sessionViewModel.deleteWorktree(id: worktreeID)
      if deleted {
        if let nextSessionID = sessionViewModel.selectedSessionID {
          chatViewModel.prepareToDisplaySession(nextSessionID)
          chatViewModel.scheduleLoadMessages(for: nextSessionID, force: true)
        } else if isChatSectionSelected {
          chatViewModel.showEmptyState()
        }
      }
    }
  }

  private func switchToNonChatSection(_ selection: SidebarSelection) {
    hideActiveChatForUtilityNavigation()
    GitPanelPresentation.hide(showGitPanel: $showInspectorPanel)
    sidebarSelection = selection
  }

  private func hideActiveChatForUtilityNavigation() {
    // Non-chat sections are visual navigation only. Active streams are owned by
    // ChatViewModel and must continue while Settings/Skills/Fleet/etc. are open.
    chatViewModel.cancelScheduledLoad()
    sessionViewModel.select(nil)
    chatViewModel.showEmptyState()
  }

  private var isChatSectionSelected: Bool {
    switch sidebarSelection {
    case .thread, .workspace, .none:
      true
    case .skills, .fleet, .experiments, .git, .settings:
      false
    }
  }

  private var shouldShowInspector: Bool {
    showInspectorPanel && isChatSectionSelected
  }

  private func toggleInspector() {
    GitPanelPresentation.toggle(
      showGitPanel: $showInspectorPanel,
      selectedSessionID: gitPanelSelectionID,
      appState: appState,
      sessionViewModel: sessionViewModel,
      chatViewModel: chatViewModel
    )
  }

  private func toggleThreadsSidebar() {
    withAnimation(.easeInOut(duration: 0.16)) {
      showThreadsSidebar.toggle()
    }
  }

  private func hideInspector() {
    GitPanelPresentation.hide(showGitPanel: $showInspectorPanel)
  }

  private func openGitView() {
    hideInspector()
    bindDefaultGitRepositoryTargetIfNeeded()
    sidebarSelection = .git
  }

  private func syncThreadInspectorContext() {
    guard sidebarSelection != .git else {
      return
    }

    gitViewModel.bindThreadContext(selectedThreadContext)
  }

  private func bindGitRepositoryTarget(_ target: GitRepositoryTarget) {
    gitViewModel.bindRepositoryTarget(target)
  }

  private func bindDefaultGitRepositoryTargetIfNeeded() {
    guard gitViewModel.repositoryTarget == nil,
      let target = sessionViewModel.defaultGitRepositoryTarget
    else {
      return
    }

    bindGitRepositoryTarget(target)
  }

}

private struct ThreadContextHeader: View {
  let context: ThreadContextSnapshot

  var body: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
      contextControlStrip

      if let detailLine {
        Text(detailLine)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
          .lineLimit(2)
      }

      if metadataLine.isEmpty == false {
        Text(metadataLine)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
          .lineLimit(2)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.horizontal, FawxSpacing.paddingLG)
    .padding(.vertical, FawxSpacing.paddingMD)
    .fawxSurface(.section)
    .overlay(alignment: .bottom) {
      Divider()
    }
    .accessibilityIdentifier("threadContextHeader")
  }

  private var contextControlStrip: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
        if let workspaceLabel {
          ThreadContextPill(
            systemImage: "folder",
            title: "Workspace",
            value: workspaceLabel,
            tone: .fawxText
          )
          .accessibilityIdentifier("threadContextWorkspaceBadge")
        }

        if let worktreeLabel = context.worktreeLabel {
          ThreadContextPill(
            systemImage: "folder.badge.gearshape",
            title: "Worktree",
            value: worktreeLabel,
            tone: .fawxText
          )
        }

        if let isClean = context.isClean {
          ThreadContextPill(
            systemImage: isClean ? "checkmark.circle" : "exclamationmark.circle",
            title: "Git",
            value: isClean ? "Clean" : "Dirty",
            tone: isClean ? .fawxSuccess : .fawxWarning
          )
        }

        if let divergenceLabel = context.divergenceLabel {
          ThreadContextPill(
            systemImage: "arrow.left.arrow.right",
            title: "Sync",
            value: divergenceLabel,
            tone: .blue
          )
        }

        if let worktreeStatusLabel = context.worktreeStatusLabel {
          ThreadContextPill(
            systemImage: "tray.full",
            title: "Worktree",
            value: worktreeStatusLabel,
            tone: .fawxTextSecondary
          )
        }
      }
    }
  }

  private var workspaceLabel: String? {
    if let workspaceName = context.workspaceName, context.workspaceKind != .general {
      return workspaceName
    }
    return nil
  }

  private var detailLine: String? {
    var parts: [String] = []
    if let rootPath = context.rootPath {
      parts.append(rootPath)
    }
    if let baseRef = context.baseRef {
      parts.append("from \(baseRef)")
    }
    if let repositoryOrigin = context.repositoryOrigin {
      parts.append(repositoryOrigin)
    }

    return parts.isEmpty ? nil : parts.joined(separator: " · ")
  }

  private var metadataLine: String {
    [
      context.threadKind.rawValue.capitalized,
      context.messageCount == 1 ? "1 message" : "\(context.messageCount) messages",
    ]
    .filter { $0.isEmpty == false }
    .joined(separator: " · ")
  }
}

private struct ShellPanelMenuButton: View {
  let showsThreadsSidebar: Bool
  let canShowInspector: Bool
  let showsInspector: Bool
  let toggleThreadsSidebar: () -> Void
  let toggleInspector: () -> Void

  var body: some View {
    FawxDropdownMenu(minWidth: 180) {
      ShellDropdownLabel(title: "Panels", systemImage: "rectangle.split.3x1")
    } content: { dismiss in
      FawxDropdownActionRow(
        title: showsThreadsSidebar ? "Hide Threads" : "Show Threads",
        systemImage: showsThreadsSidebar ? "sidebar.left" : "sidebar.leading"
      ) {
        toggleThreadsSidebar()
        dismiss()
      }

      if canShowInspector {
        FawxDropdownActionRow(
          title: showsInspector ? "Hide Inspector" : "Show Inspector",
          systemImage: showsInspector ? "sidebar.right" : "sidebar.trailing"
        ) {
          toggleInspector()
          dismiss()
        }
      }
    }
    .help("Show or hide shell panels")
    .accessibilityIdentifier("shellPanelMenuButton")
  }
}

private struct BranchContextMenuButton: View {
  let title: String
  let context: ThreadContextSnapshot?
  let repositoryTargets: [GitRepositoryTarget]
  let selectedRepositoryTarget: GitRepositoryTarget?
  let selectRepositoryTarget: (GitRepositoryTarget) -> Void
  let openGitView: () -> Void

  var body: some View {
    FawxDropdownMenu(minWidth: 260) {
      ShellDropdownLabel(title: title, systemImage: "arrow.trianglehead.branch", titleMaxWidth: 180)
    } content: { dismiss in
      if let context {
        FawxDropdownSectionHeader(title: "Current Context")
        FawxDropdownInfoRow(
          title: context.branchName ?? context.worktreeLabel ?? "No branch",
          systemImage: "arrow.trianglehead.branch"
        )

        if let workspaceName = context.workspaceName {
          FawxDropdownInfoRow(title: workspaceName, systemImage: "folder")
        }

        if let rootPath = context.rootPath {
          FawxDropdownInfoRow(title: rootPath, systemImage: "location")
        }
      }

      if repositoryTargets.isEmpty == false {
        if context != nil {
          FawxDropdownDivider()
        }

        FawxDropdownSectionHeader(title: "Git Targets")
        ForEach(repositoryTargets) { target in
          FawxDropdownActionRow(
            title: target.branchMenuLabel,
            systemImage: target.systemImage,
            isSelected: target.id == selectedRepositoryTarget?.id
          ) {
            selectRepositoryTarget(target)
            dismiss()
          }
        }
      }

      if context != nil || repositoryTargets.isEmpty == false {
        FawxDropdownDivider()
      }

      FawxDropdownActionRow(
        title: "Open Git View",
        systemImage: "point.topleft.down.curvedto.point.bottomright.up"
      ) {
        openGitView()
        dismiss()
      }
    }
    .help("Choose branch or Git context")
    .accessibilityIdentifier("branchContextMenuButton")
  }
}

private struct ShellDropdownLabel: View {
  let title: String
  let systemImage: String
  var titleMaxWidth: CGFloat?

  @State private var isHovering = false

  var body: some View {
    HStack(alignment: .center, spacing: FawxSpacing.paddingXS) {
      Image(systemName: systemImage)
        .font(.system(size: 11, weight: .semibold))

      Text(title)
        .lineLimit(1)
        .truncationMode(.middle)
        .frame(maxWidth: titleMaxWidth, alignment: .leading)

      Image(systemName: "chevron.down")
        .font(.system(size: 9, weight: .semibold))
    }
    .font(FawxTypography.status)
    .foregroundStyle(isHovering ? Color.fawxText : Color.fawxTextSecondary)
    .padding(.horizontal, FawxSpacing.paddingXS)
    .padding(.vertical, FawxSpacing.paddingXS)
    .contentShape(Rectangle())
#if os(macOS)
    .onHover { isHovering = $0 }
#endif
  }
}

private extension GitRepositoryTarget {
  var branchMenuLabel: String {
    branchName?.nonEmpty ?? title
  }

  var systemImage: String {
    switch kind {
    case .workspace:
      "folder"
    case .worktree:
      "point.topleft.down.curvedto.point.bottomright.up"
    case .thread:
      "text.bubble"
    }
  }
}

private struct ThreadContextPill: View {
  let systemImage: String
  let title: String
  let value: String
  let tone: Color

  var body: some View {
    Label {
      HStack(alignment: .firstTextBaseline, spacing: 3) {
        Text(title)
          .foregroundStyle(Color.fawxTextSecondary)
        Text(value)
          .foregroundStyle(tone)
      }
    } icon: {
      Image(systemName: systemImage)
        .foregroundStyle(tone)
    }
    .font(FawxTypography.status)
    .lineLimit(1)
    .fixedSize(horizontal: true, vertical: false)
  }
}

private struct WorktreeLifecycleSheet: View {
  let workspaceName: String
  let mode: ContentView.WorktreeDraftMode
  let initialTitle: String
  let initialBranch: String
  let initialBaseRef: String
  let onCancel: () -> Void
  let onCreate: (String, String, String) -> Void

  @State private var title: String
  @State private var branch: String
  @State private var baseRef: String

  init(
    workspaceName: String,
    mode: ContentView.WorktreeDraftMode,
    initialTitle: String,
    initialBranch: String,
    initialBaseRef: String,
    onCancel: @escaping () -> Void,
    onCreate: @escaping (String, String, String) -> Void
  ) {
    self.workspaceName = workspaceName
    self.mode = mode
    self.initialTitle = initialTitle
    self.initialBranch = initialBranch
    self.initialBaseRef = initialBaseRef
    self.onCancel = onCancel
    self.onCreate = onCreate
    _title = State(initialValue: initialTitle)
    _branch = State(initialValue: initialBranch)
    _baseRef = State(initialValue: initialBaseRef)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
      Text(mode == .thread ? "New Isolated Worktree Thread" : "Create Permanent Worktree")
        .font(FawxTypography.sidebarTitle)
        .foregroundStyle(Color.fawxText)

      Text(workspaceName)
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)

      if mode == .thread {
        labeledField("Thread Title", text: $title, prompt: "Implement worktree lifecycle")
      }

      labeledField("Branch Name", text: $branch, prompt: "feature/worktree-lifecycle")
      labeledField("Base Ref", text: $baseRef, prompt: "Optional, e.g. origin/dev")

      HStack {
        Spacer(minLength: 0)

        Button("Cancel", action: onCancel)

        Button(mode == .thread ? "Create Thread" : "Create Worktree") {
          onCreate(title, branch, baseRef)
        }
        .buttonStyle(.borderedProminent)
        .tint(.fawxAccent)
        .disabled(isSubmissionInvalid)
      }
    }
    .padding(FawxSpacing.paddingLG)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(Color.fawxBackground)
  }

  private var isSubmissionInvalid: Bool {
    branch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      || (mode == .thread && title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
  }

  private func labeledField(_ label: String, text: Binding<String>, prompt: String) -> some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
      Text(label)
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)

      TextField(prompt, text: text)
        .textFieldStyle(.roundedBorder)
    }
  }
}
