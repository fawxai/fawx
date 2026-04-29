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
                  "timestamp": {
                    "secs_since_epoch": 1742187600,
                    "nanos_since_epoch": 0
                  },
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

        XCTAssertEqual(entry.timestamp.timeIntervalSince1970, 1_742_187_600, accuracy: 0.001)
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
                  "timestamp": {
                    "secs_since_epoch": 1742187660,
                    "nanos_since_epoch": 0
                  },
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

        XCTAssertEqual(entry.timestamp.timeIntervalSince1970, 1_742_187_660, accuracy: 0.001)
        XCTAssertEqual(entry.actionSummary, "POST api.example.com/v1/runs")
        XCTAssertEqual(entry.actionContext, "Status 202")
        XCTAssertFalse(entry.displayTime.isEmpty)
        XCTAssertTrue(entry.metadataLabels.contains("Audit only"))
        XCTAssertTrue(entry.metadataLabels.contains("POST"))
    }

    func testRipcordStatusDecodesSystemTimestamp() throws {
        let data = Data(
            """
            {
              "active": true,
              "tripwire_id": "credential_read",
              "tripwire_description": "Credentials touched",
              "activated_at": {
                "secs_since_epoch": 1742187600,
                "nanos_since_epoch": 250000000
              },
              "entry_count": 3
            }
            """.utf8
        )

        let status = try JSONDecoder().decode(RipcordStatusResponse.self, from: data)
        let activatedAt = try XCTUnwrap(status.activatedAt)

        XCTAssertTrue(status.active)
        XCTAssertEqual(activatedAt.timeIntervalSince1970, 1_742_187_600.25, accuracy: 0.000_001)
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
