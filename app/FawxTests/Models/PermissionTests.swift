import XCTest
@testable import Fawx

final class PermissionTests: XCTestCase {
    func testPermissionsResponseDefaultsModeToPromptWhenMissing() throws {
        let data = Data(
            """
            {
              "preset": "power",
              "permissions": [
                { "action": "shell", "level": "propose", "title": "Shell Commands" }
              ],
              "available_presets": ["power", "cautious", "experimental", "custom"]
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(PermissionsResponse.self, from: data)

        XCTAssertEqual(response.mode, .prompt)
        XCTAssertEqual(response.permissions.first?.level, "propose")
    }

    func testPermissionsResponseDecodesCapabilityMode() throws {
        let data = Data(
            """
            {
              "preset": "power",
              "mode": "capability",
              "permissions": [
                { "action": "shell", "level": "denied", "title": "Shell Commands" }
              ],
              "available_presets": ["power", "cautious", "experimental", "custom"]
            }
            """.utf8
        )

        let response = try JSONDecoder().decode(PermissionsResponse.self, from: data)

        XCTAssertEqual(response.mode, .capability)
        XCTAssertEqual(response.permissions.first?.level, "denied")
    }

    func testLegacyCompatibleRequestMapsAskToPropose() {
        let request = PermissionsPatchRequest(
            preset: nil,
            mode: .prompt,
            changes: [PermissionChange(action: "shell", level: "ask")]
        )

        let legacyRequest = request.legacyCompatibleRequest

        XCTAssertEqual(legacyRequest?.mode, .prompt)
        XCTAssertEqual(legacyRequest?.changes?.first?.action, "shell")
        XCTAssertEqual(legacyRequest?.changes?.first?.level, "propose")
    }
}
