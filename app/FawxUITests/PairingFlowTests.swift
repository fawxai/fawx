import Foundation
import XCTest

final class PairingFlowTests: XCTestCase {
    private struct GeneratedPairingCode: Decodable {
        let code: String
    }

    private let pairingCodeFilePath = "/tmp/fawx_test_pairing_code.txt"

    private var serverURL: String {
        TestConfig.serverURL ?? "http://127.0.0.1:8400"
    }

    private var explicitPairingCodeOverride: String? {
        if let value = ProcessInfo.processInfo.environment["FAWX_TEST_PAIRING_CODE"], value.isEmpty == false {
            return value
        }
        return nil
    }

    private var recentPairingCode: String? {
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
    func testPairingCodeCompletesOnboarding() async throws {
        try await Self.requireReachableServer()
        let pairingCode = try await resolvedPairingCode()

        let app = TestConfig.makeApp(resetState: true, includeCredentials: false)
        app.launch()
        TestConfig.openRemoteOnboardingIfNeeded(in: app)

        let serverField = app.descendants(matching: .any)["serverURLField"]
        XCTAssertTrue(serverField.waitForExistence(timeout: 10), "Expected the server URL field to appear.")
        serverField.tap()
        serverField.typeText(serverURL)

        let healthButton = app.buttons["testConnectionButton"]
        XCTAssertTrue(healthButton.waitForExistence(timeout: 5), "Expected the health check button to appear.")

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
        let continueDeadline = ContinuousClock.now.advanced(by: .seconds(10))
        while !continueButton.isEnabled && ContinuousClock.now < continueDeadline {
            try? await Task.sleep(for: .milliseconds(100))
        }
        continueButton.tap()

        let codeField = app.descendants(matching: .any)["bearerTokenField"]
        XCTAssertTrue(codeField.waitForExistence(timeout: 10), "Expected the pairing code field to appear.")
        codeField.tap()
        codeField.typeText(pairingCode)

        let pairButton = app.buttons["Pair Device"]
        XCTAssertTrue(pairButton.waitForExistence(timeout: 3), "Expected the pair button to appear.")
        pairButton.tap()

        let messageInput = app.descendants(matching: .any)["messageInput"]
        let sessionList = app.descendants(matching: .any)["sessionList"]
        XCTAssertTrue(
            messageInput.waitForExistence(timeout: 15) || sessionList.waitForExistence(timeout: 5),
            "Expected the app to transition into the main chat experience after pairing."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "pairing-flow-success"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    @MainActor
    private func resolvedPairingCode() async throws -> String {
        if let explicitPairingCodeOverride {
            return explicitPairingCodeOverride
        }

        do {
            if let generatedCode = try await generatePairingCode() {
                persistPairingCode(generatedCode)
                return generatedCode
            }
        } catch {
            if let recentPairingCode {
                return recentPairingCode
            }
            throw error
        }

        if let recentPairingCode {
            return recentPairingCode
        }

        throw XCTSkip(
            "Set FAWX_TEST_BEARER_TOKEN to auto-generate a code, or provide FAWX_TEST_PAIRING_CODE, or update /tmp/fawx_test_pairing_code.txt within the last 5 minutes."
        )
    }

    @MainActor
    private func generatePairingCode() async throws -> String? {
        guard let bearerToken = TestConfig.bearerToken else {
            return nil
        }

        let endpoint = try XCTUnwrap(URL(string: serverURL + "/v1/pair/generate"))
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.timeoutInterval = 10
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONSerialization.data(withJSONObject: [:])

        let (data, response) = try await URLSession.shared.data(for: request)
        let httpResponse = try XCTUnwrap(response as? HTTPURLResponse)
        guard (200 ... 299).contains(httpResponse.statusCode) else {
            let responseBody = String(data: data, encoding: .utf8) ?? "<non-UTF8 response>"
            throw XCTSkip(
                "Unable to auto-generate a pairing code. /v1/pair/generate returned \(httpResponse.statusCode): \(responseBody)"
            )
        }

        let generated = try JSONDecoder().decode(GeneratedPairingCode.self, from: data)
        return generated.code
    }

    @MainActor
    private func persistPairingCode(_ code: String) {
        try? "\(code)\n".write(toFile: pairingCodeFilePath, atomically: true, encoding: .utf8)
    }

    private static func requireReachableServer() async throws {
        guard let serverURL = TestConfig.serverURL else {
            throw XCTSkip("Missing FAWX_TEST_SERVER_URL")
        }

        let healthPaths = ["/health", "/v1/health"]
        var failures: [String] = []

        for healthPath in healthPaths {
            let endpoint = try XCTUnwrap(URL(string: serverURL + healthPath))
            var request = URLRequest(url: endpoint)
            request.timeoutInterval = 5

            if let bearerToken = TestConfig.bearerToken {
                request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
            }

            do {
                let (_, response) = try await URLSession.shared.data(for: request)
                let httpResponse = try XCTUnwrap(response as? HTTPURLResponse)
                if (200 ... 299).contains(httpResponse.statusCode) {
                    return
                }
                failures.append("\(healthPath) returned \(httpResponse.statusCode)")
            } catch {
                failures.append("\(healthPath) failed: \(error.localizedDescription)")
            }
        }

        throw XCTSkip(
            "Pairing UI tests require a reachable test server. Health checks failed: \(failures.joined(separator: "; "))."
        )
    }
}
