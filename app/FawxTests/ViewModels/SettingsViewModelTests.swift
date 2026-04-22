import XCTest
@testable import Fawx

final class SettingsViewModelTests: XCTestCase {
    func testParseScannedConnectionUsesHTTPSForTailscaleTransportHint() {
        let connection = SettingsViewModel.parseScannedConnection(
            // RFC 5737 TEST-NET-3 keeps this fixture concrete without implying a real tailnet host.
            "fawx://connect?host=203.0.113.24&port=8400&transport=tailscale_https&token=REDACTED"
        )

        XCTAssertEqual(connection?.serverURL, "https://203.0.113.24:8400")
        XCTAssertNil(connection?.token)
    }

    func testParseScannedConnectionUsesHTTPForLanTransportHint() {
        let connection = SettingsViewModel.parseScannedConnection(
            "fawx://connect?host=pairing-host.local&port=8400&transport=lan_http&token=test-token"
        )

        XCTAssertEqual(connection?.serverURL, "http://pairing-host.local:8400")
        XCTAssertEqual(connection?.token, "test-token")
    }

    func testParseScannedConnectionKeepsLoopbackConnectionsOnHTTPWithoutTransportHint() {
        let connection = SettingsViewModel.parseScannedConnection(
            // Omitting the transport hint is intentional: loopback should still default to HTTP.
            "fawx://connect?host=127.0.0.1&port=8400&token=local-token"
        )

        XCTAssertEqual(connection?.serverURL, "http://127.0.0.1:8400")
        XCTAssertEqual(connection?.token, "local-token")
    }
}
