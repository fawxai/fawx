import XCTest
@testable import Fawx

@MainActor
final class ExperimentsViewModelTests: XCTestCase {
    func testRefreshSelectedExperimentKeepsDetailWhenResultsRequestFails() async {
        let detail = ExperimentDetail(
            id: "exp-1",
            name: "Evaluate branch quality",
            kind: .tournament,
            status: .completed,
            config: ExperimentConfig(population: 4, rounds: 2, minConfidence: "0.8", outputMode: "ranked"),
            createdAt: 100,
            startedAt: 120,
            completedAt: 180,
            fleetNodes: ["node-a", "node-b"],
            progress: ExperimentProgress(completedMatches: 4, totalMatches: 4),
            result: nil,
            error: nil
        )

        let sut = ExperimentsViewModel(
            appState: AppState(),
            fetchExperimentDetail: { _ in detail },
            fetchExperimentResults: { _ in throw APIError.httpStatus(404, "Results are not ready yet.") },
            stopExperimentRequest: { id in StopExperimentResponse(id: id, stopping: true) }
        )

        sut.selectedExperimentID = detail.id

        await sut.refreshSelectedExperiment()

        XCTAssertEqual(sut.selectedExperiment, detail)
        XCTAssertNil(sut.selectedResults)
        XCTAssertNil(sut.detailErrorMessage)
        XCTAssertEqual(sut.resultsErrorMessage, "Results are not ready yet.")
    }
}
