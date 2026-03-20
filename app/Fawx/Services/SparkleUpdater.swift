#if os(macOS)
import Foundation
import Observation
import Sparkle

enum SparkleConfiguration {
    static let feedURLKey = "SUFeedURL"
    static let publicEDKeyKey = "SUPublicEDKey"
    static let publicKeyPlaceholder = "PASTE_PUBLIC_KEY_HERE"

    static func isReady(infoDictionary: [String: Any]) -> Bool {
        guard let feedURL = trimmedString(for: feedURLKey, in: infoDictionary),
              !feedURL.isEmpty
        else {
            return false
        }

        guard let publicKey = trimmedString(for: publicEDKeyKey, in: infoDictionary),
              !publicKey.isEmpty
        else {
            return false
        }

        return publicKey != publicKeyPlaceholder
    }

    private static func trimmedString(for key: String, in infoDictionary: [String: Any]) -> String? {
        guard let value = infoDictionary[key] as? String else {
            return nil
        }

        return value.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// Manages Sparkle auto-update lifecycle for macOS.
/// The updater stays dormant until the real public EdDSA key is configured.
@MainActor
@Observable
final class SparkleUpdater {
    var canCheckForUpdates = false

    @ObservationIgnored private let updaterController: SPUStandardUpdaterController?
    @ObservationIgnored private var canCheckObservation: NSKeyValueObservation?

    init(infoDictionary: [String: Any] = Bundle.main.infoDictionary ?? [:]) {
        guard SparkleConfiguration.isReady(infoDictionary: infoDictionary) else {
            updaterController = nil
            return
        }

        let controller = SPUStandardUpdaterController(
            startingUpdater: true,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
        updaterController = controller
        canCheckForUpdates = controller.updater.canCheckForUpdates
        canCheckObservation = controller.updater.observe(
            \.canCheckForUpdates,
            options: [.initial, .new]
        ) { [weak self] updater, _ in
            Task { @MainActor [weak self] in
                self?.canCheckForUpdates = updater.canCheckForUpdates
            }
        }
    }

    func checkForUpdates() {
        updaterController?.checkForUpdates(nil)
    }
}
#endif
