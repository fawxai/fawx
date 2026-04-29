import CoreGraphics
import SwiftUI

enum FawxAnimation {
    static let expand = Animation.easeInOut(duration: 0.16)
}

enum FawxSpacing {
    static let paddingXS: CGFloat = 4
    static let paddingSM: CGFloat = 8
    static let paddingMD: CGFloat = 12
    static let paddingLG: CGFloat = 16
    static let paddingXL: CGFloat = 24
    static let cornerRadiusSM: CGFloat = 4
    static let cornerRadius: CGFloat = 8
    static let sidebarWidth: CGFloat = 260
    static let messageWidthRatio: CGFloat = 0.85
    static let ripcordNotificationMaxWidth: CGFloat = 320
    static let ripcordReviewTrayMaxWidth: CGFloat = 520
    static let placeholderCopyMaxWidth: CGFloat = 320
    static let inputBarMinHeight: CGFloat = 48
    static let inputBarMaxHeight: CGFloat = 200
    static let transcriptEdgeClamp: CGFloat = 48

    static func maxMessageWidth(for containerWidth: CGFloat) -> CGFloat {
        let proportional = containerWidth * messageWidthRatio
        return min(max(proportional, 400), 1200)
    }

    static func resolvedChatContainerWidth(for viewportWidth: CGFloat) -> CGFloat {
        max(viewportWidth - (paddingXL * 2), 1)
    }
}
