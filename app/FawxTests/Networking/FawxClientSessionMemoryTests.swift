import Foundation
import XCTest
@testable import Fawx

final class FawxClientSessionMemoryTests: XCTestCase {
    func testSessionMemoryBuildsGetRequest() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let request = try await client.sessionMemoryRequestForTesting(id: "session-123")

        XCTAssertEqual(request.httpMethod, "GET")
        XCTAssertEqual(request.url?.path, "/v1/sessions/session-123/memory")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
    }

    func testUpdateSessionMemoryBuildsPutRequest() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )
        let memory = SessionMemory(
            project: "Compaction UX",
            currentState: "Adding the session memory panel",
            keyDecisions: ["Expose memory in a sheet"],
            activeFiles: ["app/Fawx/Views/Shared/SessionMemoryPanel.swift"],
            customContext: ["Gracefully handle old servers"],
            lastUpdated: 123
        )

        let request = try await client.updateSessionMemoryRequestForTesting(
            id: "session-123",
            memory: memory
        )
        let body = try XCTUnwrap(request.httpBody)
        let payload = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: body) as? [String: Any]
        )

        XCTAssertEqual(request.httpMethod, "PUT")
        XCTAssertEqual(request.url?.path, "/v1/sessions/session-123/memory")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
        XCTAssertEqual(payload["project"] as? String, "Compaction UX")
        XCTAssertEqual(payload["current_state"] as? String, "Adding the session memory panel")
        XCTAssertEqual(payload["key_decisions"] as? [String], ["Expose memory in a sheet"])
        XCTAssertEqual(
            payload["active_files"] as? [String],
            ["app/Fawx/Views/Shared/SessionMemoryPanel.swift"]
        )
        XCTAssertEqual(payload["custom_context"] as? [String], ["Gracefully handle old servers"])
        XCTAssertEqual(payload["last_updated"] as? Int, 123)
    }
}
