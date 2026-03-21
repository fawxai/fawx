import MarkdownUI
import SwiftUI

struct MessageBubble: View {
    @Environment(\.containerWidth) private var containerWidth

    let role: MessageRole
    let content: String
    let timestamp: Int?
    let isStreaming: Bool

    init(message: SessionMessage) {
        self.role = message.role
        self.content = message.content
        self.timestamp = message.timestamp
        self.isStreaming = false
    }

    init(role: MessageRole, content: String, timestamp: Int? = nil, isStreaming: Bool = false) {
        self.role = role
        self.content = content
        self.timestamp = timestamp
        self.isStreaming = isStreaming
    }

    var body: some View {
        if role == .system {
            Text(content)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.vertical, FawxSpacing.paddingSM)
        } else {
            bubbleContent
        }
    }

    private var bubbleContent: some View {
        HStack {
            if role == .user {
                Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
            }

            VStack(alignment: role == .user ? .trailing : .leading, spacing: FawxSpacing.paddingSM) {
                bubbleLabel
                    .padding(.horizontal, bubbleHorizontalPadding)
                    .padding(.vertical, FawxSpacing.paddingMD)
                    .background(bubbleBackground)
                    .overlay(bubbleBorder)
                    .clipShape(RoundedRectangle(cornerRadius: bubbleCornerRadius))

                if let timestamp {
                    Text(timeString(timestamp))
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .monospacedDigit()
                }
            }
            .frame(
                maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth),
                alignment: role == .user ? .trailing : .leading
            )

            if role != .user {
                Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
            }
        }
        .frame(maxWidth: .infinity)
        .accessibilityIdentifier(accessibilityIdentifier)
    }

    @ViewBuilder
    private var bubbleLabel: some View {
        if role == .user {
            messageContent
                .fixedSize(horizontal: false, vertical: true)
        } else {
            messageContent
                .frame(maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth), alignment: .leading)
        }
    }

    private var bubbleHorizontalPadding: CGFloat {
        switch role {
        case .assistant, .tool:
            FawxSpacing.paddingXL
        case .user, .system:
            FawxSpacing.paddingLG
        }
    }

    private var bubbleCornerRadius: CGFloat {
        FawxSpacing.cornerRadius + 4
    }

    @ViewBuilder
    private var messageContent: some View {
        switch role {
        case .user:
            Text(content)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxUserBubbleText)
                .textSelection(.enabled)
        case .assistant:
            Markdown(content + (isStreaming ? "▍" : ""))
                .markdownTextStyle {
                    FontSize(FawxTypography.chatBodyPointSize)
                    ForegroundColor(Color.fawxText)
                }
                .markdownTextStyle(\.strong) {
                    FontWeight(.semibold)
                }
                .markdownTextStyle(\.code) {
                    FontFamilyVariant(.monospaced)
                    FontSize(.em(0.92))
                    ForegroundColor(Color.fawxText)
                    BackgroundColor(Color.fawxCode.opacity(FawxOpacity.codeBackground))
                }
                .markdownTextStyle(\.link) {
                    ForegroundColor(Color.fawxAccent)
                }
                .markdownBlockStyle(\.codeBlock) { configuration in
                    CodeBlock(language: configuration.language, content: configuration.content)
                }
                .textSelection(.enabled)
        case .tool:
            Text(verbatim: toolDisplayContent)
                .font(FawxTypography.code)
                .foregroundStyle(Color.fawxText)
                .textSelection(.enabled)
        case .system:
            EmptyView()
        }
    }

    private var backgroundColor: Color {
        switch role {
        case .user:
            return Color.fawxUserBubble
        case .assistant:
            return isStreaming ? Color.fawxSurfaceHover : Color.fawxSurface
        case .tool:
            return Color.fawxCode
        case .system:
            return Color.fawxAccentSubtle
        }
    }

    private var bubbleBackground: some View {
        RoundedRectangle(cornerRadius: bubbleCornerRadius)
            .fill(backgroundColor)
    }

    @ViewBuilder
    private var bubbleBorder: some View {
        if let borderColor {
            RoundedRectangle(cornerRadius: bubbleCornerRadius)
                .stroke(borderColor, lineWidth: 1)
        }
    }

    private var borderColor: Color? {
        switch role {
        case .user:
            return nil
        case .assistant:
            if isStreaming {
                return Color.fawxBorder
            }
            return Color.fawxBorder.opacity(FawxOpacity.borderMedium)
        case .tool:
            return Color.fawxBorder.opacity(FawxOpacity.borderStrong)
        case .system:
            return Color.fawxAccent.opacity(FawxOpacity.accentBorder)
        }
    }

    private var accessibilityIdentifier: String {
        switch role {
        case .user:
            return "userMessage"
        case .assistant:
            return isStreaming ? "streamingAssistantMessage" : "assistantMessage"
        case .tool:
            return "toolMessage"
        case .system:
            return "systemMessage"
        }
    }

    private var toolDisplayContent: String {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Tool completed without text output." : content
    }
}
