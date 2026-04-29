import SwiftUI

#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

struct CodeBlock: View {
    let language: String?
    let content: String

    @State private var isHovering = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Text((language?.isEmpty == false ? language! : "plain text").uppercased())
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Spacer()

                if shouldShowCopyButton {
                    Button {
                        copyContent()
                    } label: {
                        Label("Copy", systemImage: "doc.on.doc")
                            .font(FawxTypography.status)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Color.fawxTextSecondary)
                }
            }
            .padding(.horizontal, FawxSpacing.paddingMD)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(Color.fawxSurfaceHover)

            ScrollView(.horizontal, showsIndicators: true) {
#if os(macOS)
                SelectableCodeBlockText(content: content)
                    .fixedSize(horizontal: true, vertical: true)
                    .padding(FawxSpacing.paddingMD)
#else
                Text(content)
                    .font(FawxTypography.code)
                    .foregroundStyle(Color.fawxText)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(FawxSpacing.paddingMD)
#endif
            }
            .background(Color.fawxCode)
        }
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .onHover { hovering in
            isHovering = hovering
        }
    }

    private var shouldShowCopyButton: Bool {
#if os(macOS)
        isHovering
#else
        true
#endif
    }

    private func copyContent() {
#if os(iOS)
        UIPasteboard.general.string = content
#elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(content, forType: .string)
#endif
    }
}

#if os(macOS)
private struct SelectableCodeBlockText: NSViewRepresentable {
    let content: String

    func makeNSView(context: Context) -> IntrinsicCodeTextView {
        let textView = IntrinsicCodeTextView()
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.usesFindBar = true
        textView.font = Self.codeFont
        textView.textColor = NSColor(Color.fawxText)
        textView.textContainerInset = .zero
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = false
        textView.textContainer?.heightTracksTextView = false
        textView.textContainer?.containerSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.isHorizontallyResizable = true
        textView.isVerticallyResizable = true
        textView.minSize = .zero
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.applyFawxTextSelectionChrome()
        return textView
    }

    func updateNSView(_ textView: IntrinsicCodeTextView, context: Context) {
        let attributed = attributedString
        textView.font = Self.codeFont
        textView.textColor = NSColor(Color.fawxText)
        if textView.string != attributed.string {
            let selectedRanges = textView.selectedRanges
            textView.textStorage?.setAttributedString(attributed)
            textView.restoreValidSelectedRanges(selectedRanges)
        }
        textView.applyFawxTextSelectionChrome()
        textView.invalidateIntrinsicContentSize()
    }

    private var attributedString: NSAttributedString {
        let attributed = NSMutableAttributedString(string: content)
        let fullRange = NSRange(location: 0, length: attributed.length)
        guard fullRange.length > 0 else {
            return attributed
        }
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineBreakMode = .byClipping
        attributed.addAttributes(
            [
                .font: Self.codeFont,
                .foregroundColor: NSColor(Color.fawxText),
                .paragraphStyle: paragraphStyle,
            ],
            range: fullRange
        )
        return attributed
    }

    private static let codeFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
}

private final class IntrinsicCodeTextView: NSTextView {
    override var intrinsicContentSize: NSSize {
        guard let textContainer, let layoutManager else {
            return NSSize(width: 0, height: 0)
        }

        textContainer.containerSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        layoutManager.ensureLayout(for: textContainer)
        let usedRect = layoutManager.usedRect(for: textContainer)
        return NSSize(
            width: ceil(max(usedRect.width, 1)),
            height: ceil(max(usedRect.height, fallbackLineHeight))
        )
    }

    private var fallbackLineHeight: CGFloat {
        guard let font else {
            return 1
        }
        return max(font.ascender - font.descender + font.leading, 1)
    }

    override func layout() {
        super.layout()
        keepUnboundedTextContainer()
        invalidateIntrinsicContentSize()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        keepUnboundedTextContainer()
        invalidateIntrinsicContentSize()
    }

    private func keepUnboundedTextContainer() {
        textContainer?.containerSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
    }

    func restoreValidSelectedRanges(_ ranges: [NSValue]) {
        let textLength = (string as NSString).length
        let validRanges = ranges.compactMap { value -> NSValue? in
            let range = value.rangeValue
            guard range.location != NSNotFound,
                  range.location <= textLength,
                  NSMaxRange(range) <= textLength
            else {
                return nil
            }
            return value
        }
        if !validRanges.isEmpty {
            selectedRanges = validRanges
        }
    }
}
#endif
