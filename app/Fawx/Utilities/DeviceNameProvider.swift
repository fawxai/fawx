import Foundation

#if os(iOS)
import UIKit
#endif

enum DeviceNameProvider {
    @MainActor
    static func current() -> String {
#if os(iOS)
        let candidate = UIDevice.current.name.trimmingCharacters(in: .whitespacesAndNewlines)
        return candidate.isEmpty ? "iPhone" : candidate
#elseif os(macOS)
        let candidate = Host.current().localizedName?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return candidate.isEmpty ? "This Mac" : candidate
#else
        let candidate = ProcessInfo.processInfo.hostName.trimmingCharacters(in: .whitespacesAndNewlines)
        return candidate.isEmpty ? "Fawx Device" : candidate
#endif
    }
}
