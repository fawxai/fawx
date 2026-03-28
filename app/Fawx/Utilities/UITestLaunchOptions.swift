import Foundation

enum UITestLaunchOptions {
    static let resetStateArgument = "--uitesting-reset-state"
    static let uiTestingArgument = "--uitesting"
    static let serverURLEnvironmentKey = "FAWX_TEST_SERVER_URL"
    static let bearerTokenEnvironmentKey = "FAWX_TEST_BEARER_TOKEN"
    static let pairedDeviceNameEnvironmentKey = "FAWX_TEST_PAIRED_DEVICE_NAME"
    static let defaultsSuiteEnvironmentKey = "FAWX_TEST_DEFAULTS_SUITE"
    static let keychainServiceEnvironmentKey = "FAWX_TEST_KEYCHAIN_SERVICE"
    static let localConfigPathEnvironmentKey = "FAWX_TEST_LOCAL_CONFIG_PATH"
    static let disableLocalInstallEnvironmentKey = "FAWX_TEST_DISABLE_LOCAL_INSTALL"

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

    static var defaultsSuiteOverride: String? {
        overrideValue(for: defaultsSuiteEnvironmentKey)
    }

    static var keychainServiceOverride: String? {
        overrideValue(for: keychainServiceEnvironmentKey)
    }

    static var localConfigPathOverride: String? {
        overrideValue(for: localConfigPathEnvironmentKey)
    }

    static var shouldDisableLocalInstall: Bool {
        flagValue(for: disableLocalInstallEnvironmentKey)
    }

    private static func overrideValue(for key: String) -> String? {
        let value = ProcessInfo.processInfo.environment[key]?
            .trimmingCharacters(in: .whitespacesAndNewlines)

        guard let value, !value.isEmpty else {
            return nil
        }

        return value
    }

    private static func flagValue(for key: String) -> Bool {
        guard let value = overrideValue(for: key)?.lowercased() else {
            return false
        }

        switch value {
        case "1", "true", "yes", "on":
            return true
        default:
            return false
        }
    }
}
