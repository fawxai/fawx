import XCTest
@testable import Fawx

final class LocalBootstrapServiceTests: XCTestCase {
    func testBootstrapResultDecodesValidJSON() throws {
        let json = """
        {
          "port": 8400,
          "host": "127.0.0.1",
          "bearer_token": "secret-token",
          "data_dir": "/Users/test/.fawx",
          "config_path": "/Users/test/.fawx/config.toml",
          "created": true
        }
        """

        let result = try JSONDecoder().decode(BootstrapResult.self, from: Data(json.utf8))

        XCTAssertEqual(result.port, 8400)
        XCTAssertEqual(result.host, "127.0.0.1")
        XCTAssertEqual(result.bearerToken, "secret-token")
        XCTAssertEqual(result.dataDir, "/Users/test/.fawx")
        XCTAssertEqual(result.configPath, "/Users/test/.fawx/config.toml")
        XCTAssertTrue(result.created)
    }

    func testBootstrapErrorDecodesErrorJSON() throws {
        let json = """
        {
          "error": "All ports 8400-8410 are in use",
          "port_range": [8400, 8410]
        }
        """

        let error = try JSONDecoder().decode(BootstrapError.self, from: Data(json.utf8))

        XCTAssertEqual(error.error, "All ports 8400-8410 are in use")
        XCTAssertEqual(error.portRange, [8400, 8410])
    }

    func testBootstrapErrorDecodesNilPortRange() throws {
        let json = """
        {
          "error": "No open ports were available",
          "port_range": null
        }
        """

        let error = try JSONDecoder().decode(BootstrapError.self, from: Data(json.utf8))

        XCTAssertEqual(error.error, "No open ports were available")
        XCTAssertNil(error.portRange)
    }

    func testProcessExitedWithErrorUsesFallbackMessageWhenEmpty() {
        let error = LocalBootstrapService.BootstrapFailure.processExitedWithError(code: 23, message: "")

        XCTAssertEqual(error.errorDescription, "Fawx setup couldn't finish (exit code 23).")
    }

    func testXmlEscapeHandlesSpecialCharacters() {
        let escaped = LocalBootstrapService.xmlEscape("/Applications/Fawx & Friends/fawx<beta>")
        XCTAssertEqual(escaped, "/Applications/Fawx &amp; Friends/fawx&lt;beta&gt;")
    }

    func testGeneratePlistContainsExpectedFields() {
        let plist = LocalBootstrapService.generatePlist(
            binaryPath: "/Applications/Fawx.app/Contents/MacOS/fawx-server",
            port: 8400,
            dataDir: "/Users/test/.fawx",
            logPath: "/Users/test/Library/Logs/Fawx/server.log"
        )

        XCTAssertTrue(plist.contains("<string>ai.fawx.server</string>"))
        XCTAssertTrue(plist.contains("<string>/Applications/Fawx.app/Contents/MacOS/fawx-server</string>"))
        XCTAssertTrue(plist.contains("<string>8400</string>"))
        XCTAssertTrue(plist.contains("<string>/Users/test/.fawx</string>"))
        XCTAssertTrue(plist.contains("<string>/Users/test/Library/Logs/Fawx/server.log</string>"))
    }
}
