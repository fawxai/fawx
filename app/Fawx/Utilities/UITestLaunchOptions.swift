import Foundation

enum UITestLaunchOptions {
    static let resetStateArgument = "--uitesting-reset-state"
    static let uiTestingArgument = "--uitesting"
    static let serverURLEnvironmentKey = "FAWX_TEST_SERVER_URL"
    static let bearerTokenEnvironmentKey = "FAWX_TEST_BEARER_TOKEN"
    static let pairedDeviceNameEnvironmentKey = "FAWX_TEST_PAIRED_DEVICE_NAME"

    static var shouldResetState: Bool {
        ProcessInfo.processInfo.arguments.contains(resetStateArgument)
    }

    static var isUITesting: Bool {
        ProcessInfo.processInfo.arguments.contains(uiTestingArgument)
    }

    static var serverURLOverride: String? {
        overrideValue(for: serverURLEnvironmentKey)
    }

    static var bearerTokenOverride: String? {
        overrideValue(for: bearerTokenEnvironmentKey)
    }

    static var pairedDeviceNameOverride: String? {
        overrideValue(for: pairedDeviceNameEnvironmentKey)
    }

    private static func overrideValue(for key: String) -> String? {
        let value = ProcessInfo.processInfo.environment[key]?
            .trimmingCharacters(in: .whitespacesAndNewlines)

        guard let value, !value.isEmpty else {
            return nil
        }

        return value
    }
}
