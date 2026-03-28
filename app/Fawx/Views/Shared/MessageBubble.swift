import MarkdownUI
import SwiftUI
#if os(macOS)
import AppKit
#endif
#if os(iOS)
import UIKit
#endif
import PDFKit

struct MessageBubble: View {
    @Environment(\.containerWidth) private var containerWidth

    let role: MessageRole
    let content: String
    let timestamp: Int?
    let isStreaming: Bool
    let footnoteText: String?
    private let contentBlocks: [SessionContentBlock]?

    init(message: SessionMessage, footnoteText: String? = nil) {
        self.role = message.role
        self.content = message.content
        self.timestamp = message.timestamp
        self.isStreaming = false
        self.footnoteText = footnoteText
        self.contentBlocks = message.contentBlocks
    }

    init(
        role: MessageRole,
        content: String,
        timestamp: Int? = nil,
        isStreaming: Bool = false,
        footnoteText: String? = nil
    ) {
        self.role = role
        self.content = content
        self.timestamp = timestamp
        self.isStreaming = isStreaming
        self.footnoteText = footnoteText
        self.contentBlocks = nil
    }

    var body: some View {
        Group {
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
        .textSelection(.enabled)
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

                if timestamp != nil || footnoteText != nil {
                    VStack(alignment: role == .user ? .trailing : .leading, spacing: 2) {
                        if let timestamp {
                            Text(timeString(timestamp))
                                .font(FawxTypography.status)
                                .foregroundStyle(Color.fawxTextSecondary)
                                .monospacedDigit()
                        }

                        if let footnoteText, !footnoteText.isEmpty {
                            Text(footnoteText)
                                .font(FawxTypography.status)
                                .foregroundStyle(Color.fawxTextSecondary)
                                .monospacedDigit()
                        }
                    }
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
        if role == .tool {
            Text(verbatim: toolDisplayContent)
                .font(FawxTypography.code)
                .foregroundStyle(Color.fawxText)
        } else {
            structuredMessageContent
        }
    }

    @ViewBuilder
    private var structuredMessageContent: some View {
        let attachmentBlocks = displayBlocks.filter(\.isRenderableAttachment)
        let renderedText = structuredTextContent

        VStack(alignment: role == .user ? .trailing : .leading, spacing: FawxSpacing.paddingSM) {
            if !attachmentBlocks.isEmpty {
                VStack(alignment: role == .user ? .trailing : .leading, spacing: FawxSpacing.paddingSM) {
                    ForEach(Array(attachmentBlocks.enumerated()), id: \.offset) { _, block in
                        MessageAttachmentBlockView(block: block)
                    }
                }
            }

            if !renderedText.isEmpty || isStreaming {
                switch role {
                case .assistant:
                    Markdown(renderedText + (isStreaming ? "▍" : ""))
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
                case .user:
                    Text(renderedText)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxUserBubbleText)
                case .system, .tool:
                    EmptyView()
                }
            }
        }
    }

    private var displayBlocks: [SessionContentBlock] {
        contentBlocks ?? [.text(content)]
    }

    private var structuredTextContent: String {
        displayBlocks.compactMap { block in
            switch block {
            case .text(let text):
                return text
            case .toolUse(_, let name, let input):
                let renderedInput = input.description.trimmingCharacters(in: .whitespacesAndNewlines)
                return renderedInput.isEmpty ? "[\(name)]" : "[\(name)] \(renderedInput)"
            case .toolResult:
                return nil
            case .image, .document:
                return nil
            }
        }
        .joined(separator: "\n\n")
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
        return trimmed.isEmpty ? "Tool output available." : content
    }
}

private struct MessageAttachmentBlockView: View {
    let block: SessionContentBlock

    var body: some View {
        switch block {
        case .image(let mediaType, let encodedData):
            Button(action: openAttachment) {
                imageAttachmentView(mediaType: mediaType, encodedData: encodedData)
            }
            .buttonStyle(.plain)
        case .document(_, let encodedData, let filename):
            Button(action: openAttachment) {
                documentAttachmentView(encodedData: encodedData, filename: filename)
            }
            .buttonStyle(.plain)
        case .text, .toolUse, .toolResult:
            EmptyView()
        }
    }

    @ViewBuilder
    private func imageAttachmentView(mediaType: String, encodedData: String?) -> some View {
        if
            let encodedData,
            let data = Data(base64Encoded: encodedData)
        {
            attachmentImage(data: data)
                .resizable()
                .aspectRatio(contentMode: .fit)
                .frame(maxHeight: 240)
                .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius, style: .continuous))
        } else {
            Label("Image unavailable", systemImage: mediaType.hasPrefix("image/") ? "photo" : "exclamationmark.triangle")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private func documentAttachmentView(encodedData: String?, filename: String?) -> some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            if
                let encodedData,
                let data = Data(base64Encoded: encodedData)
            {
                pdfThumbnail(data: data)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 56, height: 72)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            } else {
                Image(systemName: "doc.richtext")
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(Color.fawxTextSecondary)
                    .frame(width: 56, height: 72)
                    .background(Color.fawxBackground)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            }

            VStack(alignment: .leading, spacing: 4) {
                Text(filename ?? "Document")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
                    .lineLimit(2)

                Text("Open document")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            Spacer(minLength: 0)
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderMedium), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private func openAttachment() {
        guard
            let payload = attachmentPayload,
            let data = Data(base64Encoded: payload.data)
        else {
            return
        }

        AttachmentPreviewPresenter.present(
            data: data,
            filename: payload.filename
        )
    }

    private var attachmentPayload: (data: String, filename: String)? {
        switch block {
        case .image(let mediaType, let data):
            guard let data else {
                return nil
            }
            let ext = mediaType.components(separatedBy: "/").last ?? "png"
            return (data, "image.\(ext)")
        case .document(_, let data, let filename):
            guard let data else {
                return nil
            }
            return (data, filename ?? "document.pdf")
        case .text, .toolUse, .toolResult:
            return nil
        }
    }

    #if os(macOS)
    private func attachmentImage(data: Data) -> Image {
        if let image = NSImage(data: data) {
            return Image(nsImage: image)
        }

        return Image(systemName: "photo")
    }

    private func pdfThumbnail(data: Data) -> Image {
        if
            let document = PDFDocument(data: data),
            let page = document.page(at: 0)
        {
            return Image(nsImage: page.thumbnail(of: NSSize(width: 120, height: 160), for: .mediaBox))
        }

        return Image(systemName: "doc.richtext")
    }
    #else
    private func attachmentImage(data: Data) -> Image {
        if let image = UIImage(data: data) {
            return Image(uiImage: image)
        }

        return Image(systemName: "photo")
    }

    private func pdfThumbnail(data: Data) -> Image {
        if
            let document = PDFDocument(data: data),
            let page = document.page(at: 0)
        {
            return Image(uiImage: page.thumbnail(of: CGSize(width: 120, height: 160), for: .mediaBox))
        }

        return Image(systemName: "doc.richtext")
    }
    #endif
}

private extension SessionContentBlock {
    var isRenderableAttachment: Bool {
        switch self {
        case .image, .document:
            return true
        case .text, .toolUse, .toolResult:
            return false
        }
    }
}
