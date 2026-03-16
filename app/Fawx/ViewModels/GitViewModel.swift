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
    var status: GitStatusResponse?
    var diff: GitDiffResponse?
    var commits: [GitCommitEntry] = []
    var isLoading = false
    var errorMessage: String?
    var selectedFilePath: String?
    var commitMessage = ""
    var isPerformingAction = false
    var lastActionSummary: String?
    var pendingConfirmation: GitConfirmationRequest?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var stagedFiles: [GitFileEntry] {
        (status?.files ?? []).filter(\.staged).sorted { $0.path.localizedCaseInsensitiveCompare($1.path) == .orderedAscending }
    }

    var unstagedFiles: [GitFileEntry] {
        (status?.files ?? []).filter { !$0.staged }.sorted { $0.path.localizedCaseInsensitiveCompare($1.path) == .orderedAscending }
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

    func refresh() async {
        guard appState.isConfigured else {
            status = nil
            diff = nil
            commits = []
            errorMessage = nil
            selectedFilePath = nil
            lastActionSummary = nil
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            async let statusTask = appState.client.gitStatus()
            async let diffTask = appState.client.gitDiff()
            async let logTask = appState.client.gitLog(limit: 10)

            let (statusResponse, diffResponse, logResponse) = try await (statusTask, diffTask, logTask)
            status = statusResponse
            diff = diffResponse
            commits = logResponse.commits
            if let selectedFilePath, !(statusResponse.files.contains { $0.path == selectedFilePath }) {
                self.selectedFilePath = nil
            }
            errorMessage = nil
        } catch {
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
        selectFile(file)
        guard !isPerformingAction else {
            return
        }

        isPerformingAction = true
        defer { isPerformingAction = false }

        do {
            if file.staged {
                _ = try await appState.client.gitUnstage(paths: [file.path])
                appState.showToast(message: "Unstaged \(file.path).", style: .info)
            } else {
                _ = try await appState.client.gitStage(paths: [file.path])
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
            action: { try await appState.client.gitStageAll() }
        )
    }

    func unstageAll() async {
        await runMutation(
            successMessage: "Unstaged all changes.",
            action: { try await appState.client.gitUnstageAll() }
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
            action: { try await appState.client.gitCommit(message: message) },
            onSuccess: { _ in
                self.commitMessage = ""
            }
        )
    }

    private func performPush() async {
        await runMutation(
            successMessage: nil,
            action: { try await appState.client.gitPush() },
            onSuccess: { response in
                self.lastActionSummary = "Pushed \(response.branch) to \(response.remote)."
                self.appState.showToast(message: self.lastActionSummary ?? "Pushed changes.", style: .success)
            }
        )
    }

    private func performPull() async {
        await runMutation(
            successMessage: nil,
            action: { try await appState.client.gitPull() },
            onSuccess: { response in
                self.lastActionSummary = response.summary
                self.appState.showToast(
                    message: response.conflicts ? "Pull completed with conflicts." : (response.summary.isEmpty ? "Pulled latest changes." : response.summary),
                    style: response.conflicts ? .warning : .success
                )
            }
        )
    }

    func fetch() async {
        await runMutation(
            successMessage: nil,
            action: { try await appState.client.gitFetch() },
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
            guard let header = block.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first else {
                return false
            }

            let headerLine = String(header)
            if headerLine == "diff --git a/\(path) b/\(path)" {
                return true
            }

            return block.contains("\n--- a/\(path)\n") || block.contains("\n+++ b/\(path)\n")
        }
    }
}
