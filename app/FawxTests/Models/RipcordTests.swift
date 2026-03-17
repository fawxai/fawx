import XCTest
@testable import Fawx

final class RipcordTests: XCTestCase {
    func testJournalEntryDecodesFileWriteSummaryAndMetadata() throws {
        let data = Data(
            """
            {
              "entries": [
                {
                  "id": 7,
                  "timestamp": "2026-03-17T05:00:00Z",
                  "tool_name": "write_file",
                  "tool_call_id": "call_123",
                  "action": {
                    "type": "file_write",
                    "path": "src/main.rs",
                    "snapshot_hash": "abc123",
                    "size_bytes": 1024,
                    "created": false
                  },
                  "reversible": true
                }
              ]
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(RipcordJournalResponse.self, from: data)
        let entry = try XCTUnwrap(response.entries.first)

        XCTAssertEqual(entry.actionSummary, "src/main.rs")
        XCTAssertEqual(entry.actionContext, "Snapshot abc123")
        XCTAssertFalse(entry.displayTime.isEmpty)
        XCTAssertTrue(entry.metadataLabels.contains("Reversible"))
        XCTAssertTrue(entry.metadataLabels.contains("1 KB"))
    }

    func testJournalEntryDecodesNetworkRequestSummary() throws {
        let data = Data(
            """
            {
              "entries": [
                {
                  "id": 2,
                  "timestamp": "2026-03-17T05:01:00Z",
                  "tool_name": "fetch",
                  "tool_call_id": "call_456",
                  "action": {
                    "type": "network_request",
                    "url": "https://api.example.com/v1/runs",
                    "method": "post",
                    "status_code": 202
                  },
                  "reversible": false
                }
              ]
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(RipcordJournalResponse.self, from: data)
        let entry = try XCTUnwrap(response.entries.first)

        XCTAssertEqual(entry.actionSummary, "POST api.example.com/v1/runs")
        XCTAssertEqual(entry.actionContext, "Status 202")
        XCTAssertFalse(entry.displayTime.isEmpty)
        XCTAssertTrue(entry.metadataLabels.contains("Audit only"))
        XCTAssertTrue(entry.metadataLabels.contains("POST"))
    }

    func testRipcordStatusFallsBackToGenericDescription() {
        let status = RipcordStatusResponse(
            active: true,
            tripwireId: "credential_read",
            tripwireDescription: "   ",
            activatedAt: nil,
            entryCount: 3
        )

        XCTAssertEqual(status.displayDescription, "Tripwire crossed")
        XCTAssertEqual(status.entryCountLabel, "3 actions journaled")
    }
}
