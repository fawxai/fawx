import XCTest

enum TestConfig {
    static let uiTestingArgument = "--uitesting"
    static let resetStateArgument = "--uitesting-reset-state"

    static let serverURL = ProcessInfo.processInfo.environment["FAWX_TEST_SERVER_URL"]
    static let bearerToken = ProcessInfo.processInfo.environment["FAWX_TEST_BEARER_TOKEN"]

    static func makeApp(resetState: Bool = true) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments.append(uiTestingArgument)

        if resetState {
            app.launchArguments.append(resetStateArgument)
        }

        if let serverURL {
            app.launchEnvironment["FAWX_TEST_SERVER_URL"] = serverURL
        }

        if let bearerToken {
            app.launchEnvironment["FAWX_TEST_BEARER_TOKEN"] = bearerToken
        }

        return app
    }
}
