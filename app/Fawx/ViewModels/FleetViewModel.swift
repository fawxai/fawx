import Foundation
import Observation

@MainActor
@Observable
final class FleetViewModel {
    var overview: FleetOverviewResponse?
    var nodes: [FleetNodeSummary] = []
    var isLoading = false
    var errorMessage: String?

    var selectedNodeID: String?
    var selectedNodeDetail: FleetNodeDetailResponse?
    var isLoadingDetail = false
    var detailErrorMessage: String?
    var draftTaskDescription = ""
    var isDispatchingTask = false

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var summaryLine: String {
        guard let overview else {
            return "Fleet"
        }

        var fragments: [String] = []
        if overview.healthyNodes > 0 {
            fragments.append("\(overview.healthyNodes) online")
        }
        if overview.degradedNodes > 0 {
            fragments.append("\(overview.degradedNodes) stale")
        }
        if overview.offlineNodes > 0 {
            fragments.append("\(overview.offlineNodes) offline")
        }

        if fragments.isEmpty {
            fragments.append("0 online")
        }

        return "\(overview.totalNodes) nodes (\(fragments.joined(separator: ", ")))"
    }

    func refresh() async {
        guard appState.isConfigured else {
            overview = nil
            nodes = []
            errorMessage = nil
            closeDetail()
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            async let overviewTask = appState.client.fleetOverview()
            async let nodesTask = appState.client.fleetNodes()

            let (latestOverview, latestNodes) = try await (overviewTask, nodesTask)
            overview = latestOverview
            nodes = latestNodes.nodes.sorted { lhs, rhs in
                let leftPriority = lhs.displayStatus.priority
                let rightPriority = rhs.displayStatus.priority
                if leftPriority != rightPriority {
                    return leftPriority < rightPriority
                }
                return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
            errorMessage = nil

            if selectedNodeID != nil {
                await refreshSelectedNode()
            }
        } catch {
            if overview == nil {
                nodes = []
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func presentNode(_ node: FleetNodeSummary) async {
        selectedNodeID = node.id
        selectedNodeDetail = nil
        detailErrorMessage = nil
        draftTaskDescription = ""
        await refreshSelectedNode()
    }

    func refreshSelectedNode() async {
        guard let selectedNodeID else {
            return
        }

        guard !isLoadingDetail else {
            return
        }

        isLoadingDetail = true
        defer { isLoadingDetail = false }

        do {
            selectedNodeDetail = try await appState.client.fleetNode(id: selectedNodeID)
            detailErrorMessage = nil
        } catch {
            if selectedNodeDetail == nil {
                selectedNodeDetail = nil
            }
            detailErrorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func dispatchTask() async {
        guard let selectedNodeID else {
            return
        }

        let trimmedTask = draftTaskDescription.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedTask.isEmpty else {
            detailErrorMessage = "Enter a task before dispatching."
            return
        }

        guard !isDispatchingTask else {
            return
        }

        isDispatchingTask = true
        defer { isDispatchingTask = false }

        do {
            let response = try await appState.client.dispatchFleetTask(nodeID: selectedNodeID, task: trimmedTask)
            detailErrorMessage = nil
            draftTaskDescription = ""
            appState.showToast(
                message: response.accepted
                    ? "Dispatched task \(response.taskID) to \(selectedNodeDetail?.name ?? "node")."
                    : "The node did not accept the task.",
                style: response.accepted ? .success : .warning
            )
            await refreshSelectedNode()
        } catch {
            detailErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func closeDetail() {
        selectedNodeID = nil
        selectedNodeDetail = nil
        isLoadingDetail = false
        detailErrorMessage = nil
        draftTaskDescription = ""
        isDispatchingTask = false
    }
}
