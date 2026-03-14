import SwiftUI

struct ToastView: View {
    let toast: AppToast

    var body: some View {
        Text(toast.message)
            .font(FawxTypography.chatBody)
            .foregroundStyle(Color.fawxText)
            .padding(.horizontal, FawxSpacing.paddingLG)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(backgroundColor)
            .clipShape(Capsule())
            .overlay(
                Capsule()
                    .stroke(borderColor, lineWidth: 1)
            )
            .shadow(color: Color.black.opacity(0.12), radius: 12, x: 0, y: 8)
    }

    private var backgroundColor: Color {
        switch toast.style {
        case .info:
            return Color.fawxSurface
        case .success:
            return Color.fawxSuccess.opacity(0.14)
        case .error:
            return Color.fawxError.opacity(0.14)
        }
    }

    private var borderColor: Color {
        switch toast.style {
        case .info:
            return Color.fawxBorder
        case .success:
            return Color.fawxSuccess.opacity(0.35)
        case .error:
            return Color.fawxError.opacity(0.35)
        }
    }
}
