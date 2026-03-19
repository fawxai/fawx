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
}

private struct CreatedSession: Decodable {
    let key: String
}
