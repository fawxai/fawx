import Foundation

enum UITestLaunchOptions {
    static let resetStateArgument = "--uitesting-reset-state"
    static let uiTestingArgument = "--uitesting"

    static var shouldResetState: Bool {
        ProcessInfo.processInfo.arguments.contains(resetStateArgument)
    }
}
