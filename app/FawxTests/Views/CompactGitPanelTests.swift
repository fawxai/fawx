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
            "Inspect working tree status, commit changes, and sync with remote."
        )
        XCTAssertEqual(snapshot.changedFileCount, 0)
        XCTAssertEqual(snapshot.stagedFileSummary, "Stage files before committing.")
        XCTAssertFalse(snapshot.canCommit)
        XCTAssertEqual(snapshot.previewLineCountLabel, "0")
        XCTAssertFalse(snapshot.isDiffTruncated)
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

    private func makeSUT(viewModel: GitViewModel) -> CompactGitPanel {
        CompactGitPanel(
            viewModel: viewModel,
            openFullViewAction: {},
            dismissAction: {}
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
