import XCTest

final class PairingFlowTests: XCTestCase {
    private let pairingCodeFilePath = "/tmp/fawx_test_pairing_code.txt"

    private var serverURL: String {
        if let value = ProcessInfo.processInfo.environment["FAWX_TEST_SERVER_URL"], value.isEmpty == false {
            return value
        }
        if let value = try? String(contentsOfFile: "/tmp/fawx_test_server_url.txt", encoding: .utf8) {
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty == false {
                return trimmed
            }
        }
        return "http://100.123.20.63:8400"
    }

    private var pairingCode: String? {
        if let value = ProcessInfo.processInfo.environment["FAWX_TEST_PAIRING_CODE"], value.isEmpty == false {
            return value
        }

        guard let attributes = try? FileManager.default.attributesOfItem(atPath: pairingCodeFilePath),
              let modifiedAt = attributes[.modificationDate] as? Date,
              Date().timeIntervalSince(modifiedAt) <= 300,
              let value = try? String(contentsOfFile: pairingCodeFilePath, encoding: .utf8)
        else {
            return nil
        }

        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testPairingCodeCompletesOnboarding() throws {
        guard let pairingCode else {
            throw XCTSkip("Set a fresh FAWX_TEST_PAIRING_CODE or update /tmp/fawx_test_pairing_code.txt within the last 5 minutes to run pairing tests.")
        }

        let app = TestConfig.makeApp(resetState: true, includeCredentials: false)
        app.launch()

        let serverField = app.descendants(matching: .any)["serverURLField"]
        XCTAssertTrue(serverField.waitForExistence(timeout: 5), "Expected the server URL field to appear.")
        serverField.tap()
        serverField.typeText(serverURL)
        serverField.typeText("\n")

        let healthButton = app.buttons["testConnectionButton"]
        XCTAssertTrue(healthButton.waitForExistence(timeout: 3), "Expected the health check button to appear.")

        let continueButton = app.buttons["continueButton"]
        if continueButton.isEnabled == false {
            if let dismissButton = app.keyboards.buttons.allElementsBoundByIndex.first(where: { $0.label.lowercased() == "done" || $0.label.lowercased() == "return" }) {
                dismissButton.tap()
            }

            if continueButton.isEnabled == false {
                healthButton.tap()
            }
        }

        XCTAssertTrue(continueButton.waitForExistence(timeout: 3), "Expected the continue button to appear.")

        let continueEnabled = NSPredicate(format: "isEnabled == true")
        let continueExpectation = XCTNSPredicateExpectation(predicate: continueEnabled, object: continueButton)
        XCTAssertEqual(
            XCTWaiter.wait(for: [continueExpectation], timeout: 10),
            .completed,
            "Expected the continue button to enable after a successful health check."
        )
        continueButton.tap()

        let codeField = app.descendants(matching: .any)["bearerTokenField"]
        XCTAssertTrue(codeField.waitForExistence(timeout: 5), "Expected the pairing code field to appear.")
        codeField.tap()
        codeField.typeText(pairingCode)

        let pairButton = app.buttons["Pair Device"]
        XCTAssertTrue(pairButton.waitForExistence(timeout: 3), "Expected the pair button to appear.")
        pairButton.tap()

        let sessionList = app.descendants(matching: .any)["sessionList"]
        XCTAssertTrue(
            sessionList.waitForExistence(timeout: 15),
            "Expected the app to transition to the main session list after pairing."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "pairing-flow-success"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
