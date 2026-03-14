import Foundation

#if os(iOS)
import UIKit
#endif

enum FawxHaptics {
    @MainActor
    static func lightImpact() {
#if os(iOS)
        let generator = UIImpactFeedbackGenerator(style: .light)
        generator.prepare()
        generator.impactOccurred()
#endif
    }
}
