import XCTest
import Foundation

final class AuthenticatedChatFlowTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testAuthenticatedUserCanOpenNewConversationComposer() throws {
        XCTAssertNotNil(TestConfig.serverURL, "FAWX_TEST_SERVER_URL must be set for authenticated chat tests.")
        XCTAssertNotNil(TestConfig.bearerToken, "FAWX_TEST_BEARER_TOKEN must be set for authenticated chat tests.")

        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        let sessionList = app.descendants(matching: .any)["sessionList"]
        XCTAssertTrue(sessionList.waitForExistence(timeout: 10), "Expected the session list to appear for an authenticated launch.")

        let newSessionButton = app.buttons["newSessionButton"]
        XCTAssertTrue(newSessionButton.waitForExistence(timeout: 5), "Expected the new session button to appear.")
        newSessionButton.tap()

        let messageInput = app.descendants(matching: .any)["messageInput"]
        let messageInputAppeared = messageInput.waitForExistence(timeout: 10)
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
        XCTAssertNotNil(TestConfig.serverURL, "FAWX_TEST_SERVER_URL must be set for authenticated chat tests.")
        XCTAssertNotNil(TestConfig.bearerToken, "FAWX_TEST_BEARER_TOKEN must be set for authenticated chat tests.")

        let sessionID = try await Self.createSession(label: "UI Test \(UUID().uuidString.prefix(8))")

        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        let sessionList = app.descendants(matching: .any)["sessionList"]
        XCTAssertTrue(sessionList.waitForExistence(timeout: 10), "Expected the session list to appear for an authenticated launch.")

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
        guard let serverURL = TestConfig.serverURL else {
            throw XCTSkip("Missing FAWX_TEST_SERVER_URL")
        }
        guard let bearerToken = TestConfig.bearerToken else {
            throw XCTSkip("Missing FAWX_TEST_BEARER_TOKEN")
        }

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
}

private struct CreatedSession: Decodable {
    let key: String
}
