import Observation
import SwiftUI

private enum GitViewLayout {
  static let outerPadding = FawxSpacing.paddingSM
  static let sectionPadding = FawxSpacing.paddingMD
  static let rowPadding = FawxSpacing.paddingSM
  static let sidebarMinWidth: CGFloat = 340
  static let sidebarIdealWidth: CGFloat = 400
  static let sidebarMaxWidth: CGFloat = 460
}

struct GitView: View {
  @Bindable var viewModel: GitViewModel
  let isActive: Bool
  let repositoryTargets: [GitRepositoryTarget]
  let defaultRepositoryTarget: GitRepositoryTarget?
  let selectRepositoryTarget: (GitRepositoryTarget) -> Void

  init(
    viewModel: GitViewModel,
    isActive: Bool = true,
    repositoryTargets: [GitRepositoryTarget] = [],
    defaultRepositoryTarget: GitRepositoryTarget? = nil,
    selectRepositoryTarget: @escaping (GitRepositoryTarget) -> Void = { _ in }
  ) {
    _viewModel = Bindable(viewModel)
    self.isActive = isActive
    self.repositoryTargets = repositoryTargets
    self.defaultRepositoryTarget = defaultRepositoryTarget
    self.selectRepositoryTarget = selectRepositoryTarget
  }

  var body: some View {
    Group {
      #if os(macOS)
        splitLayout
      #else
        stackedLayout
      #endif
    }
    .background(Color.fawxBackground)
    .task(id: refreshTaskID) { @MainActor in
      guard isActive else {
        viewModel.cancelRefresh()
        return
      }
      selectDefaultRepositoryTargetIfNeeded()
      await viewModel.refresh()
    }
    .onAppear {
      selectDefaultRepositoryTargetIfNeeded()
    }
    .onChange(of: repositoryTargets) { _, _ in
      selectDefaultRepositoryTargetIfNeeded()
    }
    .onDisappear {
      viewModel.cancelRefresh()
    }
    .alert(
      item: Binding(
        get: { viewModel.pendingConfirmation },
        set: { newValue in
          if newValue == nil {
            viewModel.cancelPendingConfirmation()
          }
        }
      )
    ) { request in
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

  private var refreshTaskID: String {
    "\(isActive)-\(viewModel.refreshTaskID)"
  }

  private func selectDefaultRepositoryTargetIfNeeded() {
    if let repositoryTarget = viewModel.repositoryTarget,
      repositoryTargets.contains(where: { $0.id == repositoryTarget.id })
    {
      return
    }

    guard let defaultRepositoryTarget else {
      return
    }
    selectRepositoryTarget(defaultRepositoryTarget)
  }

  #if os(macOS)
    private var splitLayout: some View {
      HSplitView {
        ScrollView {
          GitSidebarContent(
            viewModel: viewModel,
            repositoryTargets: repositoryTargets,
            selectRepositoryTarget: selectRepositoryTarget
          )
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .padding(GitViewLayout.outerPadding)
        }
        .background(Color.fawxBackground)
        .refreshable {
          await viewModel.refresh()
        }
        .frame(
          minWidth: GitViewLayout.sidebarMinWidth,
          idealWidth: GitViewLayout.sidebarIdealWidth,
          maxWidth: GitViewLayout.sidebarMaxWidth
        )
        .frame(maxHeight: .infinity, alignment: .topLeading)

        GitDiffPanel(
          diff: viewModel.displayedDiff,
          summary: viewModel.diff,
          selectedFilePath: viewModel.selectedFilePath
        )
        .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
  #endif

  private var stackedLayout: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
        GitSidebarContent(
          viewModel: viewModel,
          repositoryTargets: repositoryTargets,
          selectRepositoryTarget: selectRepositoryTarget
        )
        GitDiffPanel(
          diff: viewModel.displayedDiff,
          summary: viewModel.diff,
          selectedFilePath: viewModel.selectedFilePath
        )
      }
      .padding(GitViewLayout.outerPadding)
    }
    .refreshable {
      await viewModel.refresh()
    }
  }
}

private struct GitOperationsMenu: View {
  @Bindable var viewModel: GitViewModel

  var body: some View {
    FawxDropdownMenu(minWidth: 150) {
      GitMenuLabel(title: "Operations", systemImage: "ellipsis.circle")
    } content: { dismiss in
      FawxDropdownActionRow(title: "Fetch", systemImage: "arrow.clockwise") {
        Task {
          await viewModel.fetch()
        }
        dismiss()
      }

      FawxDropdownActionRow(title: "Pull", systemImage: "arrow.down") {
        viewModel.requestPullConfirmation()
        dismiss()
      }

      FawxDropdownActionRow(title: "Push", systemImage: "arrow.up") {
        viewModel.requestPushConfirmation()
        dismiss()
      }
    }
    .disabled(viewModel.isPerformingAction)
    .help("Fetch, pull, or push repository changes")
    .accessibilityIdentifier("gitOperationsMenu")
  }
}

private struct GitMenuLabel: View {
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

private struct GitSidebarContent: View {
  @Bindable var viewModel: GitViewModel
  let repositoryTargets: [GitRepositoryTarget]
  let selectRepositoryTarget: (GitRepositoryTarget) -> Void

  var body: some View {
    let emptyStateCopy = emptyStateCopy

    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      branchSection

      if let errorMessage = viewModel.errorMessage, viewModel.status == nil {
        GitPlaceholderView(
          title: "Could not load Git status",
          message: errorMessage,
          actionTitle: "Try Again",
          action: {
            Task {
              await viewModel.refresh()
            }
          }
        )
      } else if let status = viewModel.status {
        filesSection(status: status)
        commitSection
        commitsSection
      } else if viewModel.isLoading {
        ProgressView("Loading Git status...")
          .frame(maxWidth: .infinity, minHeight: 220)
      } else {
        GitPlaceholderView(
          title: emptyStateCopy.title,
          message: emptyStateCopy.message
        )
      }
    }
  }

  private var branchSection: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
        Image(systemName: "point.topleft.down.curvedto.point.bottomright.up")
          .foregroundStyle(Color.fawxAccent)

        Text(viewModel.branchTitle)
          .font(FawxTypography.heading1)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)

        targetPicker
        GitOperationsMenu(viewModel: viewModel)
        GitCleanBadge(isClean: viewModel.status?.clean ?? false)
      }

      if let contextLine = viewModel.contextLine {
        Text(contextLine)
          .font(FawxTypography.status)
          .foregroundStyle(Color.fawxTextSecondary)
      }

      if let summary = viewModel.lastActionSummary, !summary.isEmpty {
        Text(summary)
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
      } else if let status = viewModel.status {
        Text(status.clean ? "Working tree is clean." : "\(status.files.count) changed files")
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
      } else if viewModel.threadContext?.hasRepositoryContext == false {
        Text(emptyStateCopy.message)
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
      } else {
        Text("Inspect working tree status, commit changes, and sync with remote.")
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
      }
    }
    .padding(GitViewLayout.sectionPadding)
    .fawxSurface(.section)
  }

  private var emptyStateCopy: (title: String, message: String) {
    if viewModel.repositoryTarget == nil && repositoryTargets.isEmpty == false {
      return (
        "Choose Git target",
        "Pick a workspace, worktree, or thread before inspecting repository changes."
      )
    }

    if viewModel.threadContext?.hasRepositoryContext == false {
      if let threadContext = viewModel.threadContext, threadContext.binding == .general {
        return (
          "No repository bound",
          "This thread is not attached to a workspace or branch yet."
        )
      }

      return (
        "No repository bound",
        "This thread is attached to a workspace without repository metadata."
      )
    }

    return (
      "No repository data",
      "Git information will appear when the server is connected to a workspace."
    )
  }

  @ViewBuilder
  private var targetPicker: some View {
    if repositoryTargets.isEmpty == false {
      FawxDropdownMenu(minWidth: 220) {
        GitMenuLabel(
          title: viewModel.repositoryTarget?.title ?? "Choose Target",
          systemImage: "folder.badge.gearshape",
          titleMaxWidth: 160
        )
      } content: { dismiss in
        ForEach(repositoryTargets) { target in
          FawxDropdownActionRow(
            title: target.title,
            systemImage: target.systemImage,
            isSelected: target.id == viewModel.repositoryTarget?.id
          ) {
            selectRepositoryTarget(target)
            dismiss()
          }
        }
      }
      .help("Choose the workspace, worktree, or thread Git should inspect")
      .accessibilityIdentifier("gitTargetPickerMenu")
    }
  }

  private func filesSection(status: GitStatusResponse) -> some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      HStack {
        Text("File Status")
          .font(FawxTypography.heading2)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)

        Button("Stage All") {
          Task {
            await viewModel.stageAll()
          }
        }
        .disabled(viewModel.unstagedFiles.isEmpty || viewModel.isPerformingAction)

        Button("Unstage All") {
          Task {
            await viewModel.unstageAll()
          }
        }
        .disabled(viewModel.stagedFiles.isEmpty || viewModel.isPerformingAction)
      }

      if status.clean {
        Text("No changes to stage.")
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
          .frame(maxWidth: .infinity, minHeight: 120)
      } else {
        if !viewModel.stagedFiles.isEmpty {
          fileGroup(title: "Staged", files: viewModel.stagedFiles)
        }

        if !viewModel.unstagedFiles.isEmpty {
          fileGroup(title: "Unstaged", files: viewModel.unstagedFiles)
        }
      }
    }
    .padding(GitViewLayout.sectionPadding)
    .fawxSurface(.section)
  }

  private func fileGroup(title: String, files: [GitFileEntry]) -> some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
      Text(title)
        .font(FawxTypography.sidebarTitle)
        .foregroundStyle(Color.fawxTextSecondary)

      ForEach(files) { file in
        GitFileRow(
          file: file,
          isSelected: viewModel.selectedFilePath == file.path,
          isActionDisabled: viewModel.isPerformingAction,
          openDiff: {
            viewModel.selectFile(file)
          },
          toggleStage: {
            Task {
              await viewModel.toggleStage(for: file)
            }
          }
        )
      }
    }
  }

  private var commitSection: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      Text("Commit")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      TextField("Commit message", text: $viewModel.commitMessage)
        .textFieldStyle(.roundedBorder)

      HStack {
        Text(
          viewModel.stagedFiles.isEmpty
            ? "Stage files before committing." : "\(viewModel.stagedFiles.count) staged files"
        )
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)

        Spacer(minLength: 0)

        Button(viewModel.isPerformingAction ? "Committing..." : "Commit") {
          viewModel.requestCommitConfirmation()
        }
        .buttonStyle(.borderedProminent)
        .tint(.fawxAccent)
        .disabled(!viewModel.canCommit || viewModel.isPerformingAction)
      }
    }
    .padding(GitViewLayout.sectionPadding)
    .fawxSurface(.section)
  }

  private var commitsSection: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      Text("Recent Commits")
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      if viewModel.commits.isEmpty {
        Text("No recent commits found.")
          .font(FawxTypography.chatBody)
          .foregroundStyle(Color.fawxTextSecondary)
      } else {
        ForEach(viewModel.commits) { commit in
          VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: FawxSpacing.paddingSM) {
              Text(commit.shortHash)
                .font(FawxTypography.code)
                .foregroundStyle(Color.fawxAccent)

              Text(commit.message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            Text("\(commit.author) · \(commit.timestamp)")
              .font(FawxTypography.status)
              .foregroundStyle(Color.fawxTextSecondary)
          }
          .padding(.vertical, FawxSpacing.paddingXS)
        }
      }
    }
    .padding(GitViewLayout.sectionPadding)
    .fawxSurface(.section)
  }
}

private struct GitDiffPanel: View {
  let diff: String
  let summary: GitDiffResponse?
  let selectedFilePath: String?

  var body: some View {
    VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
      HStack {
        Text(selectedFilePath ?? "Repository Diff")
          .font(FawxTypography.heading2)
          .foregroundStyle(Color.fawxText)

        Spacer(minLength: 0)

        if let summary {
          HStack(spacing: FawxSpacing.paddingSM) {
            GitSummaryBadge(label: "\(summary.filesChanged) files", color: .fawxTextSecondary)
            GitSummaryBadge(label: "+\(summary.insertions)", color: .fawxSuccess)
            GitSummaryBadge(label: "-\(summary.deletions)", color: .fawxError)
          }
        }
      }

      if diff.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        GitPlaceholderView(
          title: "No diff available",
          message: "Working tree changes will appear here."
        )
      } else {
        diffScrollView
      }
    }
    .padding(GitViewLayout.sectionPadding)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .fawxSurface(.section)
  }

  private var diffScrollView: some View {
    GeometryReader { proxy in
      ScrollView([.horizontal, .vertical]) {
        LazyVStack(alignment: .leading, spacing: 0) {
          ForEach(
            Array(diff.split(separator: "\n", omittingEmptySubsequences: false).enumerated()),
            id: \.offset
          ) { _, line in
            GitDiffLineView(line: String(line))
          }
        }
        .frame(
          minWidth: max(0, proxy.size.width - (GitViewLayout.sectionPadding * 2)),
          alignment: .leading
        )
        .padding(GitViewLayout.sectionPadding)
      }
      .fawxSurface(.code)
    }
    .frame(minHeight: 240)
  }
}

private struct GitFileRow: View {
  private enum Layout {
    static let actionWidth: CGFloat = 78
    static let actionLabelWidth: CGFloat = 58
    static let minimumHitHeight: CGFloat = 36
  }

  let file: GitFileEntry
  let isSelected: Bool
  let isActionDisabled: Bool
  let openDiff: () -> Void
  let toggleStage: () -> Void

  var body: some View {
    HStack(spacing: FawxSpacing.paddingSM) {
      Button(action: openDiff) {
        HStack(spacing: FawxSpacing.paddingMD) {
          Text(file.status.shortLabel)
            .font(FawxTypography.code)
            .foregroundStyle(file.status.color)
            .frame(width: 20)

          VStack(alignment: .leading, spacing: 2) {
            Text(file.path)
              .font(FawxTypography.chatBody)
              .foregroundStyle(Color.fawxText)
              .lineLimit(2)
              .frame(maxWidth: .infinity, alignment: .leading)

            Text(file.staged ? "Staged" : "Unstaged")
              .font(FawxTypography.status)
              .foregroundStyle(Color.fawxTextSecondary)
          }
          .frame(maxWidth: .infinity, alignment: .leading)

          Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, minHeight: Layout.minimumHitHeight, alignment: .leading)
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .frame(maxWidth: .infinity, minHeight: Layout.minimumHitHeight, alignment: .leading)
      .contentShape(Rectangle())

      Button(action: toggleStage) {
        Text(file.staged ? "Unstage" : "Stage")
          .font(FawxTypography.status)
          .frame(width: Layout.actionLabelWidth)
      }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .disabled(isActionDisabled)
        .frame(width: Layout.actionWidth, alignment: .trailing)
        .help(file.staged ? "Unstage \(file.path)" : "Stage \(file.path)")
    }
    .padding(GitViewLayout.rowPadding)
    .frame(maxWidth: .infinity, alignment: .leading)
    .fawxRowChrome(isSelected: isSelected, cornerRadius: 12)
  }
}

private struct GitDiffLineView: View {
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

private struct GitCleanBadge: View {
  let isClean: Bool

  var body: some View {
    Text(isClean ? "Clean" : "Dirty")
      .font(FawxTypography.status)
      .foregroundStyle(isClean ? Color.fawxSuccess : Color.fawxWarning)
      .padding(.horizontal, FawxSpacing.paddingSM)
      .padding(.vertical, FawxSpacing.paddingXS)
      .background((isClean ? Color.fawxSuccess : Color.fawxWarning).opacity(0.14))
      .clipShape(Capsule())
  }
}

private struct GitSummaryBadge: View {
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

private struct GitPlaceholderView: View {
  let title: String
  let message: String
  var actionTitle: String?
  var action: (() -> Void)?

  var body: some View {
    VStack(spacing: FawxSpacing.paddingMD) {
      Image(systemName: "arrow.trianglehead.branch")
        .font(.system(size: 28, weight: .semibold))
        .foregroundStyle(Color.fawxTextSecondary)

      Text(title)
        .font(FawxTypography.heading2)
        .foregroundStyle(Color.fawxText)

      Text(message)
        .font(FawxTypography.chatBody)
        .foregroundStyle(Color.fawxTextSecondary)
        .multilineTextAlignment(.center)
        .frame(maxWidth: 420)

      if let actionTitle, let action {
        Button(actionTitle, action: action)
          .buttonStyle(.bordered)
      }
    }
    .frame(maxWidth: .infinity, minHeight: 240)
    .padding(GitViewLayout.sectionPadding)
    .fawxSurface(.section)
  }
}

extension GitFileState {
  fileprivate var color: Color {
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

private extension GitRepositoryTarget {
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
