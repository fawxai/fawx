import SwiftUI

extension View {
    @ViewBuilder
    func iOSInlineNavigationTitle() -> some View {
#if os(iOS)
        self.navigationBarTitleDisplayMode(.inline)
#else
        self
#endif
    }
}

extension ToolbarItemPlacement {
    static var fawxTopLeading: ToolbarItemPlacement {
#if os(iOS)
        .topBarLeading
#else
        .navigation
#endif
    }
}
