import Observation
import SwiftUI

struct ExperimentsView: View {
    @Bindable var viewModel: ExperimentsViewModel
    let isActive: Bool

    init(viewModel: ExperimentsViewModel, isActive: Bool = true) {
        _viewModel = Bindable(viewModel)
        self.isActive = isActive
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
                if let errorMessage = viewModel.errorMessage, !viewModel.experiments.isEmpty {
                    Text(errorMessage)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxWarning)
                        .padding(.horizontal, containerPadding)
                }

                content
                    .padding(.horizontal, containerPadding)
                    .padding(.vertical, FawxSpacing.paddingLG)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color.fawxBackground)
        .refreshable {
            await viewModel.refresh()
        }
        .task(id: isActive) {
            guard isActive else {
                return
            }
            await viewModel.refresh()
        }
        .task(id: "poll:\(isActive)") {
            guard isActive else {
                return
            }

            while !Task.isCancelled {
                try? await Task.sleep(for: RefreshCadence.dashboardPanels)
                guard !Task.isCancelled else {
                    break
                }
                guard isActive else {
                    break
                }
                await viewModel.refresh()
            }
        }
        .sheet(
            isPresented: Binding(
                get: { viewModel.selectedExperimentID != nil },
                set: { isPresented in
                    if !isPresented {
                        viewModel.clearSelection()
                    }
                }
            )
        ) {
            ExperimentDetailSheet(viewModel: viewModel)
                .fawxOpaqueModalPresentation()
        }
    }

    @ViewBuilder
    private var content: some View {
        if viewModel.isLoading && viewModel.experiments.isEmpty {
            ProgressView("Loading experiments...")
                .frame(maxWidth: .infinity, minHeight: 260)
        } else if let errorMessage = viewModel.errorMessage, viewModel.experiments.isEmpty {
            ExperimentsPlaceholderView(
                title: "Could not load experiments",
                message: errorMessage,
                actionTitle: "Try Again",
                action: {
                    Task {
                        await viewModel.refresh()
                    }
                }
            )
        } else if viewModel.experiments.isEmpty {
            ExperimentsPlaceholderView(
                title: "No experiments yet",
                message: "Experiment history will appear here once runs have been created."
            )
        } else {
            LazyVStack(spacing: FawxSpacing.paddingMD) {
                ForEach(viewModel.experiments) { experiment in
                    Button {
                        Task {
                            await viewModel.selectExperiment(experiment)
                        }
                    } label: {
                        ExperimentSummaryCard(experiment: experiment)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var containerPadding: CGFloat {
#if os(macOS)
        FawxSpacing.paddingXL
#else
        FawxSpacing.paddingLG
#endif
    }
}

private struct ExperimentSummaryCard: View {
    let experiment: ExperimentSummary

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(experiment.name)
                        .font(FawxTypography.heading2)
                        .foregroundStyle(Color.fawxText)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Text(experiment.kind.displayName)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                ExperimentStatusBadge(status: experiment.status)
            }

            Text(experiment.scoreSummary)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)

            Text("Created \(relativeTimestampString(experiment.createdAt))")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private struct ExperimentDetailSheet: View {
    @Environment(\.dismiss) private var dismiss

    @Bindable var viewModel: ExperimentsViewModel

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    if viewModel.isLoadingDetail && viewModel.selectedExperiment == nil {
                        ProgressView("Loading experiment...")
                            .frame(maxWidth: .infinity, minHeight: 260)
                    } else if let errorMessage = viewModel.detailErrorMessage,
                              viewModel.selectedExperiment == nil {
                        ExperimentsPlaceholderView(
                            title: "Could not load experiment",
                            message: errorMessage,
                            actionTitle: "Retry",
                            action: {
                                Task {
                                    await viewModel.refreshSelectedExperiment()
                                }
                            }
                        )
                    } else if let detail = viewModel.selectedExperiment {
                        overviewCard(detail)
                        configurationCard(detail)
                        timingCard(detail)

                        if detail.status == .running || detail.status == .queued {
                            runningControls(detail)
                        }

                        if let results = viewModel.selectedResults,
                           detail.status == .completed || detail.result != nil {
                            resultsCard(results)
                        } else if let resultsErrorMessage = viewModel.resultsErrorMessage,
                                  !resultsErrorMessage.isEmpty {
                            statusMessageCard(
                                title: "Results unavailable",
                                message: resultsErrorMessage,
                                color: .fawxWarning
                            )
                        }

                        if let error = detail.error, !error.isEmpty {
                            statusMessageCard(title: "Failure", message: error, color: .fawxError)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .navigationTitle(viewModel.selectedExperiment?.name ?? "Experiment")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        viewModel.clearSelection()
                        dismiss()
                    }
                }
            }
        }
        .frame(minWidth: 460, minHeight: 540)
    }

    private func overviewCard(_ detail: ExperimentDetail) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(detail.name)
                        .font(FawxTypography.heading1)
                        .foregroundStyle(Color.fawxText)

                    Text(detail.kind.displayName)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)

                ExperimentStatusBadge(status: detail.status)
            }

            if let progress = detail.progress {
                Text("Progress: \(progress.completedMatches) of \(progress.totalMatches) matches")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
            }

            if !detail.fleetNodes.isEmpty {
                Text("Nodes: \(detail.fleetNodes.joined(separator: ", "))")
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

    private func configurationCard(_ detail: ExperimentDetail) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Configuration")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            ExperimentInfoRow(label: "Population", value: "\(detail.config.population)")
            ExperimentInfoRow(label: "Rounds", value: "\(detail.config.rounds)")
            ExperimentInfoRow(label: "Min confidence", value: detail.config.minConfidence ?? "Default")
            ExperimentInfoRow(label: "Output mode", value: detail.config.outputMode ?? "Default")
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func timingCard(_ detail: ExperimentDetail) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Timing")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            ExperimentInfoRow(label: "Created", value: absoluteTimestampString(detail.createdAt))
            ExperimentInfoRow(label: "Started", value: detail.startedAt.map(absoluteTimestampString) ?? "Not started")
            ExperimentInfoRow(label: "Completed", value: detail.completedAt.map(absoluteTimestampString) ?? "In progress")
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func runningControls(_ detail: ExperimentDetail) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Live Run")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text("This experiment is still running. You can stop it if needed.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            Button(viewModel.isStoppingExperiment ? "Stopping..." : "Stop Experiment") {
                Task {
                    await viewModel.stopSelectedExperiment()
                }
            }
            .buttonStyle(.borderedProminent)
            .tint(.fawxError)
            .disabled(viewModel.isStoppingExperiment)
        }
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func resultsCard(_ results: ExperimentResultsResponse) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Results")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            if results.leaders.isEmpty {
                Text("No candidate scores are available yet.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            } else {
                VStack(spacing: FawxSpacing.paddingSM) {
                    ForEach(Array(results.leaders.enumerated()), id: \.element.id) { index, leader in
                        ExperimentLeaderRow(leader: leader, isWinner: index == 0)
                    }
                }
            }

            HStack(spacing: FawxSpacing.paddingMD) {
                ExperimentSummaryPill(title: "Plans", value: "\(results.plansGenerated)")
                ExperimentSummaryPill(title: "Branches", value: "\(results.branchesCreated.count)")
                ExperimentSummaryPill(title: "Skipped", value: "\(results.skipped.count)")
            }

            if let tournament = results.tournament {
                statusMessageCard(
                    title: "Tournament",
                    message: "Round \(tournament.round) of \(tournament.totalRounds), \(tournament.remainingMatches) matches remaining.",
                    color: .fawxAccent
                )
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

    private func statusMessageCard(title: String, message: String, color: Color) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text(title)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(color)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
        .padding(FawxSpacing.paddingMD)
        .background(color.opacity(0.1))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

private struct ExperimentLeaderRow: View {
    let leader: ExperimentLeader
    let isWinner: Bool

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 2) {
                Text(leader.name)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text(leader.chainID)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Spacer(minLength: 0)

            VStack(alignment: .trailing, spacing: 2) {
                Text(leader.score.formatted(.number.precision(.fractionLength(2))))
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text(leader.risk.capitalized)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(isWinner ? Color.fawxSuccess.opacity(0.14) : Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(alignment: .topLeading) {
            if isWinner {
                Text("Winner")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxSuccess)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, 4)
                    .background(Color.fawxSuccess.opacity(0.14))
                    .clipShape(Capsule())
                    .padding(FawxSpacing.paddingSM)
            }
        }
    }
}

private struct ExperimentStatusBadge: View {
    let status: ExperimentStatus
    @State private var isAnimating = false

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(status.color)
                .frame(width: 8, height: 8)
                .opacity(status == .running ? (isAnimating ? 0.35 : 1) : 1)

            Text(status.displayName)
                .font(FawxTypography.status)
                .foregroundStyle(status.color)
        }
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, FawxSpacing.paddingXS)
        .background(status.color.opacity(0.14))
        .clipShape(Capsule())
        .onAppear {
            guard status == .running else {
                return
            }
            withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) {
                isAnimating = true
            }
        }
    }
}

private struct ExperimentInfoRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            Text(label)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 120, alignment: .leading)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)

            Spacer(minLength: 0)
        }
    }
}

private struct ExperimentSummaryPill: View {
    let title: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

private struct ExperimentsPlaceholderView: View {
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: "waveform.path.ecg.rectangle")
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Color.fawxTextSecondary)

            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 480)

            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.bordered)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 280)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private extension ExperimentStatus {
    var color: Color {
        switch self {
        case .queued:
            .fawxWarning
        case .running:
            .blue
        case .completed:
            .fawxSuccess
        case .stopped:
            .fawxTextSecondary
        case .failed:
            .fawxError
        }
    }
}

private func absoluteTimestampString(_ epochSeconds: Int) -> String {
    Date(timeIntervalSince1970: TimeInterval(epochSeconds)).formatted(date: .abbreviated, time: .shortened)
}
