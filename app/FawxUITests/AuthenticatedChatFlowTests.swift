import XCTest

final class AuthenticatedChatFlowTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testAuthenticatedUserCanCreateSessionAndReceiveReply() throws {
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
        messageInput.tap()
        messageInput.typeText("Reply with exactly the single word FawxTest and nothing else.")

        let sendButton = app.buttons["sendButton"]
        XCTAssertTrue(sendButton.waitForExistence(timeout: 3), "Expected the send button to appear.")
        sendButton.tap()

        let assistantMessage = app.descendants(matching: .any)["assistantMessage"]
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
}
