import XCTest

enum TestConfig {
    static let uiTestingArgument = "--uitesting"
    static let resetStateArgument = "--uitesting-reset-state"

    static let serverURL = value(
        environmentKey: "FAWX_TEST_SERVER_URL",
        fallbackFile: "/tmp/fawx_test_server_url.txt"
    )
    static let bearerToken = value(
        environmentKey: "FAWX_TEST_BEARER_TOKEN",
        fallbackFile: "/tmp/fawx_test_bearer_token.txt"
    )
    static let pairedDeviceName = value(
        environmentKey: "FAWX_TEST_PAIRED_DEVICE_NAME",
        fallbackFile: "/tmp/fawx_test_paired_device_name.txt"
    )

    static func makeApp(resetState: Bool = true, includeCredentials: Bool = true) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments.append(uiTestingArgument)

        if resetState {
            app.launchArguments.append(resetStateArgument)
        }

        if includeCredentials {
            if let serverURL {
                app.launchEnvironment["FAWX_TEST_SERVER_URL"] = serverURL
            }

            if let bearerToken {
                app.launchEnvironment["FAWX_TEST_BEARER_TOKEN"] = bearerToken
            }

            if let pairedDeviceName {
                app.launchEnvironment["FAWX_TEST_PAIRED_DEVICE_NAME"] = pairedDeviceName
            }
        }

        return app
    }

    private static func value(environmentKey: String, fallbackFile: String) -> String? {
        if let environmentValue = ProcessInfo.processInfo.environment[environmentKey]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           environmentValue.isEmpty == false
        {
            return environmentValue
        }

        if let rawFileValue = try? String(contentsOfFile: fallbackFile, encoding: .utf8) {
            let fileValue = rawFileValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard fileValue.isEmpty == false else {
                return nil
            }
            return fileValue
        }

        return nil
    }
}
