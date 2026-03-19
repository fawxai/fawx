import XCTest
@testable import Fawx

@MainActor
final class PermissionsViewModelTests: XCTestCase {
    func testSetModeRollsBackOptimisticUpdateWhenRequestFails() async {
        let appState = AppState()
        let sut = PermissionsViewModel(appState: appState)

        XCTAssertEqual(sut.permissionMode, .prompt)
        XCTAssertEqual(appState.permissionMode, .prompt)

        await sut.setMode(.capability)

        XCTAssertEqual(sut.permissionMode, .prompt)
        XCTAssertEqual(appState.permissionMode, .prompt)
        XCTAssertEqual(sut.errorMessage, APIError.notConfigured.localizedDescription)
    }
}
