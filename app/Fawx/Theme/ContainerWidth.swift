import SwiftUI

private struct ContainerWidthKey: EnvironmentKey {
    static let defaultValue: CGFloat = 720
}

extension EnvironmentValues {
    var containerWidth: CGFloat {
        get { self[ContainerWidthKey.self] }
        set { self[ContainerWidthKey.self] = newValue }
    }
}
