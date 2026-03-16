import Foundation
import Observation

@MainActor
@Observable
final class ExperimentsViewModel {
    var experiments: [ExperimentSummary] = []
    var isLoading = false
    var errorMessage: String?

    var selectedExperimentID: String?
    var selectedExperiment: ExperimentDetail?
    var selectedResults: ExperimentResultsResponse?
    var isLoadingDetail = false
    var detailErrorMessage: String?
    var isStoppingExperiment = false

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    func refresh() async {
        guard appState.isConfigured else {
            experiments = []
            errorMessage = nil
            clearSelection()
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            let response = try await appState.client.experiments()
            experiments = response.experiments.sorted { lhs, rhs in
                if lhs.createdAt != rhs.createdAt {
                    return lhs.createdAt > rhs.createdAt
                }
                return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
            errorMessage = nil

            if selectedExperimentID != nil {
                await refreshSelectedExperiment()
            }
        } catch {
            if experiments.isEmpty {
                experiments = []
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func selectExperiment(_ experiment: ExperimentSummary) async {
        selectedExperimentID = experiment.id
        selectedExperiment = nil
        selectedResults = nil
        detailErrorMessage = nil
        await refreshSelectedExperiment()
    }

    func refreshSelectedExperiment() async {
        guard let selectedExperimentID else {
            return
        }

        guard !isLoadingDetail else {
            return
        }

        isLoadingDetail = true
        defer { isLoadingDetail = false }

        do {
            async let detailTask = appState.client.experiment(id: selectedExperimentID)
            async let resultsTask = appState.client.experimentResults(id: selectedExperimentID)

            selectedExperiment = try await detailTask
            selectedResults = try await resultsTask
            detailErrorMessage = nil
        } catch {
            if selectedExperiment == nil {
                selectedExperiment = nil
                selectedResults = nil
            }
            detailErrorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func stopSelectedExperiment() async {
        guard let selectedExperimentID else {
            return
        }

        guard !isStoppingExperiment else {
            return
        }

        isStoppingExperiment = true
        defer { isStoppingExperiment = false }

        do {
            let response = try await appState.client.stopExperiment(id: selectedExperimentID)
            appState.showToast(
                message: response.stopping ? "Stopping experiment \(selectedExperiment?.name ?? response.id)." : "Experiment stop request was ignored.",
                style: response.stopping ? .warning : .info
            )
            await refresh()
        } catch {
            detailErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func clearSelection() {
        selectedExperimentID = nil
        selectedExperiment = nil
        selectedResults = nil
        isLoadingDetail = false
        detailErrorMessage = nil
        isStoppingExperiment = false
    }
}
