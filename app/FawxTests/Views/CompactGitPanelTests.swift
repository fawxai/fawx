import XCTest

@testable import Fawx

@MainActor
final class CompactGitPanelTests: XCTestCase {
  func testSnapshotReflectsDirtyRepositoryAndTruncatedPreview() {
    let appState = AppState(startLoadingPersistedState: false)
    let viewModel = GitViewModel(appState: appState)
    let selectedPath = "Sources/CompactGitPanel.swift"

    viewModel.status = GitStatusResponse(
      branch: "feature/review",
      files: [
        GitFileEntry(path: selectedPath, status: .modified, staged: false),
        GitFileEntry(path: "README.md", status: .added, staged: true),
      ],
      clean: false
    )
    viewModel.diff = GitDiffResponse(
      diff: makeDiff(path: selectedPath, bodyLineCount: 90),
      filesChanged: 1,
      insertions: 90,
      deletions: 0
    )
    viewModel.selectedFilePath = selectedPath
    viewModel.commitMessage = "Tighten panel coverage"

    let snapshot = makeSUT(viewModel: viewModel).snapshot

    XCTAssertEqual(snapshot.primaryState, .ready)
    XCTAssertEqual(snapshot.branchTitle, "feature/review")
    XCTAssertEqual(snapshot.statusBadgeLabel, "Dirty")
    XCTAssertEqual(snapshot.statusTone, .warning)
    XCTAssertEqual(snapshot.statusSummary, "2 changed files")
    XCTAssertEqual(snapshot.changedFileCount, 2)
    XCTAssertEqual(snapshot.stagedFileCount, 1)
    XCTAssertEqual(snapshot.unstagedFileCount, 1)
    XCTAssertEqual(snapshot.stagedFileSummary, "1 staged file")
    XCTAssertTrue(snapshot.canStageAll)
    XCTAssertTrue(snapshot.canPush)
    XCTAssertTrue(snapshot.canCommit)
    XCTAssertEqual(snapshot.diffTitle, selectedPath)
    XCTAssertEqual(snapshot.previewLineCountLabel, "80/94")
    XCTAssertEqual(snapshot.previewLines.count, 80)
    XCTAssertTrue(snapshot.isDiffTruncated)
  }

  func testSnapshotReportsErrorStateWhenStatusLoadFails() {
    let appState = AppState(startLoadingPersistedState: false)
    let viewModel = GitViewModel(appState: appState)

    viewModel.errorMessage = "Git unavailable"
    viewModel.commitMessage = "No-op"

    let snapshot = makeSUT(viewModel: viewModel).snapshot

    XCTAssertEqual(snapshot.primaryState, .error(message: "Git unavailable"))
    XCTAssertEqual(snapshot.branchTitle, "Git")
    XCTAssertEqual(snapshot.statusBadgeLabel, "Unknown")
    XCTAssertEqual(snapshot.statusTone, .neutral)
    XCTAssertEqual(
      snapshot.statusSummary,
      "Git status follows the active thread here."
    )
    XCTAssertEqual(snapshot.changedFileCount, 0)
    XCTAssertEqual(snapshot.stagedFileSummary, "Stage files before committing.")
    XCTAssertFalse(snapshot.canCommit)
    XCTAssertEqual(snapshot.previewLineCountLabel, "0")
    XCTAssertFalse(snapshot.isDiffTruncated)
  }

  func testSnapshotReportsNoGitForGeneralThreadWithoutRepositoryContext() {
    let appState = AppState(startLoadingPersistedState: false)
    let viewModel = GitViewModel(appState: appState)

    viewModel.bindThreadContext(
      makeThreadContext(
        workspaceKind: .general,
        binding: .general,
        branchName: nil,
        worktreeLabel: nil,
        workspaceName: nil,
        workspacePath: nil,
        repositoryOrigin: nil
      )
    )

    let snapshot = makeSUT(viewModel: viewModel).snapshot

    XCTAssertEqual(snapshot.primaryState, .empty)
    XCTAssertEqual(snapshot.branchTitle, "Git")
    XCTAssertEqual(snapshot.statusBadgeLabel, "No Git")
    XCTAssertEqual(snapshot.statusTone, .neutral)
    XCTAssertEqual(snapshot.statusSummary, "This thread is not attached to a repository yet.")
  }

  func testSnapshotUsesThreadBoundBranchIdentityBeforeGitStatusLoads() {
    let appState = AppState(startLoadingPersistedState: false)
    let viewModel = GitViewModel(appState: appState)

    viewModel.bindThreadContext(
      makeThreadContext(
        branchName: "feature/thread-context",
        worktreeLabel: "thread-context",
        workspaceName: "Repo",
        workspacePath: "/tmp/repo"
      )
    )

    let snapshot = makeSUT(viewModel: viewModel).snapshot

    XCTAssertEqual(snapshot.primaryState, .empty)
    XCTAssertEqual(snapshot.branchTitle, "feature/thread-context")
    XCTAssertEqual(snapshot.statusBadgeLabel, "Unknown")
    XCTAssertEqual(snapshot.statusSummary, "Git status follows the active thread here.")
  }

  func testSnapshotUsesLoadingStateBeforeRepositoryDataArrives() {
    let appState = AppState(startLoadingPersistedState: false)
    let viewModel = GitViewModel(appState: appState)

    viewModel.isLoading = true

    let snapshot = makeSUT(viewModel: viewModel).snapshot

    XCTAssertEqual(snapshot.primaryState, .loading)
    XCTAssertEqual(snapshot.statusBadgeLabel, "Loading")
    XCTAssertEqual(snapshot.statusTone, .neutral)
    XCTAssertEqual(snapshot.statusSummary, "Loading working tree status...")
  }

  func testThreadActivityCardSnapshotUsesBackgroundNoticeContent() {
    let snapshot = ThreadActivityCardSnapshot(
      activity: nil,
      backgroundActivityNotice: BackgroundThreadActivityNotice(
        primaryThreadID: "thread-background",
        primaryThreadTitle: "Background Thread",
        primaryBadgeLabel: "Implementing",
        activeThreadCount: 2,
        subagentThreadCount: 1
      )
    )

    XCTAssertEqual(snapshot.summaryText, "2 other threads running")
    XCTAssertEqual(snapshot.detailText, "Background Thread · Implementing · +1 more · 1 subagent")
    XCTAssertTrue(snapshot.infoRows.isEmpty)
  }

  func testThreadActivityCardSnapshotFallsBackToSelectedThreadActivity() {
    let snapshot = ThreadActivityCardSnapshot(
      activity: makeThreadActivity(
        runtime: ThreadRuntimeActivity(
          isStreaming: true,
          liveToolCallCount: 2,
          runningToolCallCount: 1,
          progressLabel: "Implementing",
          progressMessage: "Editing files"
        )
      ),
      backgroundActivityNotice: nil
    )

    XCTAssertEqual(snapshot.summaryText, "The selected thread is actively working.")
    XCTAssertEqual(snapshot.detailText, "Implementing · 1 tool running")
    XCTAssertEqual(
      snapshot.infoRows,
      [
        .init(label: "Status", value: "Implementing"),
        .init(label: "Active tools", value: "1 tool running"),
        .init(label: "Detail", value: "Editing files"),
      ]
    )
  }

  private func makeSUT(viewModel: GitViewModel) -> CompactGitPanel {
    CompactGitPanel(
      viewModel: viewModel,
      threadContext: viewModel.threadContext,
      threadActivity: nil,
      backgroundActivityNotice: nil,
      openSessionMemoryAction: {},
      openFullViewAction: {},
      dismissAction: {}
    )
  }

  private func makeThreadContext(
    workspaceKind: WorkspaceKind = .repository,
    binding: ThreadContextSnapshot.Binding = .workspace,
    branchName: String? = "dev",
    worktreeLabel: String? = nil,
    workspaceName: String? = "Repo",
    workspacePath: String? = "/tmp/repo",
    repositoryOrigin: String? = "git@github.com:example/fawx.git"
  ) -> ThreadContextSnapshot {
    ThreadContextSnapshot(
      thread: .init(
        id: "thread-1",
        sessionID: "session-1",
        displayTitle: "Thread 1",
        kind: .coding,
        status: .active,
        model: "gpt-5.4",
        messageCount: 3
      ),
      workspace: .init(
        name: workspaceName,
        path: workspacePath,
        kind: workspaceKind
      ),
      repository: .init(
        branchName: branchName,
        worktreeLabel: worktreeLabel,
        worktreePath: worktreeLabel.map { "/tmp/repo/.worktrees/\($0)" },
        baseRef: "origin/dev",
        origin: repositoryOrigin,
        isClean: true,
        divergenceLabel: nil,
        worktreeStatusLabel: nil
      ),
      binding: binding,
    )
  }

  private func makeThreadActivity(
    runtime: ThreadRuntimeActivity? = nil
  ) -> ThreadActivitySnapshot {
    ThreadActivitySnapshot(
      threadID: "thread-1",
      sessionID: "session-1",
      kind: .coding,
      status: .active,
      runtime: runtime,
      hasUnreadActivity: false
    )
  }

  private func makeDiff(path: String, bodyLineCount: Int) -> String {
    let body = (1...bodyLineCount).map { "+line \($0)" }.joined(separator: "\n")
    return """
      diff --git a/\(path) b/\(path)
      --- a/\(path)
      +++ b/\(path)
      @@ -0,0 +1,\(bodyLineCount) @@
      \(body)
      """
  }
}
