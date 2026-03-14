import SwiftUI

struct SessionRowView: View {
    let session: Session
    let isSelected: Bool
    let isStreaming: Bool

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
            if isStreaming {
                Circle()
                    .fill(Color.fawxAccent)
                    .frame(width: 8, height: 8)
                    .padding(.top, 5)
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
                    Text(session.displayTitle)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(isSelected ? Color.fawxText : Color.fawxText)
                        .lineLimit(1)

                    Spacer(minLength: FawxSpacing.paddingSM)

                    Text(relativeTimestampString(session.updatedAt))
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .lineLimit(1)
                }

                if let preview = session.subtitlePreview, !preview.isEmpty {
                    Text(preview)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, FawxSpacing.paddingXS)
        .padding(.horizontal, FawxSpacing.paddingXS)
    }
}
