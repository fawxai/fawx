import Observation
import SwiftUI

struct CompactGitPanel: View {
    @Bindable var viewModel: GitViewModel

    let openFullViewAction: () -> Void
    let dismissAction: () -> Void

    var snapshot: CompactGitPanelSnapshot {
        CompactGitPanelSnapshot(viewModel: viewModel)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                GitCompactHeaderCard(snapshot: snapshot, dismissAction: dismissAction)
                GitQuickActionsCard(
                    viewModel: viewModel,
                    snapshot: snapshot,
                    openFullViewAction: openFullViewAction
                )
                contentSection
            }
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground)
        .task {
            await viewModel.refresh()
        }
        .alert(item: pendingConfirmationBinding, content: confirmationAlert)
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
                message: "Git information appears when the server is connected to a workspace."
            )
        }
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
                branchTitle: status.branch,
                statusBadgeLabel: status.clean ? "Clean" : "Dirty",
                statusTone: status.clean ? .success : .warning,
                statusSummary: statusSummary(for: status),
                changedFileCount: status.files.count
            )
        }

        return StatusDetails(
            branchTitle: "Git",
            statusBadgeLabel: viewModel.isLoading ? "Loading" : "Unknown",
            statusTone: .neutral,
            statusSummary: viewModel.isLoading
                ? "Loading working tree status..."
                : "Inspect working tree status, commit changes, and sync with remote.",
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

        return displayedDiff
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

private struct GitCompactHeaderCard: View {
    let snapshot: CompactGitPanelSnapshot
    let dismissAction: () -> Void

    var body: some View {
        FawxSurfaceCard(spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
                Image(systemName: "point.topleft.down.curvedto.point.bottomright.up")
                    .foregroundStyle(Color.fawxAccent)

                Text(snapshot.branchTitle)
                    .font(FawxTypography.heading1)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: 0)

                Button(action: dismissAction) {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .frame(width: 24, height: 24)
                }
                .buttonStyle(.plain)
                .foregroundStyle(Color.fawxTextSecondary)
                .help("Hide Git side panel")
            }

            HStack(spacing: FawxSpacing.paddingSM) {
                GitCompactBadge(label: snapshot.statusBadgeLabel, color: snapshot.statusTone.color)

                Text(snapshot.statusSummary)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .lineLimit(2)
            }
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
        Button("Open Full View", action: openFullViewAction)
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

private extension GitFileState {
    var compactColor: Color {
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
