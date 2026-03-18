import Foundation
import XCTest
@testable import Fawx

final class FawxClientFleetTests: XCTestCase {
    func testRemoveFleetNodeBuildsDeleteRequestWithEncodedNodeID() async throws {
        let client = FawxClient(
            baseURL: URL(string: "http://localhost:8400"),
            bearerToken: "test-token"
        )

        let request = try await client.removeFleetNodeRequestForTesting(id: "node/a b")
        let components = try XCTUnwrap(request.url.flatMap {
            URLComponents(url: $0, resolvingAgainstBaseURL: false)
        })

        XCTAssertEqual(request.httpMethod, "DELETE")
        XCTAssertEqual(components.percentEncodedPath, "/v1/fleet/nodes/node%2Fa%20b")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-token")
    }
}
