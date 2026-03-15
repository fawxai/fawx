import Foundation

#if os(iOS)
import UIKit
#endif

enum FawxHaptics {
    @MainActor
    static func lightImpact() {
#if os(iOS)
        lightGenerator.impactOccurred()
        lightGenerator.prepare()
#endif
    }

#if os(iOS)
    @MainActor private static let lightGenerator: UIImpactFeedbackGenerator = {
        let generator = UIImpactFeedbackGenerator(style: .light)
        generator.prepare()
        return generator
    }()
#endif
}
