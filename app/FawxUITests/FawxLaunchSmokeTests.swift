import XCTest

final class FawxLaunchSmokeTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testLaunchShowsOnboardingWhenStateIsReset() throws {
        let app = TestConfig.makeApp(resetState: true, includeCredentials: false)
        app.launch()
        TestConfig.openRemoteOnboardingIfNeeded(in: app)

        let serverField = app.descendants(matching: .any)["serverURLField"]
        XCTAssertTrue(
            serverField.waitForExistence(timeout: 5),
            "Expected the onboarding server URL field to appear on launch."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "launch-smoke-onboarding"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
