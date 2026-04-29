import Observation
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
  import AppKit
#endif

private enum SidebarLayout {
  static let listLeadingInset: CGFloat = 0
  static let listTrailingInset = FawxSpacing.paddingXS
  static let rowLeadingPadding = FawxSpacing.paddingXS
  static let rowTrailingPadding = FawxSpacing.paddingXS
  static let childRowLeadingPadding = FawxSpacing.paddingSM
  static let rowContentSpacing = FawxSpacing.paddingXS
  static let headerLeadingPadding = FawxSpacing.paddingSM
  static let headerTrailingPadding = FawxSpacing.paddingSM
  static let footerLeadingPadding = FawxSpacing.paddingXS
  static let footerTrailingPadding = FawxSpacing.paddingXS
}

struct Sidebar: View {
  struct ActionHandlers {
    let newThread: (String?) -> Void
    let newWorktreeThread: (String) -> Void
    let createWorktree: (String) -> Void
    let toggleWorkspaceExpansion: (String) -> Void
    let selectThread: (String) -> Void
    let archiveThread: (String) -> Void
    let archiveWorktree: (String) -> Void
    let deleteWorktree: (String) -> Void
    let renameThread: (String, String) -> Void
    let moveWorkspaces: (IndexSet, Int) -> Void
    let moveThreads: (String, IndexSet, Int) -> Void
    let archiveWorkspaceThreads: (String) -> Void
    let removeWorkspace: (String) -> Void
    let addWorkspace: () -> Void
    let showSkills: () -> Void
    let showGit: () -> Void
    let showFleet: () -> Void
    let showExperiments: () -> Void
    let openSettings: () -> Void
  }

  private struct ThreadRenameDraft: Identifiable {
    let threadID: String
    let initialTitle: String

    var id: String { threadID }
  }

  private enum ConfirmationRequest: Identifiable {
    case archiveThread(threadID: String, title: String)
    case archiveWorkspaceThreads(workspaceID: String, workspaceName: String)
    case removeWorkspace(workspaceID: String, workspaceName: String)
    case archiveWorktree(worktreeID: String, title: String)
    case deleteWorktree(worktreeID: String, title: String)

    var id: String {
      switch self {
      case .archiveThread(let threadID, _):
        "archive-thread-\(threadID)"
      case .archiveWorkspaceThreads(let workspaceID, _):
        "archive-workspace-\(workspaceID)"
      case .removeWorkspace(let workspaceID, _):
        "remove-workspace-\(workspaceID)"
      case .archiveWorktree(let worktreeID, _):
        "archive-worktree-\(worktreeID)"
      case .deleteWorktree(let worktreeID, _):
        "delete-worktree-\(worktreeID)"
      }
    }
  }

  @Bindable var sessionViewModel: SessionViewModel
  let selection: SidebarSelection?
  let streamingSessionIDs: Set<String>
  let actions: ActionHandlers

  @State private var searchText = ""
  @State private var renameDraft: ThreadRenameDraft?
  @State private var pendingConfirmation: ConfirmationRequest?
  @State private var draggedItem: SidebarDragItem?

  var body: some View {
    VStack(spacing: 0) {
      header
      threadList
      footer
    }
    .background(Color.fawxBackground)
    .accessibilityIdentifier("threadsSidebar")
    .sheet(item: $renameDraft) { draft in
      ThreadRenameSheet(
        initialTitle: draft.initialTitle,
        onCancel: { renameDraft = nil },
        onSave: { updatedTitle in
          actions.renameThread(draft.threadID, updatedTitle)
          renameDraft = nil
        }
      )
      .frame(minWidth: 360, idealWidth: 420)
      .fawxOpaqueModalPresentation()
    }
    .alert(item: $pendingConfirmation) { confirmation in
      sidebarConfirmationAlert(for: confirmation)
    }
  }

  private var header: some View {
    VStack(spacing: FawxSpacing.paddingSM) {
      sidebarSearchField

      HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
        Text("Threads")
          .font(FawxTypography.sidebarTitle)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)

        Button(action: toggleAllWorkspaces) {
          SidebarIconGlyph(
            systemName: sessionViewModel.areAllWorkspacesExpanded
              ? "rectangle.compress.vertical"
              : "rectangle.expand.vertical"
          )
        }
        .buttonStyle(.plain)
        .help(
          sessionViewModel.areAllWorkspacesExpanded
            ? "Collapse all workspaces"
            : "Expand all workspaces"
        )
        .accessibilityIdentifier("threadsCollapseAllButton")

        FawxDropdownMenu(minWidth: 190) {
          SidebarIconGlyph(systemName: "line.3.horizontal.decrease.circle")
        } content: { dismiss in
          FawxDropdownSectionHeader(title: "Organize")
          FawxDropdownActionRow(
            title: "By project",
            isSelected: sessionViewModel.organizationMode == .byProject
          ) {
            sessionViewModel.organizationMode = .byProject
            dismiss()
          }
          FawxDropdownActionRow(
            title: "Chronological list",
            isSelected: sessionViewModel.organizationMode == .chronologicalList
          ) {
            sessionViewModel.organizationMode = .chronologicalList
            dismiss()
          }

          FawxDropdownDivider()
          FawxDropdownSectionHeader(title: "Sort by")
          FawxDropdownActionRow(
            title: "Created",
            isSelected: sessionViewModel.sortMode == .created
          ) {
            sessionViewModel.sortMode = .created
            dismiss()
          }
          FawxDropdownActionRow(
            title: "Updated",
            isSelected: sessionViewModel.sortMode == .updated
          ) {
            sessionViewModel.sortMode = .updated
            dismiss()
          }
        }
        .help("Organize threads")
        .accessibilityIdentifier("threadsOrganizeMenu")

        Button(action: actions.addWorkspace) {
          SidebarIconGlyph(systemName: "folder.badge.plus")
        }
        .buttonStyle(.plain)
        .help("Add workspace")
        .accessibilityIdentifier("addWorkspaceButton")
      }
    }
    .padding(.leading, SidebarLayout.headerLeadingPadding)
    .padding(.trailing, SidebarLayout.headerTrailingPadding)
    .padding(.top, FawxSpacing.paddingMD)
    .padding(.bottom, FawxSpacing.paddingSM)
    .fawxSurface(.rail)
  }

  private var threadList: some View {
    List {
      threadListContent
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .listStyle(.plain)
    .scrollContentBackground(.hidden)
    .fawxSurface(.rail)
  }

  private var sidebarSearchField: some View {
    HStack(spacing: FawxSpacing.paddingXS) {
      Image(systemName: "magnifyingglass")
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(Color.fawxTextSecondary)

      TextField("Search threads", text: $searchText)
        .textFieldStyle(.plain)
        .font(FawxTypography.sidebar)
        .foregroundStyle(Color.fawxText)
        .accessibilityIdentifier("threadSearchField")

      if searchText.isEmpty == false {
        Button {
          searchText = ""
        } label: {
          Image(systemName: "xmark.circle.fill")
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(Color.fawxTextSecondary.opacity(0.75))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Clear thread search")
      }
    }
    .padding(.vertical, FawxSpacing.paddingXS)
    .overlay(alignment: .bottom) {
      Rectangle()
        .fill(Color.fawxText.opacity(0.1))
        .frame(height: 1)
    }
  }

  @ViewBuilder
  private var threadListContent: some View {
    if sessionViewModel.isLoading && visibleWorkspaceGroups.isEmpty
      && visibleChronologicalEntries.isEmpty
    {
      sidebarPlaceholder(
        title: "Loading threads...",
        message: "Fetching workspaces and thread activity."
      )
    } else if let errorMessage = sessionViewModel.errorMessage, sessionViewModel.workspaces.isEmpty
    {
      sidebarPlaceholder(
        title: "Could not load threads",
        message: errorMessage,
        actionTitle: "Retry"
      ) {
        Task {
          await sessionViewModel.refresh()
        }
      }
    } else if sessionViewModel.organizationMode == .byProject {
      if visibleWorkspaceGroups.isEmpty {
        sidebarPlaceholder(
          title: emptySearchTitle,
          message: emptySearchMessage,
          actionTitle: trimmedSearchText.isEmpty ? "Start a thread" : "Clear Search"
        ) {
          if trimmedSearchText.isEmpty {
            actions.newThread(sessionViewModel.selectedWorkspaceID)
          } else {
            searchText = ""
          }
        }
      } else {
        workspaceSections
      }
    } else if visibleChronologicalEntries.isEmpty {
      sidebarPlaceholder(
        title: emptySearchTitle,
        message: emptySearchMessage,
        actionTitle: trimmedSearchText.isEmpty ? "Start a thread" : "Clear Search"
      ) {
        if trimmedSearchText.isEmpty {
          actions.newThread(sessionViewModel.selectedWorkspaceID)
        } else {
          searchText = ""
        }
      }
    } else {
      ForEach(visibleChronologicalEntries) { entry in
        threadRow(
          thread: entry.thread,
          workspace: entry.workspace,
          showsWorkspaceLabel: true
        )
      }
    }
  }

  @ViewBuilder
  private var workspaceSections: some View {
    let generalGroups = visibleWorkspaceGroups.filter { $0.workspace.isGeneral }
    let repositoryGroups = visibleWorkspaceGroups.filter { $0.workspace.isGeneral == false }
    let orderedWorkspaceIDs = repositoryGroups.map(\.workspace.id)

    ForEach(generalGroups) { group in
      workspaceSection(group, orderedWorkspaceIDs: [])
    }

    if canReorderProjectRows {
      ForEach(repositoryGroups) { group in
        workspaceSection(group, orderedWorkspaceIDs: orderedWorkspaceIDs)
      }
      .onMove(perform: actions.moveWorkspaces)
    } else {
      ForEach(repositoryGroups) { group in
        workspaceSection(group, orderedWorkspaceIDs: orderedWorkspaceIDs)
      }
    }
  }

  private func workspaceSection(
    _ group: WorkspaceThreadGroup,
    orderedWorkspaceIDs: [String]
  ) -> some View {
    Section {
      workspaceRow(group, orderedWorkspaceIDs: orderedWorkspaceIDs)

      if group.isExpanded {
        if group.showsStartThreadRow {
          startThreadRow(for: group.workspace)
        } else {
          workspaceThreadRows(group)
        }
      }
    }
  }

  @ViewBuilder
  private func workspaceThreadRows(_ group: WorkspaceThreadGroup) -> some View {
    if canReorderProjectRows {
      ForEach(group.threads) { thread in
        threadRow(
          thread: thread,
          workspace: group.workspace,
          showsWorkspaceLabel: false,
          reorderableThreadIDs: group.threads.map(\.id)
        )
      }
      .onMove { fromOffsets, toOffset in
        actions.moveThreads(group.workspace.id, fromOffsets, toOffset)
      }
    } else {
      ForEach(group.threads) { thread in
        threadRow(
          thread: thread,
          workspace: group.workspace,
          showsWorkspaceLabel: false
        )
      }
    }
  }

  @ViewBuilder
  private func workspaceRow(
    _ group: WorkspaceThreadGroup,
    orderedWorkspaceIDs: [String]
  ) -> some View {
    let row = WorkspaceSidebarRow(
      workspace: group.workspace,
      isExpanded: group.isExpanded,
      isActiveContext: group.isActiveContext,
      hasThreads: group.threads.isEmpty == false,
      toggleExpansion: { actions.toggleWorkspaceExpansion(group.workspace.id) },
      openInFinder: { openInFinder(path: group.workspace.path) },
      archiveThreads: { requestWorkspaceThreadArchive(group.workspace) },
      removeWorkspace: { requestWorkspaceRemoval(group.workspace) },
      createWorktree: { actions.createWorktree(group.workspace.id) },
      newWorktreeThread: { actions.newWorktreeThread(group.workspace.id) },
      newThread: { actions.newThread(group.workspace.id) }
    )
    .listRowInsets(
      EdgeInsets(
        top: 0,
        leading: SidebarLayout.listLeadingInset,
        bottom: 0,
        trailing: SidebarLayout.listTrailingInset
      )
    )
    .listRowSeparator(.hidden)
    .listRowBackground(Color.clear)

    if canReorderProjectRows {
      row
        .onDrag {
          draggedItem = .workspace(group.workspace.id)
          return NSItemProvider(object: group.workspace.id as NSString)
        }
        .onDrop(
          of: [UTType.plainText],
          delegate: WorkspaceSidebarDropDelegate(
            destinationWorkspaceID: group.workspace.id,
            orderedWorkspaceIDs: orderedWorkspaceIDs,
            draggedItem: $draggedItem,
            moveWorkspaces: actions.moveWorkspaces
          )
        )
    } else {
      row
    }
  }

  @ViewBuilder
  private func threadRow(
    thread: ThreadSummary,
    workspace: WorkspaceSummary,
    showsWorkspaceLabel: Bool,
    reorderableThreadIDs: [String]? = nil
  ) -> some View {
    let row = Button {
      actions.selectThread(thread.id)
    } label: {
      let activity = sessionViewModel.threadActivitySnapshot(for: thread)
      ThreadSidebarRow(
        title: sessionViewModel.threadDisplayTitle(thread),
        contextLabel: sessionViewModel.threadContextLabel(
          thread,
          includeWorkspace: showsWorkspaceLabel
        ),
        compactTimestamp: compactRelativeTimestampString(threadTimestamp(thread)),
        isSelected: sessionViewModel.selectedThreadID == thread.id,
        activity: activity,
        archive: { actions.archiveThread(thread.id) }
      )
      .padding(.leading, showsWorkspaceLabel ? 0 : SidebarLayout.childRowLeadingPadding)
    }
    .buttonStyle(.plain)
    .contextMenu {
      Button("Rename Thread") {
        renameDraft = ThreadRenameDraft(
          threadID: thread.id,
          initialTitle: sessionViewModel.threadDisplayTitle(thread)
        )
      }

      Button("Archive Thread") {
        requestThreadArchive(thread)
      }

      if let worktree = sessionViewModel.worktree(for: thread) {
        Divider()

        Button("Archive Worktree Lane") {
          requestWorktreeArchive(worktree)
        }

        Button("Remove Worktree", role: .destructive) {
          requestWorktreeRemoval(worktree)
        }
      }
    }
    .help(workspace.path.isEmpty ? sessionViewModel.threadDisplayTitle(thread) : workspace.path)
    .accessibilityIdentifier("threadRow_\(thread.id)")
    .listRowInsets(
      EdgeInsets(
        top: 0,
        leading: SidebarLayout.listLeadingInset,
        bottom: 0,
        trailing: SidebarLayout.listTrailingInset
      )
    )
    .listRowSeparator(.hidden)
    .listRowBackground(Color.clear)

    if let reorderableThreadIDs {
      row
        .onDrag {
          draggedItem = .thread(
            workspaceID: workspace.id,
            threadID: thread.id
          )
          return NSItemProvider(object: thread.id as NSString)
        }
        .onDrop(
          of: [UTType.plainText],
          delegate: ThreadSidebarDropDelegate(
            workspaceID: workspace.id,
            destinationThreadID: thread.id,
            orderedThreadIDs: reorderableThreadIDs,
            draggedItem: $draggedItem,
            moveThreads: actions.moveThreads
          )
        )
    } else {
      row
    }
  }

  private func startThreadRow(for workspace: WorkspaceSummary) -> some View {
    Button {
      actions.newThread(workspace.id)
    } label: {
      HStack(spacing: FawxSpacing.paddingSM) {
        Image(systemName: "plus.circle")
          .foregroundStyle(Color.fawxAccent)

        Text("Start a thread")
          .font(FawxTypography.sidebar)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.vertical, FawxSpacing.paddingSM)
      .padding(.leading, SidebarLayout.rowLeadingPadding)
      .padding(.trailing, SidebarLayout.rowTrailingPadding)
      .padding(.leading, SidebarLayout.childRowLeadingPadding)
      .background(
        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
          .fill(Color.fawxAccentSubtle.opacity(0.55))
      )
    }
    .buttonStyle(.plain)
    .accessibilityIdentifier("startThreadRow_\(workspace.id)")
    .listRowInsets(
      EdgeInsets(
        top: 0,
        leading: SidebarLayout.listLeadingInset,
        bottom: 0,
        trailing: SidebarLayout.listTrailingInset
      )
    )
    .listRowSeparator(.hidden)
    .listRowBackground(Color.clear)
  }

  private var footer: some View {
    VStack(spacing: 0) {
      sidebarButton(
        title: "Skills",
        systemImage: "puzzlepiece.extension",
        isSelected: selection == .skills,
        action: actions.showSkills
      )
      sidebarButton(
        title: "Git",
        systemImage: "arrow.trianglehead.branch",
        isSelected: selection == .git,
        action: actions.showGit
      )
      sidebarButton(
        title: "Fleet",
        systemImage: "point.3.connected.trianglepath.dotted",
        isSelected: selection == .fleet,
        action: actions.showFleet
      )
      sidebarButton(
        title: "Experiments",
        systemImage: "waveform.path.ecg.rectangle",
        isSelected: selection == .experiments,
        action: actions.showExperiments
      )
      sidebarButton(
        title: "Settings",
        systemImage: "gearshape",
        isSelected: selection == .settings,
        action: actions.openSettings
      )
    }
    .frame(maxWidth: .infinity)
    .padding(.leading, SidebarLayout.footerLeadingPadding)
    .padding(.trailing, SidebarLayout.footerTrailingPadding)
    .padding(.vertical, FawxSpacing.paddingSM)
    .fawxSurface(.rail)
  }

  private var visibleWorkspaceGroups: [WorkspaceThreadGroup] {
    sessionViewModel.workspaceThreadGroups(matching: searchText)
  }

  private var visibleChronologicalEntries: [ChronologicalThreadEntry] {
    sessionViewModel.chronologicalThreadEntries(matching: searchText)
  }

  private var trimmedSearchText: String {
    searchText.trimmingCharacters(in: .whitespacesAndNewlines)
  }

  private var canReorderProjectRows: Bool {
    trimmedSearchText.isEmpty && sessionViewModel.organizationMode == .byProject
  }

  private var emptySearchTitle: String {
    if trimmedSearchText.isEmpty {
      return "No threads yet"
    }

    return "No threads matching \"\(trimmedSearchText)\""
  }

  private var emptySearchMessage: String {
    if trimmedSearchText.isEmpty {
      return "Create a new thread or add a workspace to get started."
    }

    return "Try a different search term."
  }

  private func toggleAllWorkspaces() {
    if sessionViewModel.areAllWorkspacesExpanded {
      sessionViewModel.collapseAllWorkspaces()
    } else {
      sessionViewModel.expandAllWorkspaces()
    }
  }

  private func threadTimestamp(_ thread: ThreadSummary) -> Int {
    switch sessionViewModel.sortMode {
    case .created:
      return thread.createdAt
    case .updated:
      return thread.updatedAt
    }
  }

  private func sidebarButton(
    title: String,
    systemImage: String,
    isSelected: Bool,
    action: @escaping () -> Void
  ) -> some View {
    Button(action: action) {
      HStack(spacing: FawxSpacing.paddingSM) {
        Image(systemName: systemImage)
          .frame(width: 16, alignment: .center)

        Text(title)

        Spacer(minLength: 0)
      }
      .font(FawxTypography.sidebar)
      .foregroundStyle(Color.fawxText)
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.leading, SidebarLayout.rowLeadingPadding)
      .padding(.trailing, SidebarLayout.rowTrailingPadding)
      .padding(.vertical, FawxSpacing.paddingSM)
      .fawxRowChrome(isSelected: isSelected, selectionStyle: .accentOnly)
      .overlay(alignment: .leading) {
        RoundedRectangle(cornerRadius: 1.5)
          .fill(isSelected ? Color.fawxAccent : .clear)
          .frame(width: 3)
          .padding(.vertical, FawxSpacing.paddingXS)
      }
      .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
    .buttonStyle(.plain)
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func sidebarPlaceholder(
    title: String,
    message: String,
    actionTitle: String? = nil,
    action: (() -> Void)? = nil
  ) -> some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
      Text(title)
        .font(FawxTypography.sidebarTitle)
        .foregroundStyle(Color.fawxText)

      Text(message)
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)

      if let actionTitle, let action {
        Button(actionTitle, action: action)
          .buttonStyle(.borderedProminent)
          .tint(.fawxAccent)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.vertical, FawxSpacing.paddingMD)
    .listRowBackground(Color.clear)
  }

  private func openInFinder(path: String) {
    #if os(macOS)
      guard path.isEmpty == false else {
        return
      }

      NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    #endif
  }

  private func requestThreadArchive(_ thread: ThreadSummary) {
    pendingConfirmation = .archiveThread(
      threadID: thread.id,
      title: sessionViewModel.threadDisplayTitle(thread)
    )
  }

  private func requestWorkspaceThreadArchive(_ workspace: WorkspaceSummary) {
    pendingConfirmation = .archiveWorkspaceThreads(
      workspaceID: workspace.id,
      workspaceName: workspace.name
    )
  }

  private func requestWorkspaceRemoval(_ workspace: WorkspaceSummary) {
    pendingConfirmation = .removeWorkspace(
      workspaceID: workspace.id,
      workspaceName: workspace.name
    )
  }

  private func requestWorktreeArchive(_ worktree: WorktreeSummary) {
    pendingConfirmation = .archiveWorktree(
      worktreeID: worktree.id,
      title: worktree.label.nonEmpty ?? worktree.branch
    )
  }

  private func requestWorktreeRemoval(_ worktree: WorktreeSummary) {
    pendingConfirmation = .deleteWorktree(
      worktreeID: worktree.id,
      title: worktree.label.nonEmpty ?? worktree.branch
    )
  }

  private func sidebarConfirmationAlert(for confirmation: ConfirmationRequest) -> Alert {
    switch confirmation {
    case .archiveThread(_, let title):
      return Alert(
        title: Text("Archive Thread?"),
        message: Text(
          "\"\(title)\" will move out of the default thread list. You can restore it from Settings."
        ),
        primaryButton: .destructive(Text("Archive")) {
          performSidebarConfirmation(confirmation)
        },
        secondaryButton: .cancel()
      )
    case .archiveWorkspaceThreads(_, let workspaceName):
      return Alert(
        title: Text("Archive All Threads?"),
        message: Text(
          "Archive every visible thread in \(workspaceName). You can restore them later from Settings."
        ),
        primaryButton: .destructive(Text("Archive")) {
          performSidebarConfirmation(confirmation)
        },
        secondaryButton: .cancel()
      )
    case .removeWorkspace(_, let workspaceName):
      return Alert(
        title: Text("Remove Workspace?"),
        message: Text(
          "Hide \(workspaceName) from the thread shell. Its threads stay on disk and can be restored by re-adding the workspace."
        ),
        primaryButton: .destructive(Text("Remove")) {
          performSidebarConfirmation(confirmation)
        },
        secondaryButton: .cancel()
      )
    case .archiveWorktree(_, let title):
      return Alert(
        title: Text("Archive Worktree Lane?"),
        message: Text(
          "Archive the active thread lane for \(title) without deleting the worktree from disk."),
        primaryButton: .destructive(Text("Archive")) {
          performSidebarConfirmation(confirmation)
        },
        secondaryButton: .cancel()
      )
    case .deleteWorktree(_, let title):
      return Alert(
        title: Text("Remove Worktree?"),
        message: Text(
          "Delete the git worktree for \(title) and archive any active thread lane attached to it."),
        primaryButton: .destructive(Text("Remove")) {
          performSidebarConfirmation(confirmation)
        },
        secondaryButton: .cancel()
      )
    }
  }

  private func performSidebarConfirmation(_ confirmation: ConfirmationRequest) {
    switch confirmation {
    case .archiveThread(let threadID, _):
      actions.archiveThread(threadID)
    case .archiveWorkspaceThreads(let workspaceID, _):
      actions.archiveWorkspaceThreads(workspaceID)
    case .removeWorkspace(let workspaceID, _):
      actions.removeWorkspace(workspaceID)
    case .archiveWorktree(let worktreeID, _):
      actions.archiveWorktree(worktreeID)
    case .deleteWorktree(let worktreeID, _):
      actions.deleteWorktree(worktreeID)
    }
  }

}

private struct WorkspaceSidebarRow: View {
  let workspace: WorkspaceSummary
  let isExpanded: Bool
  let isActiveContext: Bool
  let hasThreads: Bool
  let toggleExpansion: () -> Void
  let openInFinder: () -> Void
  let archiveThreads: () -> Void
  let removeWorkspace: () -> Void
  let createWorktree: () -> Void
  let newWorktreeThread: () -> Void
  let newThread: () -> Void

  @State private var isHovering = false

  var body: some View {
    Button(action: toggleExpansion) {
      HStack(spacing: SidebarLayout.rowContentSpacing) {
        Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
          .font(.system(size: 10, weight: .semibold))
          .foregroundStyle(Color.fawxTextSecondary)
          .frame(width: 10)

        Image(systemName: "folder")
          .foregroundStyle(isActiveContext ? Color.fawxAccent : Color.fawxTextSecondary)

        Text(workspace.name)
          .font(FawxTypography.sidebar)
          .foregroundStyle(isActiveContext ? Color.fawxText : Color.fawxTextSecondary.opacity(0.95))
          .lineLimit(1)
          .layoutPriority(1)

        Spacer(minLength: 0)

        if isHovering {
          HStack(spacing: FawxSpacing.paddingXS) {
            if workspace.path.isEmpty == false {
              Button(action: openInFinder) {
                SidebarIconGlyph(systemName: "location")
              }
              .buttonStyle(.plain)
              .help(workspace.path)
            }

            FawxDropdownMenu(minWidth: 220) {
              SidebarOverflowGlyph()
            } content: { dismiss in
              workspaceDropdownContent(dismiss: dismiss)
            }
            .tint(Color.fawxTextSecondary)
            .foregroundStyle(Color.fawxTextSecondary)

            Button(action: newThread) {
              SidebarIconGlyph(systemName: "square.and.pencil")
            }
            .buttonStyle(.plain)
            .help("Start a thread in \(workspace.name)")
          }
        }
      }
      .font(FawxTypography.sidebar)
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.vertical, FawxSpacing.paddingSM)
      .padding(.leading, SidebarLayout.rowLeadingPadding)
      .padding(.trailing, SidebarLayout.rowTrailingPadding)
      .fawxRowChrome(
        isSelected: isActiveContext,
        isHovering: isHovering,
        selectionStyle: .accentOnly
      )
      .overlay(alignment: .leading) {
        RoundedRectangle(cornerRadius: 1.5)
          .fill(isActiveContext ? Color.fawxAccent.opacity(0.8) : .clear)
          .frame(width: 2)
          .padding(.vertical, FawxSpacing.paddingXS)
      }
      .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
    .buttonStyle(.plain)
    .accessibilityIdentifier("workspaceRow_\(workspace.id)")
    .help(workspace.path.isEmpty ? "Workspace" : workspace.path)
    .contextMenu {
      workspaceMenuContent
    }
    #if os(macOS)
      .onHover { isHovering = $0 }
    #endif
  }

  @ViewBuilder
  private var workspaceMenuContent: some View {
    if workspace.path.isEmpty == false {
      Button("Open in Finder") {
        openInFinder()
      }
    }

    if workspace.isGeneral == false {
      Button("New Isolated Worktree Thread…") {
        newWorktreeThread()
      }

      Button("Create Permanent Worktree…") {
        createWorktree()
      }
    }

    if hasThreads {
      Button("Archive Threads") {
        archiveThreads()
      }
    }

    if workspace.isGeneral == false {
      Button("Remove", role: .destructive) {
        removeWorkspace()
      }
    }
  }

  @ViewBuilder
  private func workspaceDropdownContent(dismiss: @escaping () -> Void) -> some View {
    if workspace.path.isEmpty == false {
      FawxDropdownActionRow(title: "Open in Finder", systemImage: "location") {
        openInFinder()
        dismiss()
      }
    }

    if workspace.isGeneral == false {
      FawxDropdownActionRow(title: "New Isolated Worktree Thread…", systemImage: "text.bubble") {
        newWorktreeThread()
        dismiss()
      }

      FawxDropdownActionRow(
        title: "Create Permanent Worktree…",
        systemImage: "folder.badge.gearshape"
      ) {
        createWorktree()
        dismiss()
      }
    }

    if hasThreads {
      FawxDropdownActionRow(title: "Archive Threads", systemImage: "archivebox") {
        archiveThreads()
        dismiss()
      }
    }

    if workspace.isGeneral == false {
      FawxDropdownDivider()
      FawxDropdownActionRow(
        title: "Remove",
        systemImage: "trash",
        role: .destructive
      ) {
        removeWorkspace()
        dismiss()
      }
    }
  }

}

private struct ThreadSidebarRow: View {
  private enum Layout {
    static let trailingAccessoryMinWidth: CGFloat = 26
  }

  let title: String
  let contextLabel: String?
  let compactTimestamp: String
  let isSelected: Bool
  let activity: ThreadActivitySnapshot
  let archive: () -> Void

  @State private var isHovering = false

  var body: some View {
    HStack(alignment: .top, spacing: SidebarLayout.rowContentSpacing) {
      statusIndicator
        .padding(.top, contextLabel == nil ? 3 : 4)

      VStack(alignment: .leading, spacing: contextLabel == nil ? 0 : 3) {
        Text(title)
          .font(FawxTypography.sidebar)
          .foregroundStyle(Color.fawxText)
          .lineLimit(1)

        if let contextLabel {
          Text(contextLabel)
            .font(FawxTypography.status)
            .foregroundStyle(Color.fawxTextSecondary)
            .lineLimit(1)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .layoutPriority(1)

      trailingAccessory
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.vertical, FawxSpacing.paddingSM)
    .padding(.leading, SidebarLayout.rowLeadingPadding)
    .padding(.trailing, SidebarLayout.rowTrailingPadding)
    .fawxRowChrome(
      isSelected: isSelected,
      isHovering: isHovering,
      selectionStyle: .accentOnly
    )
    .overlay(alignment: .leading) {
      RoundedRectangle(cornerRadius: 1.5)
        .fill(isSelected ? Color.fawxAccent : .clear)
        .frame(width: 3)
        .padding(.vertical, FawxSpacing.paddingXS)
    }
    .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    #if os(macOS)
      .onHover { isHovering = $0 }
    #endif
  }

  @ViewBuilder
  private var statusIndicator: some View {
    if activity.isRunning {
      ProgressView()
        .controlSize(.mini)
        .scaleEffect(0.65)
        .frame(width: 10, height: 10)
    } else if activity.showsUnreadIndicator {
      Circle()
        .fill(Color.fawxAccent)
        .frame(width: 8, height: 8)
    } else {
      Circle()
        .stroke(Color.fawxTextSecondary.opacity(0.45), lineWidth: 1.5)
        .frame(width: 8, height: 8)
    }
  }

  private var trailingAccessory: some View {
    Group {
      if isHovering {
        Button(action: archive) {
          SidebarIconGlyph(systemName: "archivebox")
        }
        .buttonStyle(.plain)
        .help("Archive thread")
      } else {
        Text(compactTimestamp)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
          .monospacedDigit()
          .lineLimit(1)
      }
    }
    .fixedSize(horizontal: true, vertical: false)
    .frame(minWidth: Layout.trailingAccessoryMinWidth, alignment: .trailing)
  }

}

private struct SidebarIconGlyph: View {
  let systemName: String

  var body: some View {
    Image(systemName: systemName)
      .font(.system(size: 12, weight: .medium))
      .foregroundStyle(Color.fawxTextSecondary)
      .frame(width: 16, height: 16, alignment: .center)
  }
}

private struct SidebarOverflowGlyph: View {
  var body: some View {
    SidebarIconGlyph(systemName: "ellipsis")
      .symbolRenderingMode(.monochrome)
      .foregroundStyle(Color.fawxTextSecondary)
      .contentShape(Rectangle())
  }
}
