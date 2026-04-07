import SwiftUI
#if os(macOS)
import AppKit
#endif
#if canImport(PDFKit)
import PDFKit
#endif
#if os(iOS)
import UIKit
#endif

struct InputBar: View {
    @Binding var text: String
    @State private var isPresentingModelSelector = false
#if os(macOS)
    @State private var macComposerHeight: CGFloat = macComposerMinimumHeight
#endif

    let queuedMessage: String?
    let pendingAttachments: [PendingAttachment]
    let isStreaming: Bool
    let connectionStatus: ConnectionStatus
    let currentPhase: String?
    let activeModel: ModelInfo?
    let availableModels: [ModelInfo]
    let thinkingLevel: ThinkingLevel?
    let availableThinkingLevels: [ThinkingLevel]
    let isUpdatingServerSettings: Bool
    let placeholder: String
    let sendAction: () -> Void
    let stopAction: () -> Void
    let dismissQueuedMessage: () -> Void
    let removeAttachment: (UUID) -> Void
    let previewAttachment: (PendingAttachment) -> Void
    let showAttachmentPicker: () -> Void
    let showPhotoLibrary: () -> Void
    let showCamera: () -> Void
    let showFiles: () -> Void
    let pasteImage: (Data) -> Void
    let selectModel: (String) -> Void
    let selectThinking: (ThinkingLevel) -> Void

    var body: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            if let queuedMessage, !queuedMessage.isEmpty {
                QueuedMessageChip(text: queuedMessage, dismiss: dismissQueuedMessage)
            }

            if !pendingAttachments.isEmpty {
                attachmentPreviewStrip
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                messageFieldPanel
                controlsRow
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface.opacity(FawxOpacity.surfaceStrong))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .fawxShadow(FawxShadow.floatingPanel)
        .sheet(isPresented: $isPresentingModelSelector) {
            NavigationStack {
                ModelSelectionList(
                    models: availableModels,
                    selectedModelID: activeModel?.modelID,
                    disableSelection: modelMenuDisabled,
                    selectModel: { modelID in
                        isPresentingModelSelector = false
                        selectModel(modelID)
                    }
                )
                .navigationTitle("Select Model")
#if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
#endif
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") {
                            isPresentingModelSelector = false
                        }
                    }
                }
#if os(macOS)
                .frame(minWidth: 500, minHeight: 420)
#endif
            }
            .fawxOpaqueModalPresentation()
        }
    }

    private var effectivePlaceholder: String {
        if connectionStatus != .connected && !isStreaming {
            return "Reconnecting..."
        }
        if let currentPhase, currentPhase.isEmpty == false {
            return currentPhase
        }
        return placeholder
    }

    @ViewBuilder
    private var messageField: some View {
#if os(macOS)
        ZStack(alignment: .topLeading) {
            if text.isEmpty {
                Text(effectivePlaceholder)
                    .font(FawxTypography.input)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .padding(.top, FawxSpacing.paddingXS + 1)
                    .allowsHitTesting(false)
            }

            MacComposerTextView(
                text: $text,
                measuredHeight: $macComposerHeight,
                textColor: NSColor(Color.fawxText),
                font: .systemFont(ofSize: FawxTypography.chatBodyPointSize),
                onPasteImage: pasteImage,
                onSend: performPrimaryAction
            )
            .frame(height: macComposerHeight)
        }
#else
        baseMessageField
#endif
    }

#if os(iOS)
    private var baseMessageField: some View {
        TextField(effectivePlaceholder, text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .font(FawxTypography.input)
            .foregroundStyle(Color.fawxText)
            .lineLimit(1 ... 6)
            .accessibilityIdentifier("messageInput")
            .frame(maxWidth: .infinity, alignment: .leading)
    }
#endif

    private var messageFieldPanel: some View {
        HStack(alignment: .bottom, spacing: FawxSpacing.paddingSM) {
            attachmentButton
            messageField
        }
            .padding(.horizontal, FawxSpacing.paddingMD)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(messageFieldBackground)
            .overlay(
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .stroke(messageFieldBorderColor, lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private var attachmentPreviewStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: FawxSpacing.paddingSM) {
                ForEach(pendingAttachments) { attachment in
                    PendingAttachmentChipView(
                        attachment: attachment,
                        removeAttachment: { removeAttachment(attachment.id) },
                        previewAttachment: { previewAttachment(attachment) }
                    )
                }
            }
            .padding(.vertical, 2)
        }
    }

    private var controlsRow: some View {
        HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
            modelMenu
            thinkingMenu
            Spacer(minLength: 0)
            primaryButton
        }
    }

    @ViewBuilder
    private var attachmentButton: some View {
#if os(iOS)
        Menu {
            Button("Photo Library", action: showPhotoLibrary)
            Button("Camera", action: showCamera)
            Button("Files", action: showFiles)
        } label: {
            attachmentButtonLabel
        }
#else
        Button(action: showAttachmentPicker) {
            attachmentButtonLabel
        }
#endif
    }

    private var attachmentButtonLabel: some View {
        Image(systemName: "paperclip")
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(Color.fawxTextSecondary)
            .frame(width: 32, height: 32)
            .background(Color.fawxSurfaceHover)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .accessibilityIdentifier("attachmentButton")
            .accessibilityLabel("Attach files")
    }

    private var modelMenu: some View {
        Button {
            guard !modelMenuDisabled else {
                return
            }
            isPresentingModelSelector = true
        } label: {
            HStack(spacing: 6) {
                ModelBadge(
                    title: compactModelName(activeModel?.modelID ?? "Unavailable", limit: 20),
                    accessibilityLabel: "Selected model \(abbreviateModelName(activeModel?.modelID ?? "Unavailable"))"
                )

                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
        .buttonStyle(.plain)
        .disabled(modelMenuDisabled)
        .help(modelHelpText)
    }

    private var thinkingMenu: some View {
        Menu {
            ForEach(availableThinkingLevels, id: \.self) { level in
                Button(displayThinkingLevel(level, modelID: activeModel?.modelID)) {
                    selectThinking(level)
                }
            }
        } label: {
            ModelBadge(
                title: displayThinkingLevel(thinkingLevel, modelID: activeModel?.modelID),
                accessibilityLabel: "Thinking level \(displayThinkingLevel(thinkingLevel, modelID: activeModel?.modelID))"
            )
        }
        .disabled(thinkingMenuDisabled)
        .help(isStreaming ? "Cannot change thinking while a response is streaming." : "Server thinking level")
    }

    private var primaryButton: some View {
        Button(primaryButtonTitle) {
            performPrimaryAction()
        }
        .buttonStyle(.borderedProminent)
        .tint(primaryButtonTint)
        .keyboardShortcut(.return, modifiers: .command)
        .accessibilityIdentifier("sendButton")
        .accessibilityLabel(primaryButtonTitle)
        .disabled(primaryButtonDisabled)
        .frame(minWidth: 80)
    }

    private var modelMenuDisabled: Bool {
        isStreaming || isUpdatingServerSettings || availableModels.isEmpty
    }

    private var thinkingMenuDisabled: Bool {
        isStreaming || isUpdatingServerSettings || availableThinkingLevels.isEmpty
    }

    private var primaryButtonTitle: String {
        if isStreaming
            && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && pendingAttachments.isEmpty
        {
            return "Stop"
        }
        return "Send"
    }

    private var primaryButtonTint: Color {
        primaryButtonTitle == "Stop" ? .fawxError : .fawxAccent
    }

    private var primaryButtonDisabled: Bool {
        if primaryButtonTitle == "Stop" {
            return false
        }
        guard connectionStatus == .connected else {
            return true
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && pendingAttachments.isEmpty
    }

    private var modelHelpText: String {
        let activeModelName = activeModel.map { abbreviateModelName($0.modelID) } ?? "Server model unavailable"
        if isStreaming {
            return "\(activeModelName)\nCannot change model while a response is streaming."
        }
        return activeModelName
    }

    private var messageFieldBackground: Color {
        if connectionStatus != .connected && !isStreaming {
            return Color.fawxSurfaceHover
        }
        return Color.fawxBackground
    }

    private var messageFieldBorderColor: Color {
        if isStreaming && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return Color.fawxAccent.opacity(FawxOpacity.accentBorder)
        }
        if connectionStatus != .connected && !isStreaming {
            return Color.fawxWarning.opacity(FawxOpacity.warningBorder)
        }
        return Color.fawxBorder.opacity(FawxOpacity.borderEmphasis)
    }

    private func performPrimaryAction() {
        guard !primaryButtonDisabled else {
            return
        }

        if isStreaming && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            stopAction()
            return
        }

        FawxHaptics.lightImpact()
        sendAction()
    }
}

#if os(macOS)
private let macComposerMinimumHeight = FawxSpacing.inputBarMinHeight - (FawxSpacing.paddingSM * 2)
private let macComposerMaximumHeight = FawxSpacing.inputBarMaxHeight - (FawxSpacing.paddingSM * 2)

private struct MacComposerTextView: NSViewRepresentable {
    @Binding var text: String
    @Binding var measuredHeight: CGFloat

    let textColor: NSColor
    let font: NSFont
    let onPasteImage: (Data) -> Void
    let onSend: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text, measuredHeight: $measuredHeight)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay

        let textView = ComposerNSTextView()
        textView.delegate = context.coordinator
        textView.drawsBackground = false
        textView.isRichText = false
        textView.importsGraphics = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticDataDetectionEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = true
        textView.isContinuousSpellCheckingEnabled = true
        textView.allowsUndo = true
        textView.textContainerInset = NSSize(width: 0, height: FawxSpacing.paddingXS)
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        textView.minSize = NSSize(width: 0, height: FawxSpacing.inputBarMinHeight - (FawxSpacing.paddingMD * 2))
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.onSend = onSend
        textView.onHeightChange = { height in
            context.coordinator.updateMeasuredHeight(height)
        }
        textView.string = text
        textView.textColor = textColor
        textView.font = font
        textView.onPasteImage = onPasteImage

        scrollView.documentView = textView
        DispatchQueue.main.async {
            textView.refreshMeasuredHeight()
        }
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? ComposerNSTextView else {
            return
        }

        if textView.string != text {
            textView.string = text
        }

        textView.textColor = textColor
        textView.font = font
        textView.onPasteImage = onPasteImage
        textView.onSend = onSend
        textView.onHeightChange = { height in
            context.coordinator.updateMeasuredHeight(height)
        }
        textView.refreshMeasuredHeight()
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        @Binding private var text: String
        @Binding private var measuredHeight: CGFloat

        init(text: Binding<String>, measuredHeight: Binding<CGFloat>) {
            _text = text
            _measuredHeight = measuredHeight
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else {
                return
            }

            text = textView.string
        }

        func updateMeasuredHeight(_ height: CGFloat) {
            let clampedHeight = min(
                max(height, macComposerMinimumHeight),
                macComposerMaximumHeight
            )
            guard abs(measuredHeight - clampedHeight) > 0.5 else {
                return
            }

            measuredHeight = clampedHeight
        }
    }
}

private final class ComposerNSTextView: NSTextView {
    var onPasteImage: ((Data) -> Void)?
    var onSend: () -> Void = {}
    var onHeightChange: ((CGFloat) -> Void)?

    override func keyDown(with event: NSEvent) {
        let isReturnKey = event.keyCode == 36 || event.keyCode == 76
        guard isReturnKey, !hasMarkedText() else {
            super.keyDown(with: event)
            return
        }

        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let relevantModifiers = modifiers.intersection([.shift, .command, .option, .control])

        switch relevantModifiers {
        case [.shift]:
            insertNewlineIgnoringFieldEditor(nil)
        case [], [.command]:
            onSend()
        default:
            super.keyDown(with: event)
        }
    }

    override func didChangeText() {
        super.didChangeText()
        refreshMeasuredHeight()
    }

    override func layout() {
        super.layout()
        refreshMeasuredHeight()
    }

    override func paste(_ sender: Any?) {
        if let data = pastedImageData() {
            onPasteImage?(data)
            return
        }

        super.paste(sender)
    }

    func refreshMeasuredHeight() {
        guard let textContainer, let layoutManager else {
            return
        }

        layoutManager.ensureLayout(for: textContainer)
        let usedRect = layoutManager.usedRect(for: textContainer)
        let measuredHeight = ceil(usedRect.height + (textContainerInset.height * 2))
        onHeightChange?(measuredHeight)
    }

    private func pastedImageData() -> Data? {
        let pasteboard = NSPasteboard.general
        let preferredTypes: [NSPasteboard.PasteboardType] = [
            .png,
            .tiff,
            .fileURL,
        ]

        for type in preferredTypes {
            guard let data = pasteboard.data(forType: type) else {
                continue
            }

            if type == .png {
                return data
            }

            if type == .tiff,
               let imageRep = NSBitmapImageRep(data: data),
               let pngData = imageRep.representation(using: .png, properties: [:]) {
                return pngData
            }

            if type == .fileURL,
               let url = NSURL(
                absoluteURLWithDataRepresentation: data,
                relativeTo: nil
               ) as URL?,
               let image = NSImage(contentsOf: url),
               let pngData = pngData(from: image) {
                return pngData
            }
        }

        return nil
    }

    private func pngData(from image: NSImage) -> Data? {
        guard
            let tiffData = image.tiffRepresentation,
            let imageRep = NSBitmapImageRep(data: tiffData)
        else {
            return nil
        }

        return imageRep.representation(using: .png, properties: [:])
    }
}
#endif

private struct PendingAttachmentChipView: View {
    let attachment: PendingAttachment
    let removeAttachment: () -> Void
    let previewAttachment: () -> Void

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Button(action: previewAttachment) {
                HStack(spacing: FawxSpacing.paddingSM) {
                    attachmentPreview
                    Text(attachment.filename)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)
                }
            }
            .buttonStyle(.plain)

            Button(action: removeAttachment) {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(Color.fawxTextSecondary)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Remove \(attachment.filename)")
        }
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, FawxSpacing.paddingXS)
        .background(Color.fawxSurfaceHover)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderMedium), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    @ViewBuilder
    private var attachmentPreview: some View {
        switch attachment.kind {
        case .image:
            AttachmentThumbnailPreview(data: attachment.data, kind: .image)
        case .pdf:
            AttachmentThumbnailPreview(data: attachment.data, kind: .pdf)
        case .textFile:
            Image(systemName: "doc.text")
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }
}

private enum AttachmentThumbnailKind {
    case image
    case pdf
}

private struct AttachmentThumbnailPreview: View {
    let data: Data
    let kind: AttachmentThumbnailKind

    var body: some View {
        Group {
            switch kind {
            case .image:
                platformImage(data: data)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            case .pdf:
                pdfPreviewImage(data: data)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            }
        }
        .frame(width: 28, height: 28)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    #if os(macOS)
    private func platformImage(data: Data) -> Image {
        if let image = NSImage(data: data) {
            return Image(nsImage: image)
        }

        return Image(systemName: "photo")
    }

    private func pdfPreviewImage(data: Data) -> Image {
        if
            let document = PDFDocument(data: data),
            let page = document.page(at: 0)
        {
            return Image(nsImage: page.thumbnail(of: NSSize(width: 80, height: 80), for: .mediaBox))
        }

        return Image(systemName: "doc.richtext")
    }
    #else
    private func platformImage(data: Data) -> Image {
        if let image = UIImage(data: data) {
            return Image(uiImage: image)
        }

        return Image(systemName: "photo")
    }

    private func pdfPreviewImage(data: Data) -> Image {
        if
            let document = PDFDocument(data: data),
            let page = document.page(at: 0)
        {
            return Image(uiImage: page.thumbnail(of: CGSize(width: 80, height: 80), for: .mediaBox))
        }

        return Image(systemName: "doc.richtext")
    }
    #endif
}
