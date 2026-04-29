import Observation
import SwiftUI

private enum ThreadInspectorSection: String, CaseIterable, Identifiable {
  case context
  case git

  var id: String { rawValue }

  var title: String {
    switch self {
    case .context:
      return "Context"
    case .git:
      return "Git"
    }
  }
}

struct CompactGitPanel: View {
  @Bindable var viewModel: GitViewModel
  let threadContext: ThreadContextSnapshot?
  let threadActivity: ThreadActivitySnapshot?
  let backgroundActivityNotice: BackgroundThreadActivityNotice?
  let openSessionMemoryAction: () -> Void
  let openFullViewAction: () -> Void
  let dismissAction: () -> Void
  @State private var selectedSection: ThreadInspectorSection = .context

  var snapshot: CompactGitPanelSnapshot {
    CompactGitPanelSnapshot(viewModel: viewModel)
  }

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
        ThreadInspectorHeaderCard(
          threadContext: threadContext,
          dismissAction: dismissAction
        )
        inspectorSectionPicker
        contentSection
      }
      .padding(FawxSpacing.paddingLG)
    }
    .background(Color.fawxBackground)
    .task(id: gitRefreshTaskID) {
      guard selectedSection == .git else {
        viewModel.cancelRefresh()
        return
      }
      await viewModel.refresh()
    }
    .onChange(of: selectedSection) { _, newValue in
      if newValue != .git {
        viewModel.cancelRefresh()
      }
    }
    .onDisappear {
      viewModel.cancelRefresh()
    }
    .alert(item: pendingConfirmationBinding, content: confirmationAlert)
  }

  private var gitRefreshTaskID: String {
    "\(selectedSection.rawValue)-\(viewModel.refreshTaskID)"
  }

  private var inspectorSectionPicker: some View {
    Picker("Inspector Section", selection: $selectedSection) {
      ForEach(ThreadInspectorSection.allCases) { section in
        Text(section.title).tag(section)
      }
    }
    .pickerStyle(.segmented)
    .tint(.fawxAccent)
  }

  private var pendingConfirmationBinding: Binding<GitConfirmationRequest?> {
    Binding(
      get: { viewModel.pendingConfirmation },
      set: { newValue in
        if newValue == nil {
          viewModel.cancelPendingConfirmation()
        }
      }
    )
  }

  @ViewBuilder
  private var contentSection: some View {
    switch selectedSection {
    case .context:
      if let threadContext {
        ThreadContextSummaryCard(context: threadContext)
        ThreadActivityCard(
          activity: threadActivity,
          backgroundActivityNotice: backgroundActivityNotice
        )
        ThreadSupportCard(
          context: threadContext,
          openSessionMemoryAction: openSessionMemoryAction,
          openFullViewAction: openFullViewAction
        )
      } else {
        GitCompactPlaceholderView(
          title: "No active thread",
          message: "Select a thread to inspect its workspace, Git, and memory context."
        )
      }
    case .git:
      GitSummaryCard(snapshot: snapshot, viewModel: viewModel)
      if threadContext?.hasRepositoryContext == true {
        GitQuickActionsCard(
          viewModel: viewModel,
          snapshot: snapshot,
          openFullViewAction: openFullViewAction
        )
      }
      gitContentSection
    }
  }

  @ViewBuilder
  private var gitContentSection: some View {
    if let threadContext, threadContext.hasRepositoryContext == false {
      GitCompactPlaceholderView(
        title: "No repository bound",
        message: gitUnavailableMessage(for: threadContext)
      )
    } else {
      switch snapshot.primaryState {
      case .error(let message):
        GitCompactPlaceholderView(
          title: "Could not load Git status",
          message: message,
          actionTitle: "Try Again"
        ) {
          Task {
            await viewModel.refresh()
          }
        }
      case .ready:
        if let status = viewModel.status {
          GitFilesCard(status: status, viewModel: viewModel)
          GitDiffCard(snapshot: snapshot)
        }
      case .loading:
        ProgressView("Loading Git status...")
          .frame(maxWidth: .infinity, minHeight: 180)
      case .empty:
        GitCompactPlaceholderView(
          title: "No repository data",
          message: "Git information appears when the selected thread is bound to a repository."
        )
      }
    }
  }

  private func gitUnavailableMessage(for context: ThreadContextSnapshot) -> String {
    if context.binding == .general {
      return "This thread is not attached to a workspace or branch yet."
    }

    if let workspaceName = context.workspaceName {
      return
        "\(workspaceName) is bound as thread context, but it is not exposing repository metadata."
    }

    return "This thread does not currently expose repository metadata."
  }

  private func confirmationAlert(for request: GitConfirmationRequest) -> Alert {
    Alert(
      title: Text(request.title),
      message: Text(request.message),
      primaryButton: .default(Text(request.confirmButtonTitle)) {
        Task {
          await viewModel.confirmPendingConfirmation()
        }
      },
      secondaryButton: .cancel {
        viewModel.cancelPendingConfirmation()
      }
    )
  }
}

struct CompactGitPanelSnapshot: Equatable {
  enum PrimaryState: Equatable {
    case loading
    case error(message: String)
    case empty
    case ready
  }

  enum StatusTone: Equatable {
    case neutral
    case success
    case warning

    var color: Color {
      switch self {
      case .neutral:
        return .fawxTextSecondary
      case .success:
        return .fawxSuccess
      case .warning:
        return .fawxWarning
      }
    }
  }

  private struct StatusDetails {
    let branchTitle: String
    let statusBadgeLabel: String
    let statusTone: StatusTone
    let statusSummary: String
    let changedFileCount: Int
  }

  private struct DiffPreview {
    let title: String
    let lines: [String]
    let lineCountLabel: String
    let isTruncated: Bool
  }

  private static let diffPreviewLineLimit = 80

  let primaryState: PrimaryState
  let branchTitle: String
  let statusBadgeLabel: String
  let statusTone: StatusTone
  let statusSummary: String
  let changedFileCount: Int
  let stagedFileCount: Int
  let unstagedFileCount: Int
  let stagedFileSummary: String
  let canStageAll: Bool
  let canPush: Bool
  let canCommit: Bool
  let diffTitle: String
  let previewLines: [String]
  let previewLineCountLabel: String
  let isDiffTruncated: Bool

  @MainActor
  init(viewModel: GitViewModel) {
    primaryState = Self.primaryState(
      status: viewModel.status,
      isLoading: viewModel.isLoading,
      errorMessage: viewModel.errorMessage
    )

    let statusDetails = Self.statusDetails(from: viewModel)
    branchTitle = statusDetails.branchTitle
    statusBadgeLabel = statusDetails.statusBadgeLabel
    statusTone = statusDetails.statusTone
    statusSummary = statusDetails.statusSummary
    changedFileCount = statusDetails.changedFileCount

    stagedFileCount = viewModel.stagedFiles.count
    unstagedFileCount = viewModel.unstagedFiles.count
    stagedFileSummary = Self.stagedFileSummary(for: stagedFileCount)
    canStageAll = !viewModel.unstagedFiles.isEmpty && !viewModel.isPerformingAction
    canPush = !viewModel.isPerformingAction
    canCommit = viewModel.canCommit && !viewModel.isPerformingAction
    let diffPreview = Self.diffPreview(
      selectedFilePath: viewModel.selectedFilePath,
      displayedDiff: viewModel.displayedDiff
    )
    diffTitle = diffPreview.title
    previewLines = diffPreview.lines
    previewLineCountLabel = diffPreview.lineCountLabel
    isDiffTruncated = diffPreview.isTruncated
  }

  private static func primaryState(
    status: GitStatusResponse?,
    isLoading: Bool,
    errorMessage: String?
  ) -> PrimaryState {
    if let errorMessage, status == nil {
      return .error(message: errorMessage)
    }
    if status != nil {
      return .ready
    }
    if isLoading {
      return .loading
    }
    return .empty
  }

  private static func statusSummary(for status: GitStatusResponse) -> String {
    if status.clean {
      return "Working tree is clean."
    }

    let fileLabel = status.files.count == 1 ? "file" : "files"
    return "\(status.files.count) changed \(fileLabel)"
  }

  private static func stagedFileSummary(for count: Int) -> String {
    guard count > 0 else {
      return "Stage files before committing."
    }

    let fileLabel = count == 1 ? "file" : "files"
    return "\(count) staged \(fileLabel)"
  }

  @MainActor
  private static func statusDetails(from viewModel: GitViewModel) -> StatusDetails {
    if let status = viewModel.status {
      return StatusDetails(
        branchTitle: viewModel.branchTitle,
        statusBadgeLabel: status.clean ? "Clean" : "Dirty",
        statusTone: status.clean ? .success : .warning,
        statusSummary: statusSummary(for: status),
        changedFileCount: status.files.count
      )
    }

    if let threadContext = viewModel.threadContext, threadContext.hasRepositoryContext == false {
      return StatusDetails(
        branchTitle: threadContext.binding == .general ? "Git" : viewModel.branchTitle,
        statusBadgeLabel: "No Git",
        statusTone: .neutral,
        statusSummary: threadContext.binding == .general
          ? "This thread is not attached to a repository yet."
          : "This thread is attached to a workspace without repository metadata.",
        changedFileCount: 0
      )
    }

    return StatusDetails(
      branchTitle: viewModel.branchTitle,
      statusBadgeLabel: viewModel.isLoading ? "Loading" : "Unknown",
      statusTone: .neutral,
      statusSummary: viewModel.isLoading
        ? "Loading working tree status..."
        : "Git status follows the active thread here.",
      changedFileCount: 0
    )
  }

  private static func diffPreview(
    selectedFilePath: String?,
    displayedDiff: String
  ) -> DiffPreview {
    let diffLines = diffLines(from: displayedDiff)
    let previewLines = Array(diffLines.prefix(diffPreviewLineLimit))
    return DiffPreview(
      title: selectedFilePath ?? "Diff Preview",
      lines: previewLines,
      lineCountLabel: previewLineCountLabel(for: diffLines.count),
      isTruncated: diffLines.count > previewLines.count
    )
  }

  private static func diffLines(from displayedDiff: String) -> [String] {
    guard displayedDiff.isEmpty == false else {
      return []
    }

    return
      displayedDiff
      .split(separator: "\n", omittingEmptySubsequences: false)
      .map(String.init)
  }

  private static func previewLineCountLabel(for totalLineCount: Int) -> String {
    guard totalLineCount > 0 else {
      return "0"
    }

    return "\(min(totalLineCount, diffPreviewLineLimit))/\(totalLineCount)"
  }
}

struct ThreadActivityCardSnapshot: Equatable {
  struct InfoRow: Equatable {
    let label: String
    let value: String
  }

  let summaryText: String
  let detailText: String?
  let infoRows: [InfoRow]

  init(
    activity: ThreadActivitySnapshot?,
    backgroundActivityNotice: BackgroundThreadActivityNotice?
  ) {
    if let backgroundActivityNotice {
      summaryText = backgroundActivityNotice.message
      detailText = backgroundActivityNotice.detail
      infoRows = []
      return
    }

    if let activity, activity.isRunning {
      summaryText = "The selected thread is actively working."
    } else {
      summaryText =
        "The selected thread is idle. Background work only appears here when another thread is active."
    }

    detailText = activity?.summaryLine

    guard let activity else {
      infoRows = []
      return
    }

    var rows: [InfoRow] = []
    if let badgeLabel = activity.badgeLabel {
      rows.append(InfoRow(label: "Status", value: badgeLabel))
    }
    if activity.runningToolCallCount > 0 {
      rows.append(
        InfoRow(
          label: "Active tools",
          value: activity.runningToolCallCount == 1
            ? "1 tool running"
            : "\(activity.runningToolCallCount) tools running"
        )
      )
    } else if activity.liveToolCallCount > 0 {
      rows.append(
        InfoRow(
          label: "Live tools",
          value: activity.liveToolCallCount == 1
            ? "1 tool in this run"
            : "\(activity.liveToolCallCount) tools in this run"
        )
      )
    }
    if let progressMessage = activity.progressMessage {
      rows.append(InfoRow(label: "Detail", value: progressMessage))
    }
    if let startedAt = activity.startedAt {
      rows.append(
        InfoRow(
          label: "Started",
          value: startedAt.formatted(date: .omitted, time: .shortened)
        )
      )
    }
    infoRows = rows
  }
}

private struct ThreadInspectorHeaderCard: View {
  let threadContext: ThreadContextSnapshot?
  let dismissAction: () -> Void

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
        Image(systemName: "sidebar.right")
          .foregroundStyle(Color.fawxAccent)

        Text(headerTitle)
          .font(FawxTypography.heading1)
          .foregroundStyle(Color.fawxText)
          .lineLimit(1)

        Spacer(minLength: 0)

        Button(action: dismissAction) {
          Image(systemName: "xmark")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 24, height: 24)
        }
        .buttonStyle(.plain)
        .foregroundStyle(Color.fawxTextSecondary)
        .help("Hide thread inspector")
      }

      if let summaryLine {
        Text(summaryLine)
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
          .lineLimit(2)
      }
    }
  }

  private var headerTitle: String {
    threadContext?.displayTitle ?? "Thread Inspector"
  }

  private var summaryLine: String? {
    guard let threadContext else {
      return "Git, memory, and task context follow the active thread here."
    }

    switch threadContext.binding {
    case .general:
      return "General thread. No workspace or Git ownership is attached yet."
    case .workspace:
      var parts: [String] = []
      if let workspaceName = threadContext.workspaceName {
        parts.append("Sharing \(workspaceName)")
      }
      if let branchName = threadContext.branchName {
        parts.append("branch \(branchName)")
      }
      if let rootPath = threadContext.rootPath {
        parts.append(rootPath)
      }
      let summary = parts.joined(separator: " · ")
      return summary.isEmpty ? nil : summary
    case .worktree:
      var parts: [String] = []
      if let worktreeLabel = threadContext.worktreeLabel {
        parts.append("Owning \(worktreeLabel)")
      }
      if let branchName = threadContext.branchName {
        parts.append("branch \(branchName)")
      }
      if let rootPath = threadContext.rootPath {
        parts.append(rootPath)
      }
      let summary = parts.joined(separator: " · ")
      return summary.isEmpty ? nil : summary
    }
  }
}

private struct ThreadContextSummaryCard: View {
  let context: ThreadContextSnapshot

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      Text("Context")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      Text(bindingDescription)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)

      VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
        infoRow("Thread", value: context.displayTitle)
        if let workspaceName = context.workspaceName, context.workspaceKind != .general {
          infoRow("Workspace", value: workspaceName)
        }
        if let worktreeLabel = context.worktreeLabel {
          infoRow("Worktree", value: worktreeLabel)
        }
        if let branchName = context.branchName {
          infoRow("Branch", value: branchName)
        }
        if let baseRef = context.baseRef {
          infoRow("Base", value: baseRef)
        }
        if let rootPath = context.rootPath {
          infoRow("Root", value: rootPath)
        }
        if let repositoryOrigin = context.repositoryOrigin {
          infoRow("Origin", value: repositoryOrigin)
        }
        infoRow("Status", value: context.threadStatus.rawValue.capitalized)
        infoRow("Model", value: context.model)
        infoRow("Session", value: context.sessionID)
        infoRow(
          "Messages",
          value: context.messageCount == 1 ? "1 message" : "\(context.messageCount) messages"
        )
      }
    }
  }

  private var bindingDescription: String {
    switch context.binding {
    case .general:
      return "This thread is not attached to a workspace yet, so coding utilities stay dormant."
    case .workspace:
      return
        "This thread shares its workspace checkout, so Git and related utilities follow that lane."
    case .worktree:
      return
        "This thread owns an isolated worktree, so Git and context stay pinned to that PR lane."
    }
  }

  private func infoRow(_ label: String, value: String) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
      Text(label)
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .frame(width: 72, alignment: .leading)

      Text(value)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxText)
        .textSelection(.enabled)
    }
  }
}

private struct ThreadActivityCard: View {
  let activity: ThreadActivitySnapshot?
  let backgroundActivityNotice: BackgroundThreadActivityNotice?

  private var snapshot: ThreadActivityCardSnapshot {
    ThreadActivityCardSnapshot(
      activity: activity,
      backgroundActivityNotice: backgroundActivityNotice
    )
  }

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      Text("Activity")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      Text(snapshot.summaryText)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)

      if let detailText = snapshot.detailText {
        Text(detailText)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
      }

      if snapshot.infoRows.isEmpty == false {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
          ForEach(Array(snapshot.infoRows.enumerated()), id: \.offset) { _, row in
            infoRow(row.label, value: row.value)
          }
        }
      }
    }
  }

  private func infoRow(_ label: String, value: String) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
      Text(label)
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .frame(width: 84, alignment: .leading)

      Text(value)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxText)
        .textSelection(.enabled)
    }
  }
}

private struct ThreadSupportCard: View {
  let context: ThreadContextSnapshot
  let openSessionMemoryAction: () -> Void
  let openFullViewAction: () -> Void

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      Text("Support")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      Text("Memory and Git stay subordinate to the active thread instead of competing with it.")
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)

      HStack(spacing: FawxSpacing.paddingSM) {
        Button("Open Session Memory", action: openSessionMemoryAction)
          .buttonStyle(.borderedProminent)
          .tint(.fawxAccent)

        if context.hasRepositoryContext {
          Button("Open Full Git View", action: openFullViewAction)
            .buttonStyle(.bordered)
        }
      }

      if context.hasRepositoryContext {
        Text(
          "Switch to the Git tab for live status, diff, and recent commit context for this thread."
        )
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
      }
    }
  }
}

private struct GitSummaryCard: View {
  let snapshot: CompactGitPanelSnapshot
  @Bindable var viewModel: GitViewModel

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
        Image(systemName: "point.topleft.down.curvedto.point.bottomright.up")
          .foregroundStyle(Color.fawxAccent)

        Text(snapshot.branchTitle)
          .font(FawxTypography.heading1)
          .foregroundStyle(Color.fawxText)
          .lineLimit(1)

        Spacer(minLength: 0)

        GitCompactBadge(label: snapshot.statusBadgeLabel, color: snapshot.statusTone.color)
      }

      if let contextLine = viewModel.contextLine {
        Text(contextLine)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
          .lineLimit(2)
      }

      Text(snapshot.statusSummary)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)
        .lineLimit(2)
    }
  }
}

private struct GitQuickActionsCard: View {
  @Bindable var viewModel: GitViewModel

  let snapshot: CompactGitPanelSnapshot
  let openFullViewAction: () -> Void

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      Text("Quick Actions")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      actionButtons
      commitSection
      openFullViewButton
    }
  }

  private var actionButtons: some View {
    HStack(spacing: FawxSpacing.paddingSM) {
      Button("Stage All") {
        Task {
          await viewModel.stageAll()
        }
      }
      .buttonStyle(.bordered)
      .disabled(!snapshot.canStageAll)

      Button("Push") {
        viewModel.requestPushConfirmation()
      }
      .buttonStyle(.bordered)
      .disabled(!snapshot.canPush)
    }
  }

  private var commitSection: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
      TextField("Commit message", text: $viewModel.commitMessage)
        .textFieldStyle(.roundedBorder)

      HStack {
        Text(snapshot.stagedFileSummary)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)

        Spacer(minLength: 0)

        Button(viewModel.isPerformingAction ? "Committing..." : "Commit") {
          viewModel.requestCommitConfirmation()
        }
        .buttonStyle(.borderedProminent)
        .tint(.fawxAccent)
        .disabled(!snapshot.canCommit)
      }
    }
  }

  private var openFullViewButton: some View {
    Button("Open Full Git View", action: openFullViewAction)
      .buttonStyle(.borderedProminent)
      .tint(.fawxAccent)
  }
}

private struct GitFilesCard: View {
  let status: GitStatusResponse
  @Bindable var viewModel: GitViewModel

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      HStack {
        Text("Changed Files")
          .font(FawxTypography.heading2)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)

        GitCompactBadge(label: "\(status.files.count)", color: .fawxTextSecondary)
      }

      if status.clean {
        Text("No changes to stage.")
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
          .frame(maxWidth: .infinity, minHeight: 120)
      } else {
        fileGroups
      }
    }
  }

  @ViewBuilder
  private var fileGroups: some View {
    if !viewModel.stagedFiles.isEmpty {
      fileGroup(title: "Staged", files: viewModel.stagedFiles)
    }

    if !viewModel.unstagedFiles.isEmpty {
      fileGroup(title: "Unstaged", files: viewModel.unstagedFiles)
    }
  }

  private func fileGroup(title: String, files: [GitFileEntry]) -> some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
      Text(title)
        .font(FawxTypography.sidebarTitle)
        .foregroundStyle(Color.fawxTextSecondary)

      ForEach(files) { file in
        Button {
          Task {
            await viewModel.toggleStage(for: file)
          }
        } label: {
          GitCompactFileRow(file: file, isSelected: viewModel.selectedFilePath == file.path)
        }
        .buttonStyle(.plain)
      }
    }
  }
}

private struct GitDiffCard: View {
  let snapshot: CompactGitPanelSnapshot

  var body: some View {
    FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
      HStack {
        Text(snapshot.diffTitle)
          .font(FawxTypography.heading2)
          .foregroundStyle(Color.fawxText)
          .lineLimit(1)

        Spacer(minLength: 0)

        GitCompactBadge(label: snapshot.previewLineCountLabel, color: .fawxTextSecondary)
      }

      diffContent

      if snapshot.isDiffTruncated {
        Text("Preview truncated. Open the full Git view to inspect the complete diff.")
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
      }
    }
  }

  @ViewBuilder
  private var diffContent: some View {
    if snapshot.previewLines.isEmpty {
      GitCompactPlaceholderView(
        title: "No diff selected",
        message: "Tap a file to preview its diff."
      )
    } else {
      ScrollView([.horizontal, .vertical]) {
        LazyVStack(alignment: .leading, spacing: 0) {
          ForEach(Array(snapshot.previewLines.enumerated()), id: \.offset) { _, line in
            GitCompactDiffLineView(line: line)
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingMD)
      }
      .frame(maxHeight: 220)
      .background(Color.fawxCode)
      .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
  }
}

private struct GitCompactBadge: View {
  let label: String
  let color: Color

  var body: some View {
    Text(label)
      .font(FawxTypography.status)
      .foregroundStyle(color)
      .padding(.horizontal, FawxSpacing.paddingSM)
      .padding(.vertical, FawxSpacing.paddingXS)
      .background(color.opacity(0.14))
      .clipShape(Capsule())
  }
}

private struct GitCompactFileRow: View {
  let file: GitFileEntry
  let isSelected: Bool

  var body: some View {
    HStack(spacing: FawxSpacing.paddingMD) {
      Text(file.status.shortLabel)
        .font(FawxTypography.code)
        .foregroundStyle(file.status.compactColor)
        .frame(width: 20)

      VStack(alignment: .leading, spacing: 2) {
        Text(file.path)
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxText)
          .lineLimit(1)

        Text(file.staged ? "Tap to unstage" : "Tap to stage")
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
      }
    }
    .padding(FawxSpacing.paddingMD)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(isSelected ? Color.fawxAccentSubtle : Color.fawxBackground)
    .clipShape(RoundedRectangle(cornerRadius: 12))
  }
}

private struct GitCompactDiffLineView: View {
  let line: String

  var body: some View {
    Text(verbatim: line.isEmpty ? " " : line)
      .font(FawxTypography.code)
      .foregroundStyle(foregroundColor)
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.horizontal, FawxSpacing.paddingSM)
      .padding(.vertical, 1)
      .background(backgroundColor)
      .textSelection(.enabled)
  }

  private var foregroundColor: Color {
    if line.hasPrefix("+"), !line.hasPrefix("+++") {
      return .fawxSuccess
    }
    if line.hasPrefix("-"), !line.hasPrefix("---") {
      return .fawxError
    }
    return .fawxText
  }

  private var backgroundColor: Color {
    if line.hasPrefix("+"), !line.hasPrefix("+++") {
      return Color.fawxSuccess.opacity(0.08)
    }
    if line.hasPrefix("-"), !line.hasPrefix("---") {
      return Color.fawxError.opacity(0.08)
    }
    return Color.clear
  }
}

private struct GitCompactPlaceholderView: View {
  let title: String
  let message: String
  var actionTitle: String?
  var action: (() -> Void)?

  var body: some View {
    VStack(spacing: FawxSpacing.paddingMD) {
      placeholderIcon
      placeholderCopy
      actionButton
    }
    .frame(maxWidth: .infinity, minHeight: 160)
    .padding(FawxSpacing.paddingLG)
    .background(Color.fawxBackground)
    .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
  }

  private var placeholderIcon: some View {
    Image(systemName: "arrow.trianglehead.branch")
      .font(.system(size: 26, weight: .semibold))
      .foregroundStyle(Color.fawxTextSecondary)
  }

  private var placeholderCopy: some View {
    VStack(spacing: FawxSpacing.paddingSM) {
      Text(title)
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      Text(message)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)
        .multilineTextAlignment(.center)
        .frame(maxWidth: FawxSpacing.placeholderCopyMaxWidth)
    }
  }

  @ViewBuilder
  private var actionButton: some View {
    if let actionTitle, let action {
      Button(actionTitle, action: action)
        .buttonStyle(.bordered)
    }
  }
}

extension GitFileState {
  fileprivate var compactColor: Color {
    switch self {
    case .modified:
      .fawxWarning
    case .added:
      .fawxSuccess
    case .deleted:
      .fawxError
    case .untracked:
      .fawxAccent
    case .renamed:
      .blue
    }
  }
}
