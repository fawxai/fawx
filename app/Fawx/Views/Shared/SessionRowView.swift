import SwiftUI

struct SessionRowView: View {
  let session: Session
  let isSelected: Bool
  let isStreaming: Bool
  let activityLabel: String?
  let showsUnreadDot: Bool
  let showsSelectionControl: Bool
  let isMarkedForBulkAction: Bool
  @State private var isHovering = false

  init(
    session: Session,
    isSelected: Bool,
    isStreaming: Bool,
    activityLabel: String? = nil,
    showsUnreadDot: Bool = false,
    showsSelectionControl: Bool = false,
    isMarkedForBulkAction: Bool = false
  ) {
    self.session = session
    self.isSelected = isSelected
    self.isStreaming = isStreaming
    self.activityLabel = activityLabel
    self.showsUnreadDot = showsUnreadDot
    self.showsSelectionControl = showsSelectionControl
    self.isMarkedForBulkAction = isMarkedForBulkAction
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingSM) {
        HStack(alignment: .center, spacing: 6) {
          if showsSelectionControl {
            Image(systemName: isMarkedForBulkAction ? "checkmark.circle.fill" : "circle")
              .font(.system(size: 14, weight: .semibold))
              .foregroundStyle(
                isMarkedForBulkAction
                  ? Color.fawxAccent
                  : Color.fawxTextSecondary.opacity(0.7)
              )
          }

          if isStreaming {
            Circle()
              .fill(Color.fawxAccent)
              .frame(width: 6, height: 6)
              .scaleEffect(isSelected ? 1 : 0.92)
          } else if showsUnreadDot {
            Circle()
              .fill(Color.fawxAccent)
              .frame(width: 6, height: 6)
          }

          Text(session.displayTitle)
            .font(FawxTypography.sidebarTitle)
            .foregroundStyle(Color.fawxText)
            .lineLimit(1)
        }

        Spacer(minLength: FawxSpacing.paddingSM)

        HStack(spacing: FawxSpacing.paddingXS) {
          if let activityLabel {
            Text(activityLabel)
              .font(FawxTypography.status)
              .foregroundStyle(Color.fawxAccent)
              .padding(.horizontal, FawxSpacing.paddingSM)
              .padding(.vertical, FawxSpacing.paddingXS / 2)
              .background(
                Capsule()
                  .fill(Color.fawxAccentSubtle.opacity(0.55))
              )
              .lineLimit(1)
          }

          Text(relativeTimestampString(session.updatedAt))
            .font(FawxTypography.status)
            .foregroundStyle(Color.fawxTextSecondary)
            .monospacedDigit()
            .lineLimit(1)
        }
      }

      Text(Self.subtitleText(for: session))
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .lineLimit(1)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.vertical, FawxSpacing.paddingSM)
    .padding(.horizontal, FawxSpacing.paddingMD)
    .background(rowBackgroundColor)
    .overlay(alignment: .leading) {
      RoundedRectangle(cornerRadius: 1.5)
        .fill(isSelected ? Color.fawxAccent : .clear)
        .frame(width: 3)
        .padding(.vertical, FawxSpacing.paddingXS)
    }
    .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    .animation(.easeInOut(duration: 0.14), value: isHovering)
    .animation(.easeInOut(duration: 0.14), value: isSelected)
    .accessibilityElement(children: .combine)
    .accessibilityIdentifier("sessionRow_\(session.id)")
    #if os(macOS)
      .onHover { hovering in
        isHovering = hovering
      }
    #endif
  }

  private var rowBackgroundColor: Color {
    if isSelected {
      return .fawxSurfaceActive
    }
    if isHovering {
      return .fawxSurfaceHover
    }
    return .clear
  }

  nonisolated static func subtitleText(for session: Session) -> String {
    if let preview = session.subtitlePreview, preview.isEmpty == false {
      return preview
    }

    if session.messageCount == 0 {
      return "No messages yet"
    }

    if session.messageCount == 1 {
      return "1 message"
    }

    return "\(session.messageCount) messages"
  }
}
