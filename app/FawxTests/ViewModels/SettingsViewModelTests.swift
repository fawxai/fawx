import XCTest
@testable import Fawx

@MainActor
final class SettingsViewModelTests: XCTestCase {
    func testParseScannedConnectionUsesHTTPSForTailscaleTransportHint() {
        let connection = SettingsViewModel.parseScannedConnection(
            "fawx://connect?host=100.93.251.101&port=8400&transport=tailscale_https&token=REDACTED"
        )

        XCTAssertEqual(connection?.serverURL, "https://100.93.251.101:8400")
        XCTAssertNil(connection?.token)
    }

    func testParseScannedConnectionUsesHTTPForLanTransportHint() {
        let connection = SettingsViewModel.parseScannedConnection(
            "fawx://connect?host=192.168.1.10&port=8400&transport=lan_http&token=test-token"
        )

        XCTAssertEqual(connection?.serverURL, "http://192.168.1.10:8400")
        XCTAssertEqual(connection?.token, "test-token")
    }
}
