import XCTest
@testable import Fawx

@MainActor
final class FleetViewModelTests: XCTestCase {
    func testRemoveSelectedNodeClearsSelectionAfterSuccessfulRemoval() async {
        let appState = AppState()
        let sut = FleetViewModel(
            appState: appState,
            removeNodeRequest: { id in
                XCTAssertEqual(id, "node-1")
                return FleetRemoveNodeResponse(id: id, removed: true)
            }
        )

        sut.nodes = [makeNodeSummary(id: "node-1", name: "MacBook Pro")]
        sut.selectedNodeID = "node-1"
        sut.selectedNodeDetail = makeNodeDetail(id: "node-1", name: "MacBook Pro")

        let removed = await sut.removeSelectedNode()

        XCTAssertTrue(removed)
        XCTAssertNil(sut.selectedNodeID)
        XCTAssertNil(sut.selectedNodeDetail)
        XCTAssertNil(sut.detailErrorMessage)
        XCTAssertEqual(appState.toast?.message, "Removed MacBook Pro from fleet.")
        XCTAssertEqual(appState.toast?.style, .info)
    }

    func testRemoveSelectedNodeKeepsSelectionWhenServerDoesNotRemoveNode() async {
        let appState = AppState()
        let sut = FleetViewModel(
            appState: appState,
            removeNodeRequest: { id in
                FleetRemoveNodeResponse(id: id, removed: false)
            }
        )

        sut.nodes = [makeNodeSummary(id: "node-1", name: "MacBook Pro")]
        sut.selectedNodeID = "node-1"
        sut.selectedNodeDetail = makeNodeDetail(id: "node-1", name: "MacBook Pro")

        let removed = await sut.removeSelectedNode()

        XCTAssertFalse(removed)
        XCTAssertEqual(sut.selectedNodeID, "node-1")
        XCTAssertEqual(sut.selectedNodeDetail?.id, "node-1")
        XCTAssertEqual(sut.detailErrorMessage, "The node could not be removed.")
        XCTAssertEqual(appState.toast?.message, "Could not remove MacBook Pro.")
        XCTAssertEqual(appState.toast?.style, .warning)
    }

    func testRemoveSelectedNodePreservesSelectionWhenRequestThrows() async {
        struct TestError: LocalizedError {
            var errorDescription: String? {
                "Network unavailable"
            }
        }

        let appState = AppState()
        let sut = FleetViewModel(
            appState: appState,
            removeNodeRequest: { _ in
                throw TestError()
            }
        )

        sut.nodes = [makeNodeSummary(id: "node-1", name: "MacBook Pro")]
        sut.selectedNodeID = "node-1"
        sut.selectedNodeDetail = makeNodeDetail(id: "node-1", name: "MacBook Pro")

        let removed = await sut.removeSelectedNode()

        XCTAssertFalse(removed)
        XCTAssertEqual(sut.selectedNodeID, "node-1")
        XCTAssertEqual(sut.selectedNodeDetail?.id, "node-1")
        XCTAssertEqual(sut.detailErrorMessage, "Network unavailable")
        XCTAssertEqual(appState.toast?.message, "Network unavailable")
        XCTAssertEqual(appState.toast?.style, .error)
    }

    private func makeNodeSummary(id: String, name: String) -> FleetNodeSummary {
        FleetNodeSummary(
            id: id,
            name: name,
            status: .healthy,
            lastSeenAt: 1_742_000_000,
            activeTasks: 0,
            capabilities: ["Agentic Loop"]
        )
    }

    private func makeNodeDetail(id: String, name: String) -> FleetNodeDetailResponse {
        FleetNodeDetailResponse(
            id: id,
            name: name,
            status: .healthy,
            lastSeenAt: 1_742_000_000,
            activeTasks: 0,
            queuedTasks: 0,
            capabilities: ["Agentic Loop"],
            endpoint: "http://127.0.0.1:8400",
            registeredAt: 1_742_000_000
        )
    }
}
