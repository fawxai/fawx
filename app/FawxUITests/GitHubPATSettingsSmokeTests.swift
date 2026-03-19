import Foundation
import XCTest

final class GitHubPATSettingsSmokeTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testGitHubPATSectionAppearsInAuthenticationSettings() async throws {
        try await Self.requireReachableServer()

        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        openSettings(in: app)

        let authenticationButton = app.buttons["Authentication"]
        XCTAssertTrue(
            authenticationButton.waitForExistence(timeout: 5),
            "Expected the Authentication settings row to appear."
        )
        authenticationButton.tap()

        let githubCard = app.descendants(matching: .any)["authProvider_github"]
        XCTAssertTrue(
            githubCard.waitForExistence(timeout: 10),
            "Expected the GitHub PAT card to appear in Authentication settings."
        )

        let tokenField = app.secureTextFields["GitHub personal access token"]
        XCTAssertTrue(
            tokenField.waitForExistence(timeout: 5),
            "Expected the GitHub personal access token field to appear."
        )

        let saveButton = app.buttons["Save GitHub token"]
        XCTAssertTrue(
            saveButton.waitForExistence(timeout: 5),
            "Expected the Save GitHub token button to appear."
        )
        XCTAssertFalse(
            saveButton.isEnabled,
            "Expected the Save GitHub token button to start disabled with an empty field."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "github-pat-auth-settings"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private static func requireCredentials() throws -> (serverURL: String, bearerToken: String) {
        guard let serverURL = TestConfig.serverURL else {
            throw XCTSkip("Missing FAWX_TEST_SERVER_URL")
        }
        guard let bearerToken = TestConfig.bearerToken else {
            throw XCTSkip("Missing FAWX_TEST_BEARER_TOKEN")
        }
        return (serverURL, bearerToken)
    }

    private static func requireReachableServer() async throws {
        let (serverURL, bearerToken) = try requireCredentials()
        let healthPaths = ["/health", "/v1/health"]
        var failures: [String] = []

        for healthPath in healthPaths {
            let endpoint = try XCTUnwrap(URL(string: serverURL + healthPath))
            var request = URLRequest(url: endpoint)
            request.timeoutInterval = 5
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")

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
            "Authenticated UI tests require a reachable test server. Health checks failed: \(failures.joined(separator: "; "))."
        )
    }

    @MainActor
    private func openSettings(in app: XCUIApplication) {
        let sectionMenuButton = app.buttons["sectionMenuButton"]
        XCTAssertTrue(
            sectionMenuButton.waitForExistence(timeout: 5),
            "Expected the section menu button to appear in the authenticated app shell."
        )
        sectionMenuButton.tap()

        let settingsButton = app.buttons["Settings"]
        XCTAssertTrue(
            settingsButton.waitForExistence(timeout: 5),
            "Expected the Settings menu action to appear."
        )
        settingsButton.tap()

        let settingsTitle = app.navigationBars["Settings"].firstMatch
        XCTAssertTrue(
            settingsTitle.waitForExistence(timeout: 5),
            "Expected the Settings screen to open."
        )
    }
}
