import XCTest

enum TestConfig {
    static let uiTestingArgument = "--uitesting"
    static let resetStateArgument = "--uitesting-reset-state"
    static let generatedDefaultsSuite = "ai.fawx.app.uitests.defaults.\(UUID().uuidString)"
    static let generatedKeychainService = "ai.fawx.app.uitests.keychain.\(UUID().uuidString)"

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
    static let localConfigPath = value(environmentKey: "FAWX_TEST_LOCAL_CONFIG_PATH")

    static var defaultsSuite: String {
        value(environmentKey: "FAWX_TEST_DEFAULTS_SUITE") ?? generatedDefaultsSuite
    }

    static var keychainService: String {
        value(environmentKey: "FAWX_TEST_KEYCHAIN_SERVICE") ?? generatedKeychainService
    }

    static var disableLocalInstall: Bool {
#if os(macOS)
        flag(environmentKey: "FAWX_TEST_DISABLE_LOCAL_INSTALL") ?? true
#else
        flag(environmentKey: "FAWX_TEST_DISABLE_LOCAL_INSTALL") ?? false
#endif
    }

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

        app.launchEnvironment["FAWX_TEST_DEFAULTS_SUITE"] = defaultsSuite
        app.launchEnvironment["FAWX_TEST_KEYCHAIN_SERVICE"] = keychainService

        if let localConfigPath {
            app.launchEnvironment["FAWX_TEST_LOCAL_CONFIG_PATH"] = localConfigPath
        }

        if disableLocalInstall {
            app.launchEnvironment["FAWX_TEST_DISABLE_LOCAL_INSTALL"] = "1"
        }

        return app
    }

    @MainActor
    static func openRemoteOnboardingIfNeeded(in app: XCUIApplication) {
        let serverField = app.descendants(matching: .any)["serverURLField"]
        if serverField.waitForExistence(timeout: 3) {
            return
        }

#if os(macOS)
        let remoteOnboardingButton = app.descendants(matching: .any)["connectToRemoteOnboardingButton"]
        XCTAssertTrue(
            remoteOnboardingButton.waitForExistence(timeout: 15),
            "Expected the macOS welcome step to offer remote onboarding."
        )
        remoteOnboardingButton.tap()
        XCTAssertTrue(
            serverField.waitForExistence(timeout: 10),
            "Expected the remote onboarding server URL field to appear after selecting remote onboarding."
        )
#endif
    }

    private static func value(environmentKey: String, fallbackFile: String? = nil) -> String? {
        if let environmentValue = ProcessInfo.processInfo.environment[environmentKey]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           environmentValue.isEmpty == false
        {
            return environmentValue
        }

        if let fallbackFile,
           let rawFileValue = try? String(contentsOfFile: fallbackFile, encoding: .utf8)
        {
            let fileValue = rawFileValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard fileValue.isEmpty == false else {
                return nil
            }
            return fileValue
        }

        return nil
    }

    private static func flag(environmentKey: String) -> Bool? {
        guard let value = ProcessInfo.processInfo.environment[environmentKey]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased(),
            !value.isEmpty
        else {
            return nil
        }

        switch value {
        case "1", "true", "yes", "on":
            return true
        case "0", "false", "no", "off":
            return false
        default:
            return nil
        }
    }
}
