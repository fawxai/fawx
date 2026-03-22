import Observation
import SwiftUI

struct FleetView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Bindable var viewModel: FleetViewModel
    let isActive: Bool

    init(viewModel: FleetViewModel, isActive: Bool = true) {
        _viewModel = Bindable(viewModel)
        self.isActive = isActive
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
                summaryCard
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(containerPadding)
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
                get: { viewModel.selectedNodeID != nil },
                set: { isPresented in
                    if !isPresented {
                        viewModel.closeDetail()
                    }
                }
            )
        ) {
            FleetNodeDetailSheet(viewModel: viewModel)
                .fawxOpaqueModalPresentation()
        }
    }

    private var summaryCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text(viewModel.summaryLine)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            if let overview = viewModel.overview {
                HStack(spacing: FawxSpacing.paddingMD) {
                    FleetMetricPill(title: "Active Tasks", value: "\(overview.activeTasks)")
                    FleetMetricPill(title: "Queued", value: "\(overview.queuedTasks)")
                    FleetMetricPill(title: "Updated", value: relativeTimestampString(overview.updatedAt))
                }
            } else {
                Text("Connected fleet nodes and dispatch status will appear here.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            if let errorMessage = viewModel.errorMessage, !viewModel.nodes.isEmpty {
                Text(errorMessage)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxWarning)
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

    @ViewBuilder
    private var content: some View {
        if viewModel.isLoading && viewModel.nodes.isEmpty {
            ProgressView("Loading fleet nodes...")
                .frame(maxWidth: .infinity, minHeight: 260)
        } else if let errorMessage = viewModel.errorMessage, viewModel.nodes.isEmpty {
            FleetPlaceholderView(
                title: "Could not load fleet",
                message: errorMessage,
                actionTitle: "Try Again",
                action: {
                    Task {
                        await viewModel.refresh()
                    }
                }
            )
        } else if viewModel.nodes.isEmpty {
            FleetPlaceholderView(
                title: "No fleet nodes registered",
                message: "Use `fawx fleet add` to connect a node."
            )
        } else {
            LazyVGrid(columns: gridColumns, spacing: FawxSpacing.paddingLG) {
                ForEach(viewModel.nodes) { node in
                    Button {
                        Task {
                            await viewModel.presentNode(node)
                        }
                    } label: {
                        FleetNodeCard(node: node)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var gridColumns: [GridItem] {
#if os(macOS)
        return [
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingLG),
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingLG),
        ]
#else
        if horizontalSizeClass == .regular {
            return [
                GridItem(.flexible(minimum: 220), spacing: FawxSpacing.paddingLG),
                GridItem(.flexible(minimum: 220), spacing: FawxSpacing.paddingLG),
            ]
        }
        return [GridItem(.flexible(minimum: 220), spacing: FawxSpacing.paddingLG)]
#endif
    }

    private var containerPadding: CGFloat {
#if os(macOS)
        FawxSpacing.paddingXL
#else
        FawxSpacing.paddingLG
#endif
    }
}

private struct FleetNodeCard: View {
    let node: FleetNodeSummary

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(node.name)
                        .font(FawxTypography.heading2)
                        .foregroundStyle(Color.fawxText)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    Text("Heartbeat \(relativeTimestampString(node.lastSeenAt))")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                FleetStatusBadge(status: node.displayStatus)
            }

            if !node.capabilities.isEmpty {
                CapabilityChipGrid(capabilities: node.capabilities)
            }

            HStack(spacing: FawxSpacing.paddingSM) {
                FleetMetricPill(title: "Tasks", value: "\(node.activeTasks)")
                FleetMetricPill(title: "State", value: node.displayStatus.title)
            }
        }
        .padding(FawxSpacing.paddingLG)
        .frame(maxWidth: .infinity, minHeight: 170, alignment: .leading)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private struct FleetNodeDetailSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var isShowingRemoveAlert = false

    @Bindable var viewModel: FleetViewModel

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    if viewModel.isLoadingDetail && viewModel.selectedNodeDetail == nil {
                        ProgressView("Loading node details...")
                            .frame(maxWidth: .infinity, minHeight: 260)
                    } else if let errorMessage = viewModel.detailErrorMessage,
                              viewModel.selectedNodeDetail == nil {
                        FleetPlaceholderView(
                            title: "Could not load node",
                            message: errorMessage,
                            actionTitle: "Retry",
                            action: {
                                Task {
                                    await viewModel.refreshSelectedNode()
                                }
                            }
                        )
                    } else if let detail = viewModel.selectedNodeDetail {
                        detailCard(detail)
                        dispatchCard
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .navigationTitle(viewModel.selectedNodeDetail?.name ?? "Fleet Node")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        viewModel.closeDetail()
                        dismiss()
                    }
                }
            }
        }
        .frame(minWidth: 420, minHeight: 480)
        .alert("Remove this fleet node?", isPresented: $isShowingRemoveAlert) {
            Button("Remove", role: .destructive) {
                Task {
                    _ = await viewModel.removeSelectedNode()
                }
            }
            .disabled(viewModel.isRemovingNode)

            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This removes the node from the fleet and revokes its join token.")
        }
    }

    private func detailCard(_ detail: FleetNodeDetailResponse) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(detail.name)
                        .font(FawxTypography.heading1)
                        .foregroundStyle(Color.fawxText)

                    Text(verbatim: detail.endpoint)
                        .font(FawxTypography.code)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .textSelection(.enabled)
                        .privacySensitive()
                }

                Spacer(minLength: 0)

                FleetStatusBadge(status: detail.displayStatus)
            }

            settingsRow(label: "Last heartbeat", value: relativeTimestampString(detail.lastSeenAt))
            settingsRow(label: "Registered", value: absoluteTimestampString(detail.registeredAt))
            settingsRow(label: "Active tasks", value: "\(detail.activeTasks)")
            settingsRow(label: "Queued tasks", value: "\(detail.queuedTasks)")

            if !detail.capabilities.isEmpty {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    Text("Capabilities")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxTextSecondary)

                    CapabilityChipGrid(capabilities: detail.capabilities)
                }
            }

            Divider()
                .overlay(Color.fawxBorder)

            HStack {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text("Node Management")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    Text("Remove this node if it should no longer receive fleet work.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                Spacer(minLength: 0)

                Button(viewModel.isRemovingNode ? "Removing..." : "Remove Node", role: .destructive) {
                    isShowingRemoveAlert = true
                }
                .buttonStyle(.bordered)
                .disabled(viewModel.isRemovingNode)
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

    private var dispatchCard: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Dispatch Task")
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text("Send a short instruction to this worker node.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            TextField("Describe the task to dispatch", text: $viewModel.draftTaskDescription, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(3...6)

            if let errorMessage = viewModel.detailErrorMessage {
                Text(errorMessage)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxWarning)
            }

            HStack {
                Spacer(minLength: 0)

                Button(viewModel.isDispatchingTask ? "Dispatching..." : "Dispatch Task") {
                    Task {
                        await viewModel.dispatchTask()
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .disabled(viewModel.isDispatchingTask)
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

    private func settingsRow(label: String, value: String) -> some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            Text(label)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(width: 110, alignment: .leading)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)

            Spacer(minLength: 0)
        }
    }
}

private struct FleetStatusBadge: View {
    let status: FleetNodeDisplayStatus

    var body: some View {
        Text(status.title)
            .font(FawxTypography.status)
            .foregroundStyle(status.color)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, FawxSpacing.paddingXS)
            .background(status.color.opacity(0.14))
            .clipShape(Capsule())
    }
}

private struct CapabilityChipGrid: View {
    let capabilities: [String]

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 92), spacing: FawxSpacing.paddingSM)], alignment: .leading, spacing: FawxSpacing.paddingSM) {
            ForEach(capabilities, id: \.self) { capability in
                Text(humanReadableCapability(capability))
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxText)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .multilineTextAlignment(.center)
                    .background(Color.fawxAccentSubtle)
                    .clipShape(Capsule())
            }
        }
    }

    private func humanReadableCapability(_ capability: String) -> String {
        capability
            .replacingOccurrences(of: "_", with: " ")
            .split(separator: " ")
            .map { $0.capitalized }
            .joined(separator: " ")
    }
}

private struct FleetMetricPill: View {
    let title: String
    let value: String

    var body: some View {
        VStack(spacing: 2) {
            Text(title)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
        }
        .frame(maxWidth: .infinity, alignment: .center)
        .multilineTextAlignment(.center)
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

private struct FleetPlaceholderView: View {
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: "point.3.connected.trianglepath.dotted")
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Color.fawxTextSecondary)

            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 460)

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

private extension FleetNodeDisplayStatus {
    var color: Color {
        switch self {
        case .online:
            .fawxSuccess
        case .busy:
            .fawxAccent
        case .stale:
            .fawxWarning
        case .offline:
            .fawxTextSecondary
        }
    }
}

private func absoluteTimestampString(_ epochSeconds: Int) -> String {
    Date(timeIntervalSince1970: TimeInterval(epochSeconds)).formatted(date: .abbreviated, time: .shortened)
}
