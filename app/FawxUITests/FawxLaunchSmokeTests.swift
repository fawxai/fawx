import XCTest

final class FawxLaunchSmokeTests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testLaunchShowsOnboardingWhenStateIsReset() throws {
        let app = TestConfig.makeApp(resetState: true)
        app.launch()

        XCTAssertTrue(
            app.textFields["serverURLField"].waitForExistence(timeout: 5),
            "Expected the onboarding server URL field to appear on launch."
        )

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = "launch-smoke-onboarding"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
