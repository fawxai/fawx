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
    var resultsErrorMessage: String?
    var isStoppingExperiment = false

    private let appState: AppState
    private let fetchExperimentDetail: @Sendable (String) async throws -> ExperimentDetail
    private let fetchExperimentResults: @Sendable (String) async throws -> ExperimentResultsResponse
    private let stopExperimentRequest: @Sendable (String) async throws -> StopExperimentResponse

    init(
        appState: AppState,
        fetchExperimentDetail: (@Sendable (String) async throws -> ExperimentDetail)? = nil,
        fetchExperimentResults: (@Sendable (String) async throws -> ExperimentResultsResponse)? = nil,
        stopExperimentRequest: (@Sendable (String) async throws -> StopExperimentResponse)? = nil
    ) {
        self.appState = appState
        self.fetchExperimentDetail = fetchExperimentDetail ?? { [client = appState.client] id in
            try await client.experiment(id: id)
        }
        self.fetchExperimentResults = fetchExperimentResults ?? { [client = appState.client] id in
            try await client.experimentResults(id: id)
        }
        self.stopExperimentRequest = stopExperimentRequest ?? { [client = appState.client] id in
            try await client.stopExperiment(id: id)
        }
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
        resultsErrorMessage = nil
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

        let requestedExperimentID = selectedExperimentID

        do {
            let detail = try await fetchExperimentDetail(requestedExperimentID)
            guard self.selectedExperimentID == requestedExperimentID else {
                return
            }
            selectedExperiment = detail
            detailErrorMessage = nil
        } catch {
            guard self.selectedExperimentID == requestedExperimentID else {
                return
            }
            if selectedExperiment == nil {
                selectedExperiment = nil
                selectedResults = nil
                resultsErrorMessage = nil
            }
            detailErrorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
            return
        }

        do {
            let results = try await fetchExperimentResults(requestedExperimentID)
            guard self.selectedExperimentID == requestedExperimentID else {
                return
            }
            selectedResults = results
            resultsErrorMessage = nil
        } catch {
            guard self.selectedExperimentID == requestedExperimentID else {
                return
            }
            selectedResults = nil

            if let apiError = error as? APIError,
               apiError.statusCode == 404,
               let detail = selectedExperiment,
               detail.status == .queued || detail.status == .running {
                resultsErrorMessage = nil
            } else {
                resultsErrorMessage = error.localizedDescription
            }
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
            let response = try await stopExperimentRequest(selectedExperimentID)
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
        resultsErrorMessage = nil
        isStoppingExperiment = false
    }
}
