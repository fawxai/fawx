import Observation
import SwiftUI

struct GitView: View {
    @Bindable var viewModel: GitViewModel

    var body: some View {
        Group {
#if os(macOS)
            splitLayout
#else
            stackedLayout
#endif
        }
        .background(Color.fawxBackground)
        .task { @MainActor in
            await viewModel.refresh()
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Fetch") {
                    Task {
                        await viewModel.fetch()
                    }
                }
                .disabled(viewModel.isPerformingAction)

                Button("Pull") {
                    Task {
                        await viewModel.pull()
                    }
                }
                .disabled(viewModel.isPerformingAction)

                Button("Push") {
                    Task {
                        await viewModel.push()
                    }
                }
                .disabled(viewModel.isPerformingAction)
            }
        }
    }

    #if os(macOS)
    private var splitLayout: some View {
        HSplitView {
            ScrollView {
                GitSidebarContent(viewModel: viewModel)
                    .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .refreshable {
                await viewModel.refresh()
            }
            .frame(minWidth: 360, idealWidth: 420, maxWidth: 480)

            GitDiffPanel(
                diff: viewModel.displayedDiff,
                summary: viewModel.diff,
                selectedFilePath: viewModel.selectedFilePath
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
    #endif

    private var stackedLayout: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                GitSidebarContent(viewModel: viewModel)
                GitDiffPanel(
                    diff: viewModel.displayedDiff,
                    summary: viewModel.diff,
                    selectedFilePath: viewModel.selectedFilePath
                )
            }
            .padding(FawxSpacing.paddingLG)
        }
        .refreshable {
            await viewModel.refresh()
        }
    }
}

private struct GitSidebarContent: View {
    @Bindable var viewModel: GitViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            branchCard

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
                filesCard(status: status)
                commitCard
                commitsCard
            } else if viewModel.isLoading {
                ProgressView("Loading Git status...")
                    .frame(maxWidth: .infinity, minHeight: 220)
            } else {
                GitPlaceholderView(
                    title: "No repository data",
                    message: "Git information will appear when the server is connected to a workspace."
                )
            }
        }
    }

    private var branchCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
                Image(systemName: "point.topleft.down.curvedto.point.bottomright.up")
                    .foregroundStyle(Color.fawxAccent)

                Text(viewModel.status?.branch ?? "Git")
                    .font(FawxTypography.heading1)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: 0)

                GitCleanBadge(isClean: viewModel.status?.clean ?? false)
            }

            if let summary = viewModel.lastActionSummary, !summary.isEmpty {
                Text(summary)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else if let status = viewModel.status {
                Text(status.clean ? "Working tree is clean." : "\(status.files.count) changed files")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                Text("Inspect working tree status, commit changes, and sync with remote.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func filesCard(status: GitStatusResponse) -> some View {
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
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
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
                    GitFileRow(file: file, isSelected: viewModel.selectedFilePath == file.path)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var commitCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Commit")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            TextField("Commit message", text: $viewModel.commitMessage)
                .textFieldStyle(.roundedBorder)

            HStack {
                Text(viewModel.stagedFiles.isEmpty ? "Stage files before committing." : "\(viewModel.stagedFiles.count) staged files")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)

                Spacer(minLength: 0)

                Button(viewModel.isPerformingAction ? "Committing..." : "Commit") {
                    Task {
                        await viewModel.commit()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(!viewModel.canCommit || viewModel.isPerformingAction)
            }
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private var commitsCard: some View {
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
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
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
                ScrollView([.horizontal, .vertical]) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(diff.split(separator: "\n", omittingEmptySubsequences: false).enumerated()), id: \.offset) { _, line in
                            GitDiffLineView(line: String(line))
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingLG)
                }
                .background(Color.fawxCode)
                .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
            }
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private struct GitFileRow: View {
    let file: GitFileEntry
    let isSelected: Bool

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Text(file.status.shortLabel)
                .font(FawxTypography.code)
                .foregroundStyle(file.status.color)
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 2) {
                Text(file.path)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
                    .frame(maxWidth: .infinity, alignment: .leading)

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
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private extension GitFileState {
    var color: Color {
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
