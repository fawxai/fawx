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
    @State private var didCopy = false

    let role: MessageRole
    let content: String
    let timestamp: Int?
    let isStreaming: Bool
    let footnoteText: String?
    private let isFinalAnswer: Bool
    private let contentBlocks: [SessionContentBlock]?

    init(message: SessionMessage, footnoteText: String? = nil) {
        self.role = message.role
        self.content = message.content
        self.timestamp = message.timestamp
        self.isStreaming = false
        self.footnoteText = footnoteText
        self.isFinalAnswer = false
        self.contentBlocks = message.contentBlocks
    }

    init(transcriptMessage: TranscriptMessage, isFinalAnswer: Bool = false) {
        self.role = transcriptMessage.message.role
        self.content = transcriptMessage.displayText
        self.timestamp = transcriptMessage.message.timestamp
        self.isStreaming = transcriptMessage.isStreaming
        self.footnoteText = transcriptMessage.footnoteText
        self.isFinalAnswer = isFinalAnswer

        if transcriptMessage.message.role == .assistant {
            self.contentBlocks = [.text(transcriptMessage.displayText)]
        } else {
            self.contentBlocks = transcriptMessage.message.contentBlocks
        }
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
        self.isFinalAnswer = false
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
#if !os(macOS)
        .textSelection(.enabled)
#endif
    }

    private var bubbleContent: some View {
        HStack {
            if role == .user {
                Spacer(minLength: FawxSpacing.transcriptEdgeClamp)
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                bubbleLabel
                    .padding(.horizontal, bubbleHorizontalPadding)
                    .padding(.vertical, FawxSpacing.paddingMD)
                    .background {
                        if role == .user {
                            RoundedRectangle(
                                cornerRadius: FawxSpacing.cornerRadius,
                                style: .continuous
                            )
                            .fill(Color.fawxSurfaceHover.opacity(FawxOpacity.surfaceMuted))
                        }
                    }

                footer
            }
            .frame(
                maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth),
                alignment: .leading
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

    @ViewBuilder
    private var footer: some View {
        if role == .assistant && !isStreaming && !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            assistantFooter
        } else if timestamp != nil || footnoteText != nil {
            timestampFooter
        }
    }

    private var assistantFooter: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            Button(action: copyResponse) {
                Image(systemName: didCopy ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Color.fawxTextSecondary)
                    .frame(width: 18, height: 18)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(didCopy ? "Copied" : "Copy response")
            .accessibilityLabel(didCopy ? "Copied response" : "Copy response")

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
        .padding(.leading, bubbleHorizontalPadding)
    }

    private var timestampFooter: some View {
        VStack(alignment: .leading, spacing: 2) {
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

    @ViewBuilder
    private var messageContent: some View {
        if role == .tool {
#if os(macOS)
            SelectableTranscriptText(
                text: toolDisplayContent,
                style: .code,
                alignment: .left
            )
#else
            Text(verbatim: toolDisplayContent)
                .font(FawxTypography.code)
                .foregroundStyle(Color.fawxText)
                .textSelection(.enabled)
#endif
        } else {
            structuredMessageContent
        }
    }

    @ViewBuilder
    private var structuredMessageContent: some View {
        let attachmentBlocks = displayBlocks.filter(\.isRenderableAttachment)
        let renderedText = structuredTextContent

        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            if !attachmentBlocks.isEmpty {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    ForEach(Array(attachmentBlocks.enumerated()), id: \.offset) { _, block in
                        MessageAttachmentBlockView(block: block)
                    }
                }
            }

            if !renderedText.isEmpty || isStreaming {
                switch role {
                case .assistant:
#if os(macOS)
                    TranscriptMarkdownContentView(
                        text: renderedText + (isStreaming ? "▍" : ""),
                        alignment: .left
                    )
#else
                    TranscriptMarkdownText(text: renderedText + (isStreaming ? "▍" : ""))
#endif
                case .user:
#if os(macOS)
                    SelectableTranscriptText(
                        text: renderedText,
                        style: .plain,
                        alignment: .left
                    )
#else
                    Text(renderedText)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxText)
                        .textSelection(.enabled)
#endif
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

    private var accessibilityIdentifier: String {
        switch role {
        case .user:
            return "userMessage"
        case .assistant:
            if isFinalAnswer {
                return isStreaming ? "streamingFinalAnswerMessage" : "finalAnswerMessage"
            }
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

    private func copyResponse() {
#if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(content, forType: .string)
#else
        UIPasteboard.general.string = content
#endif
        didCopy = true
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            didCopy = false
        }
    }
}

struct TranscriptMarkdownText: View {
    let text: String

    var body: some View {
        Markdown(text)
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
    }
}

#if os(macOS)
enum TranscriptMarkdownRenderer {
    private static let maximumRenderedTableWidth = 88
    private static let maximumWrappedTableColumnWidth = 48
    private static let minimumWrappedTableColumnWidth = 8
    private static let linkDetector = try? NSDataDetector(
        types: NSTextCheckingResult.CheckingType.link.rawValue
    )

    struct Table: Equatable {
        let rows: [[String]]

        var columnCount: Int {
            rows.map(\.count).max() ?? 0
        }

        var isEmpty: Bool {
            rows.isEmpty || columnCount == 0
        }

        func columnWidth(at index: Int) -> CGFloat {
            let bodyFont = NSFont.systemFont(ofSize: NSFont.systemFontSize)
            let headerFont = NSFont.systemFont(ofSize: NSFont.systemFontSize, weight: .semibold)
            let widestCell = rows
                .enumerated()
                .map { rowIndex, row -> CGFloat in
                    guard index < row.count else {
                        return 0
                    }
                    let font = rowIndex == 0 ? headerFont : bodyFont
                    return ceil(
                        (row[index] as NSString).size(withAttributes: [.font: font]).width
                    )
                }
                .max() ?? 0
            return min(max(widestCell, 96), 760)
        }
    }

    enum Block: Equatable {
        case text(String)
        case table(Table)
        case codeBlock(language: String?, content: String)
    }

    static var linkTextAttributes: [NSAttributedString.Key: Any] {
        [
            .foregroundColor: NSColor(Color.fawxAccent),
            .underlineStyle: NSUnderlineStyle.single.rawValue,
        ]
    }

    private enum SegmentStyle {
        case body
        case heading(Int)
        case table(Table)
        case codeBlock(language: String?)

        var isNonMarkdownBlock: Bool {
            if case .table = self {
                return true
            }
            if case .codeBlock = self {
                return true
            }
            return false
        }
    }

    private struct Segment {
        let text: String
        let style: SegmentStyle
    }

    static func displayText(for text: String) -> String {
        segments(for: text).map(\.text).joined()
    }

    static func blocks(for text: String) -> [Block] {
        var blocks: [Block] = []
        var textBuffer = ""

        func flushTextBuffer() {
            guard !textBuffer.isEmpty else {
                return
            }
            blocks.append(.text(textBuffer))
            textBuffer = ""
        }

        for segment in segments(for: text) {
            switch segment.style {
            case .table(let table):
                flushTextBuffer()
                blocks.append(.table(table))
            case .codeBlock(let language):
                flushTextBuffer()
                blocks.append(.codeBlock(language: language, content: segment.text))
            case .body, .heading:
                textBuffer += segment.text
            }
        }

        flushTextBuffer()
        return blocks
    }

    static func attributedString(
        for text: String,
        alignment: NSTextAlignment,
        baseFont: NSFont
    ) -> NSAttributedString {
        let attributed = NSMutableAttributedString()

        for segment in segments(for: text) {
            let rendered = attributedString(for: segment)
            applyAttributes(
                to: rendered,
                style: segment.style,
                alignment: alignment,
                baseFont: baseFont
            )
            attributed.append(rendered)
        }

        return attributed
    }

    private static func segments(for text: String) -> [Segment] {
        guard !text.isEmpty else {
            return []
        }

        let normalizedText = text
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        let lines = normalizedText
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map(String.init)
        var segments: [Segment] = []
        var index = 0

        func appendLineBreak(after consumedLastLineIndex: Int) {
            if consumedLastLineIndex < lines.count - 1 {
                segments.append(Segment(text: "\n", style: .body))
            }
        }

        while index < lines.count {
            if let codeBlock = fencedCodeBlock(startingAt: index, in: lines) {
                segments.append(
                    Segment(
                        text: codeBlock.content,
                        style: .codeBlock(language: codeBlock.language)
                    )
                )
                appendLineBreak(after: codeBlock.endIndex - 1)
                index = codeBlock.endIndex
                continue
            }

            if let table = tableBlock(startingAt: index, in: lines) {
                segments.append(Segment(text: table.text, style: .table(table.model)))
                appendLineBreak(after: table.endIndex - 1)
                index = table.endIndex
                continue
            }

            if let heading = atxHeading(from: lines[index]) {
                segments.append(Segment(text: heading.title, style: .heading(heading.level)))
                appendLineBreak(after: index)
                index += 1
                continue
            }

            if index + 1 < lines.count,
               let level = setextHeadingLevel(for: lines[index + 1]),
               !lines[index].trimmingCharacters(in: .whitespaces).isEmpty
            {
                segments.append(Segment(text: lines[index], style: .heading(level)))
                appendLineBreak(after: index + 1)
                index += 2
                continue
            }

            segments.append(Segment(text: lines[index], style: .body))
            appendLineBreak(after: index)
            index += 1
        }

        return segments
    }

    private static func attributedString(for segment: Segment) -> NSMutableAttributedString {
        guard segment.style.isNonMarkdownBlock else {
            if let parsed = try? AttributedString(
                markdown: segment.text,
                options: AttributedString.MarkdownParsingOptions(
                    interpretedSyntax: .inlineOnlyPreservingWhitespace
                )
            ) {
                return NSMutableAttributedString(attributedString: NSAttributedString(parsed))
            }

            return NSMutableAttributedString(string: segment.text)
        }

        return NSMutableAttributedString(string: segment.text)
    }

    private static func applyAttributes(
        to attributed: NSMutableAttributedString,
        style: SegmentStyle,
        alignment: NSTextAlignment,
        baseFont: NSFont
    ) {
        let fullRange = NSRange(location: 0, length: attributed.length)
        guard fullRange.length > 0 else {
            return
        }

        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.alignment = alignment
        paragraphStyle.lineBreakMode = .byWordWrapping
        paragraphStyle.paragraphSpacing = FawxSpacing.paddingXS

        attributed.addAttributes(
            [
                .foregroundColor: NSColor(Color.fawxText),
                .paragraphStyle: paragraphStyle,
            ],
            range: fullRange
        )

        switch style {
        case .body:
            attributed.enumerateAttribute(.font, in: fullRange) { value, range, _ in
                guard value == nil else {
                    return
                }
                attributed.addAttribute(.font, value: baseFont, range: range)
            }
            applyInlinePresentationAttributes(to: attributed, in: fullRange, baseFont: baseFont)
            applyLinkAttributes(to: attributed, in: fullRange)
        case .heading(let level):
            attributed.addAttribute(
                .font,
                value: headingFont(level: level, baseFont: baseFont),
                range: fullRange
            )
            applyInlinePresentationAttributes(
                to: attributed,
                in: fullRange,
                baseFont: headingFont(level: level, baseFont: baseFont)
            )
            applyLinkAttributes(to: attributed, in: fullRange)
        case .table:
            attributed.addAttribute(
                .font,
                value: NSFont.monospacedSystemFont(
                    ofSize: baseFont.pointSize * 0.92,
                    weight: .regular
                ),
                range: fullRange
            )
        case .codeBlock:
            attributed.addAttributes(
                [
                    .font: NSFont.monospacedSystemFont(
                        ofSize: baseFont.pointSize * 0.92,
                        weight: .regular
                    ),
                    .backgroundColor: NSColor(Color.fawxCode.opacity(FawxOpacity.codeBackground)),
                ],
                range: fullRange
            )
        }
    }

    private static func applyInlinePresentationAttributes(
        to attributed: NSMutableAttributedString,
        in fullRange: NSRange,
        baseFont: NSFont
    ) {
        let inlineIntentKey = NSAttributedString.Key("NSInlinePresentationIntent")
        attributed.enumerateAttribute(inlineIntentKey, in: fullRange) { value, range, _ in
            guard let mask = inlinePresentationMask(from: value) else {
                return
            }

            if mask & 4 != 0 {
                attributed.addAttributes(
                    [
                        .font: NSFont.monospacedSystemFont(
                            ofSize: baseFont.pointSize * 0.92,
                            weight: .regular
                        ),
                        .backgroundColor: NSColor(Color.fawxCode.opacity(FawxOpacity.codeBackground)),
                    ],
                    range: range
                )
            }

            if mask & 2 != 0 {
                attributed.addAttribute(
                    .font,
                    value: NSFont.systemFont(ofSize: baseFont.pointSize, weight: .semibold),
                    range: range
                )
            }
        }
    }

    private static func inlinePresentationMask(from value: Any?) -> Int? {
        if let value = value as? Int {
            return value
        }
        if let value = value as? NSNumber {
            return value.intValue
        }
        return nil
    }

    private static func applyLinkAttributes(
        to attributed: NSMutableAttributedString,
        in fullRange: NSRange
    ) {
        addBareURLLinks(to: attributed, in: fullRange)

        attributed.enumerateAttribute(.link, in: fullRange) { value, range, _ in
            guard value != nil else {
                return
            }
            attributed.addAttributes(
                linkTextAttributes,
                range: range
            )
        }
    }

    private static func addBareURLLinks(
        to attributed: NSMutableAttributedString,
        in fullRange: NSRange
    ) {
        guard let linkDetector else {
            return
        }

        let text = attributed.string as NSString
        linkDetector.enumerateMatches(
            in: attributed.string,
            options: [],
            range: fullRange
        ) { match, _, _ in
            guard let match,
                  let url = match.url,
                  match.range.location != NSNotFound,
                  NSMaxRange(match.range) <= text.length,
                  attributed.attribute(.link, at: match.range.location, effectiveRange: nil) == nil
            else {
                return
            }

            attributed.addAttribute(.link, value: url, range: match.range)
        }
    }

    private static func headingFont(level: Int, baseFont: NSFont) -> NSFont {
        let sizeIncrease: CGFloat
        switch level {
        case 1: sizeIncrease = 8
        case 2: sizeIncrease = 5
        case 3: sizeIncrease = 3
        case 4: sizeIncrease = 2
        default: sizeIncrease = 1
        }
        return .systemFont(ofSize: baseFont.pointSize + sizeIncrease, weight: .semibold)
    }

    private static func atxHeading(from line: String) -> (level: Int, title: String)? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        let level = trimmed.prefix(while: { $0 == "#" }).count
        guard (1...6).contains(level),
              trimmed.dropFirst(level).first?.isWhitespace == true
        else {
            return nil
        }

        var title = String(trimmed.dropFirst(level)).trimmingCharacters(in: .whitespaces)
        let closingHashCount = title.reversed().prefix(while: { $0 == "#" }).count
        if closingHashCount > 0 {
            let hashStart = title.index(title.endIndex, offsetBy: -closingHashCount)
            if hashStart > title.startIndex,
               title[title.index(before: hashStart)].isWhitespace
            {
                title = String(title[..<title.index(before: hashStart)])
                    .trimmingCharacters(in: .whitespaces)
            }
        }

        return (level, title)
    }

    private static func setextHeadingLevel(for line: String) -> Int? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else {
            return nil
        }

        if trimmed.allSatisfy({ $0 == "=" }) {
            return 1
        }
        if trimmed.count >= 3, trimmed.allSatisfy({ $0 == "-" }) {
            return 2
        }
        return nil
    }

    private static func fencedCodeBlock(startingAt index: Int, in lines: [String])
        -> (language: String?, content: String, endIndex: Int)?
    {
        let line = lines[index].trimmingCharacters(in: .whitespaces)
        let fence: String
        if line.hasPrefix("```") {
            fence = "```"
        } else if line.hasPrefix("~~~") {
            fence = "~~~"
        } else {
            return nil
        }

        let language = String(line.dropFirst(fence.count))
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
        var contentLines: [String] = []
        var scanIndex = index + 1

        while scanIndex < lines.count {
            let candidate = lines[scanIndex].trimmingCharacters(in: .whitespaces)
            if candidate.hasPrefix(fence) {
                return (language, contentLines.joined(separator: "\n"), scanIndex + 1)
            }
            contentLines.append(lines[scanIndex])
            scanIndex += 1
        }

        return (language, contentLines.joined(separator: "\n"), lines.count)
    }

    private static func tableBlock(startingAt index: Int, in lines: [String])
        -> (text: String, model: Table, endIndex: Int)?
    {
        guard index + 1 < lines.count,
              lines[index].contains("|"),
              isTableSeparatorLine(lines[index + 1])
        else {
            return nil
        }

        var rows = [tableCells(in: lines[index])]
        var scanIndex = index + 2
        while scanIndex < lines.count {
            let line = lines[scanIndex]
            guard line.contains("|"),
                  !line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                break
            }
            rows.append(tableCells(in: line))
            scanIndex += 1
        }

        return renderTable(rows, endIndex: scanIndex)
    }

    private static func isTableSeparatorLine(_ line: String) -> Bool {
        let cells = tableCells(in: line)
        guard !cells.isEmpty else {
            return false
        }

        return cells.allSatisfy { cell in
            let marker = cell.replacingOccurrences(of: " ", with: "")
            let core = marker.trimmingCharacters(in: CharacterSet(charactersIn: ":"))
            return core.count >= 3 && core.allSatisfy { $0 == "-" }
        }
    }

    private static func tableCells(in line: String) -> [String] {
        var trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.first == "|" {
            trimmed.removeFirst()
        }
        if trimmed.last == "|" {
            trimmed.removeLast()
        }

        return trimmed
            .split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }
    }

    private static func renderTable(
        _ rows: [[String]],
        endIndex: Int
    ) -> (text: String, model: Table, endIndex: Int)? {
        // The selectable transcript renderer lays tables out as one monospaced
        // text grid. Inline markdown is flattened here so width calculations
        // match the displayed text; the UI renders the same rows as a native
        // table block so tables do not degrade into a paragraph wall.
        let rows = rows.map { row in
            row.map(inlineMarkdownDisplayText)
        }
        let columnCount = rows.map(\.count).max() ?? 0
        guard columnCount > 0 else {
            return nil
        }

        let widths = (0..<columnCount).map { column in
            rows.map { row in
                column < row.count ? row[column].count : 0
            }
            .max() ?? 0
        }

        return (
            renderGridTable(rows, widths: tableColumnWidths(rows: rows, naturalWidths: widths)),
            Table(rows: rows),
            endIndex
        )
    }

    private static func shouldWrapTable(rows: [[String]], widths: [Int]) -> Bool {
        guard widths.count > 1 else {
            return false
        }

        let renderedWidth = widths.reduce(0, +) + max(0, widths.count - 1) * 2
        let hasLongCell = rows.dropFirst().contains { row in
            row.contains { cell in
                cell.count > 48 || cell.contains("\n")
            }
        }
        return renderedWidth > maximumRenderedTableWidth || hasLongCell
    }

    private static func tableColumnWidths(rows: [[String]], naturalWidths: [Int]) -> [Int] {
        guard shouldWrapTable(rows: rows, widths: naturalWidths) else {
            return naturalWidths
        }

        var widths = naturalWidths.map { width in
            min(max(width, minimumWrappedTableColumnWidth), maximumWrappedTableColumnWidth)
        }

        while renderedTableWidth(widths) > maximumRenderedTableWidth,
              let widestColumn = widths.indices.max(by: { widths[$0] < widths[$1] }),
              widths[widestColumn] > minimumWrappedTableColumnWidth
        {
            widths[widestColumn] -= 1
        }

        return widths
    }

    private static func renderedTableWidth(_ widths: [Int]) -> Int {
        widths.reduce(0, +) + max(0, widths.count - 1) * 2
    }

    private static func renderGridTable(_ rows: [[String]], widths: [Int]) -> String {
        var renderedRows: [String] = []
        for (rowIndex, row) in rows.enumerated() {
            renderedRows.append(contentsOf: renderTableRow(row, widths: widths))
            if rowIndex == 0 {
                renderedRows.append(
                    widths
                        .map { String(repeating: "-", count: max(3, $0)) }
                        .joined(separator: "  ")
                )
            }
        }

        return renderedRows.joined(separator: "\n")
    }

    private static func renderTableRow(_ row: [String], widths: [Int]) -> [String] {
        let cellLines = widths.indices.map { column in
            let value = column < row.count ? row[column] : ""
            return wrappedCellLines(value, width: widths[column])
        }
        let rowHeight = cellLines.map(\.count).max() ?? 1

        return (0..<rowHeight).map { lineIndex in
            widths.indices
                .map { column in
                    let value = lineIndex < cellLines[column].count ? cellLines[column][lineIndex] : ""
                    return paddedTableCell(value, width: widths[column])
                }
                .joined(separator: "  ")
                .trimmingCharacters(in: .whitespaces)
        }
    }

    private static func wrappedCellLines(_ text: String, width: Int) -> [String] {
        let width = max(1, width)
        let paragraphs = text
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map(String.init)
        var lines: [String] = []

        for paragraph in paragraphs {
            let words = paragraph.split(whereSeparator: \.isWhitespace).map(String.init)
            guard !words.isEmpty else {
                lines.append("")
                continue
            }

            var currentLine = ""
            for word in words {
                if word.count > width {
                    if !currentLine.isEmpty {
                        lines.append(currentLine)
                        currentLine = ""
                    }
                    var chunks = hardWrappedChunks(word, width: width)
                    currentLine = chunks.popLast() ?? ""
                    lines.append(contentsOf: chunks)
                    continue
                }

                if currentLine.isEmpty {
                    currentLine = word
                } else if currentLine.count + 1 + word.count <= width {
                    currentLine += " \(word)"
                } else {
                    lines.append(currentLine)
                    currentLine = word
                }
            }

            if !currentLine.isEmpty {
                lines.append(currentLine)
            }
        }

        return lines.isEmpty ? [""] : lines
    }

    private static func hardWrappedChunks(_ text: String, width: Int) -> [String] {
        // String indices advance by Character, so composed grapheme clusters
        // stay intact. Display width is still approximate; CJK/wide glyph
        // support belongs with the native table renderer follow-up.
        var chunks: [String] = []
        var remaining = text[...]
        while !remaining.isEmpty {
            let end = remaining.index(
                remaining.startIndex,
                offsetBy: min(width, remaining.count)
            )
            chunks.append(String(remaining[..<end]))
            remaining = remaining[end...]
        }
        return chunks
    }

    private static func paddedTableCell(_ value: String, width: Int) -> String {
        value + String(repeating: " ", count: max(0, width - value.count))
    }

    private static func inlineMarkdownDisplayText(_ markdown: String) -> String {
        if let parsed = try? AttributedString(
            markdown: markdown,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .inlineOnlyPreservingWhitespace
            )
        ) {
            return NSAttributedString(parsed).string
        }

        return markdown
    }
}

struct TranscriptMarkdownContentView: View {
    let text: String
    let alignment: NSTextAlignment

    private var blocks: [TranscriptMarkdownRenderer.Block] {
        TranscriptMarkdownRenderer.blocks(for: text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .text(let text):
                    SelectableTranscriptText(
                        text: text,
                        style: .markdown,
                        alignment: alignment
                    )
                case .table(let table):
                    TranscriptMarkdownTableView(table: table)
                case .codeBlock(let language, let content):
                    CodeBlock(language: language, content: content)
                }
            }
        }
    }
}

private struct TranscriptMarkdownTableView: View {
    let table: TranscriptMarkdownRenderer.Table

    var body: some View {
        if !table.isEmpty {
            ScrollView(.horizontal, showsIndicators: true) {
                Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                    ForEach(table.rows.indices, id: \.self) { rowIndex in
                        GridRow {
                            ForEach(0..<table.columnCount, id: \.self) { columnIndex in
                                tableCell(rowIndex: rowIndex, columnIndex: columnIndex)
                            }
                        }
                    }
                }
                .padding(FawxSpacing.paddingXS)
                .background {
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius, style: .continuous)
                        .fill(Color.fawxSurfaceHover.opacity(FawxOpacity.surfaceMuted))
                }
                .overlay {
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius, style: .continuous)
                        .stroke(Color.fawxBorder.opacity(FawxOpacity.borderMedium), lineWidth: 1)
                }
            }
            .scrollClipDisabled()
            .accessibilityElement(children: .contain)
        }
    }

    private func tableCell(rowIndex: Int, columnIndex: Int) -> some View {
        Text(cellText(rowIndex: rowIndex, columnIndex: columnIndex))
            .font(FawxTypography.chatBody)
            .fontWeight(rowIndex == 0 ? .semibold : .regular)
            .foregroundStyle(rowIndex == 0 ? Color.fawxText : Color.fawxTextSecondary)
            .frame(width: table.columnWidth(at: columnIndex), alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, FawxSpacing.paddingXS)
            .background {
                Rectangle()
                    .fill(rowIndex == 0 ? Color.fawxSurfaceActive.opacity(0.55) : Color.clear)
            }
            .overlay(alignment: .trailing) {
                if columnIndex < table.columnCount - 1 {
                    Rectangle()
                        .fill(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                        .frame(width: 1)
                }
            }
            .overlay(alignment: .bottom) {
                Rectangle()
                    .fill(Color.fawxBorder.opacity(FawxOpacity.borderSubtle))
                    .frame(height: 1)
            }
    }

    private func cellText(rowIndex: Int, columnIndex: Int) -> String {
        guard rowIndex < table.rows.count,
              columnIndex < table.rows[rowIndex].count
        else {
            return ""
        }
        return table.rows[rowIndex][columnIndex]
    }
}

struct SelectableTranscriptText: NSViewRepresentable {
    enum TextStyle {
        case markdown
        case plain
        case code
    }

    let text: String
    let style: TextStyle
    let alignment: NSTextAlignment

    func makeNSView(context: Context) -> IntrinsicSelectableTextView {
        let textView = IntrinsicSelectableTextView()
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.usesFindBar = true
        textView.linkTextAttributes = TranscriptMarkdownRenderer.linkTextAttributes
        textView.textContainerInset = .zero
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.heightTracksTextView = false
        textView.isHorizontallyResizable = false
        textView.isVerticallyResizable = true
        textView.applyFawxTextSelectionChrome()
        textView.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        textView.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return textView
    }

    func updateNSView(_ textView: IntrinsicSelectableTextView, context: Context) {
        let attributedString = attributedString
        if textView.string != attributedString.string {
            let selectedRanges = textView.selectedRanges
            textView.textStorage?.setAttributedString(attributedString)
            textView.restoreValidSelectedRanges(selectedRanges)
        }
        textView.alignment = alignment
        textView.applyFawxTextSelectionChrome()
        textView.invalidateIntrinsicContentSize()
    }

    private var attributedString: NSAttributedString {
        if style == .markdown {
            return TranscriptMarkdownRenderer.attributedString(
                for: text,
                alignment: alignment,
                baseFont: font
            )
        }

        let attributed = NSMutableAttributedString(string: text)
        let fullRange = NSRange(location: 0, length: attributed.length)
        guard fullRange.length > 0 else {
            return attributed
        }

        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.alignment = alignment
        paragraphStyle.lineBreakMode = .byWordWrapping
        paragraphStyle.paragraphSpacing = FawxSpacing.paddingXS

        attributed.addAttributes(
            [
                .font: font,
                .foregroundColor: NSColor(Color.fawxText),
                .paragraphStyle: paragraphStyle,
            ],
            range: fullRange
        )
        return attributed
    }

    private var font: NSFont {
        switch style {
        case .code:
            return .monospacedSystemFont(ofSize: 13, weight: .regular)
        case .markdown, .plain:
            return .systemFont(ofSize: FawxTypography.chatBodyPointSize, weight: .regular)
        }
    }
}

final class IntrinsicSelectableTextView: NSTextView {
    override func clicked(onLink link: Any, at charIndex: Int) {
        if let url = link as? URL {
            NSWorkspace.shared.open(url)
            return
        }
        if let url = link as? NSURL {
            NSWorkspace.shared.open(url as URL)
            return
        }
        if let string = link as? String,
           let url = URL(string: string)
        {
            NSWorkspace.shared.open(url)
            return
        }

        super.clicked(onLink: link, at: charIndex)
    }

    override var intrinsicContentSize: NSSize {
        guard let textContainer, let layoutManager else {
            return NSSize(width: NSView.noIntrinsicMetric, height: 0)
        }

        updateTextContainerSize()
        layoutManager.ensureLayout(for: textContainer)
        let usedRect = layoutManager.usedRect(for: textContainer)
        return NSSize(width: NSView.noIntrinsicMetric, height: ceil(usedRect.height))
    }

    override func layout() {
        super.layout()
        updateTextContainerSize()
        invalidateIntrinsicContentSize()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateTextContainerSize(width: newSize.width)
        invalidateIntrinsicContentSize()
    }

    private func updateTextContainerSize(width: CGFloat? = nil) {
        guard let textContainer else {
            return
        }

        let resolvedWidth = max(width ?? bounds.width, 1)
        textContainer.containerSize = NSSize(
            width: resolvedWidth,
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
