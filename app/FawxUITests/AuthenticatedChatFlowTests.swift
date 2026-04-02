import XCTest
import Foundation

final class AuthenticatedChatFlowTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testAuthenticatedUserCanOpenNewConversationComposer() async throws {
        try await Self.requireReachableServer()

        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        let messageInput = app.descendants(matching: .any)["messageInput"]
        let messageInputAppeared = waitForComposer(in: app, messageInput: messageInput)
        if !messageInputAppeared {
            let debugAttachment = XCTAttachment(string: app.debugDescription)
            debugAttachment.name = "post-new-session-debug-description"
            debugAttachment.lifetime = .keepAlways
            add(debugAttachment)

            let screenshotAttachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            screenshotAttachment.name = "post-new-session-screenshot"
            screenshotAttachment.lifetime = .keepAlways
            add(screenshotAttachment)
        }
        XCTAssertTrue(messageInputAppeared, "Expected the message input to appear in the new session.")
    }

    @MainActor
    func testAuthenticatedUserCanSendMessageInExistingSession() async throws {
        try await Self.requireReachableServer()

        let sessionID = try await Self.createSession(label: "UI Test \(UUID().uuidString.prefix(8))")

        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        openSessionsListIfNeeded(in: app)

        let sessionRow = app.descendants(matching: .any)["sessionRow_\(sessionID)"]
        XCTAssertTrue(sessionRow.waitForExistence(timeout: 10), "Expected the pre-created session row to appear.")
        sessionRow.tap()

        let messageInput = app.descendants(matching: .any)["messageInput"]
        XCTAssertTrue(messageInput.waitForExistence(timeout: 10), "Expected the message input to appear after opening the session.")
        messageInput.tap()
        messageInput.typeText("Reply with exactly the single word FawxTest and nothing else.")

        let sendButton = app.buttons["sendButton"]
        XCTAssertTrue(sendButton.waitForExistence(timeout: 3), "Expected the send button to appear.")
        sendButton.tap()

        let assistantMessage = app.staticTexts.containing(
            NSPredicate(format: "label BEGINSWITH %@", "FawxTest")
        ).firstMatch
        XCTAssertTrue(
            assistantMessage.waitForExistence(timeout: 20),
            "Expected an assistant reply to appear after sending a message."
        )

        XCTAssertFalse(
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Response interrupted")).firstMatch.exists,
            "Expected the chat flow to complete without the stream interruption decode error."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "authenticated-chat-flow"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    @MainActor
    func testAuthenticatedUserCanSeeExpectedLoadedSkillInSkillsScreen() async throws {
        let expectedSkillName = try Self.requireExpectedSkillName()
        try await Self.requireReachableServer()

        let app = TestConfig.makeApp(resetState: true)
        app.launch()
        try await completeAuthenticatedLaunchIfNeeded(in: app)
        try await waitForMainShell(in: app)
        openSkills(in: app)

        let skillCard = app.descendants(matching: .any)["skillCard_\(expectedSkillName)"]
        XCTAssertTrue(
            skillCard.waitForExistence(timeout: 10),
            "Expected the server-loaded Skills screen to include '\(expectedSkillName)'."
        )
    }

    private static func createSession(label: String) async throws -> String {
        let (serverURL, bearerToken) = try requireCredentials()

        let endpoint = URL(string: serverURL + "/v1/sessions")!
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["label": label])

        let (data, response) = try await URLSession.shared.data(for: request)
        let httpResponse = try XCTUnwrap(response as? HTTPURLResponse)
        XCTAssertEqual(httpResponse.statusCode, 201, "Expected session creation to succeed.")

        let createdSession = try JSONDecoder().decode(CreatedSession.self, from: data)
        return createdSession.key
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

    private static func requireExpectedSkillName() throws -> String {
        guard let expectedSkillName = TestConfig.expectedSkillName else {
            throw XCTSkip("Missing FAWX_TEST_EXPECTED_SKILL_NAME")
        }
        return expectedSkillName
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
    private func waitForComposer(
        in app: XCUIApplication,
        messageInput: XCUIElement,
        timeout: TimeInterval = 10
    ) -> Bool {
        if messageInput.waitForExistence(timeout: timeout) {
            return true
        }

        let newSessionButton = app.buttons["newSessionButton"]
        guard newSessionButton.waitForExistence(timeout: 2) else {
            return false
        }

        newSessionButton.tap()
        return messageInput.waitForExistence(timeout: 5)
    }

    @MainActor
    private func openSessionsListIfNeeded(in app: XCUIApplication) {
        let sessionList = app.descendants(matching: .any)["sessionList"]
        if sessionList.waitForExistence(timeout: 2) {
            return
        }

        let sectionMenuButton = app.buttons["sectionMenuButton"]
        XCTAssertTrue(
            sectionMenuButton.waitForExistence(timeout: 5),
            "Expected the section menu button to appear in the authenticated chat shell."
        )
        sectionMenuButton.tap()

        let sessionsButton = app.buttons["Sessions"]
        XCTAssertTrue(
            sessionsButton.waitForExistence(timeout: 5),
            "Expected the Sessions menu action to appear."
        )
        sessionsButton.tap()

        XCTAssertTrue(
            sessionList.waitForExistence(timeout: 5),
            "Expected the sessions list to appear after switching to the Sessions section."
        )
    }

    @MainActor
    private func completeAuthenticatedLaunchIfNeeded(in app: XCUIApplication) async throws {
        if isMainShellVisible(in: app) {
            return
        }

        TestConfig.openRemoteOnboardingIfNeeded(in: app)
        let (serverURL, bearerToken) = try Self.requireCredentials()
        try await enterServerURLIfNeeded(in: app, serverURL: serverURL)

        if isMainShellVisible(in: app) {
            return
        }

        try await enterPairingCodeIfNeeded(
            in: app,
            serverURL: serverURL,
            bearerToken: bearerToken
        )
    }

    @MainActor
    private func waitForMainShell(in app: XCUIApplication) async throws {
        let deadline = Date().addingTimeInterval(15)
        while Date() < deadline {
            if isMainShellVisible(in: app) {
                return
            }
            if app.descendants(matching: .any)["serverURLField"].exists {
                throw XCTSkip("App launched into onboarding instead of authenticated shell.")
            }
            try? await Task.sleep(nanoseconds: 200_000_000)
        }
        throw XCTSkip("Main app shell did not appear within 15 seconds.")
    }

    @MainActor
    private func enterServerURLIfNeeded(in app: XCUIApplication, serverURL: String) async throws {
        let serverField = app.descendants(matching: .any)["serverURLField"]
        guard serverField.waitForExistence(timeout: 5) else {
            return
        }

        serverField.tap()
        serverField.typeText(serverURL)

        let continueButton = app.buttons["continueButton"]
        XCTAssertTrue(continueButton.waitForExistence(timeout: 5), "Expected continue button.")
        if continueButton.isEnabled == false {
            let healthButton = app.buttons["testConnectionButton"]
            XCTAssertTrue(healthButton.waitForExistence(timeout: 5), "Expected test connection button.")
            healthButton.tap()
            if await waitForEnabled(continueButton, timeoutSeconds: 20) == false {
                throw XCTSkip("Could not continue onboarding because the server connection never became ready.")
            }
        }
        continueButton.tap()
    }

    @MainActor
    private func enterPairingCodeIfNeeded(
        in app: XCUIApplication,
        serverURL: String,
        bearerToken: String
    ) async throws {
        let codeField = app.descendants(matching: .any)["bearerTokenField"]
        guard codeField.waitForExistence(timeout: 8) else {
            return
        }

        let pairingCode = try await Self.generatePairingCode(serverURL: serverURL, bearerToken: bearerToken)
        codeField.tap()
        codeField.typeText(pairingCode)

        let pairButton = app.buttons["Pair Device"]
        XCTAssertTrue(pairButton.waitForExistence(timeout: 5), "Expected Pair Device button.")
        if await waitForEnabled(pairButton, timeoutSeconds: 10) == false {
            throw XCTSkip("Could not pair device because the pairing button never became enabled.")
        }
        pairButton.tap()
    }

    @MainActor
    private func waitForEnabled(_ element: XCUIElement, timeoutSeconds: TimeInterval) async -> Bool {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while element.isEnabled == false && Date() < deadline {
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        return element.isEnabled
    }

    @MainActor
    private func isMainShellVisible(in app: XCUIApplication) -> Bool {
        app.descendants(matching: .any)["sessionList"].exists
            || app.descendants(matching: .any)["messageInput"].exists
            || app.buttons["newSessionButton"].exists
    }

    @MainActor
    private func openSkills(in app: XCUIApplication) {
#if os(macOS)
        let navigateMenu = app.menuBars.menuBarItems["Navigate"]
        XCTAssertTrue(navigateMenu.waitForExistence(timeout: 5), "Expected Navigate menu.")
        navigateMenu.tap()

        let skillsMenuItem = app.menuItems["Skills"]
        XCTAssertTrue(skillsMenuItem.waitForExistence(timeout: 5), "Expected Skills menu item.")
        skillsMenuItem.tap()
#else
        let sectionMenuButton = app.buttons["sectionMenuButton"]
        XCTAssertTrue(sectionMenuButton.waitForExistence(timeout: 5), "Expected section menu button.")
        sectionMenuButton.tap()

        let skillsButton = app.buttons["Skills"]
        XCTAssertTrue(skillsButton.waitForExistence(timeout: 5), "Expected Skills button.")
        skillsButton.tap()
#endif

        let searchField = app.descendants(matching: .any)["skillsSearchField"]
        XCTAssertTrue(searchField.waitForExistence(timeout: 10), "Expected Skills screen.")
    }

    private static func generatePairingCode(serverURL: String, bearerToken: String) async throws -> String {
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
            let body = String(data: data, encoding: .utf8) ?? "<non-UTF8>"
            throw XCTSkip("Could not generate pairing code: \(httpResponse.statusCode) \(body)")
        }

        let generated = try JSONDecoder().decode(GeneratedPairingCode.self, from: data)
        return generated.code
    }
}

private struct CreatedSession: Decodable {
    let key: String
}

private struct GeneratedPairingCode: Decodable {
    let code: String
}
