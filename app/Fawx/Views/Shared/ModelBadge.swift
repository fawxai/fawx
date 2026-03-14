import SwiftUI

struct ModelBadge: View {
    let title: String

    var body: some View {
        Text(title)
            .font(FawxTypography.status)
            .foregroundStyle(Color.fawxText)
            .lineLimit(1)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, FawxSpacing.paddingXS)
            .background(Color.fawxSurfaceHover)
            .overlay(
                Capsule()
                    .stroke(Color.fawxBorder, lineWidth: 1)
            )
            .clipShape(Capsule())
    }
}
