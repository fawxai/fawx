import XCTest
@testable import Fawx

final class FleetTests: XCTestCase {
    func testFleetRemoveNodeResponseDecodesRemovalState() throws {
        let data = Data(
            """
            {
              "id": "node-123",
              "removed": true
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(FleetRemoveNodeResponse.self, from: data)

        XCTAssertEqual(response.id, "node-123")
        XCTAssertTrue(response.removed)
    }
}
