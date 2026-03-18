import SwiftUI
#if os(macOS)
import AppKit
#endif
#if os(iOS)
import UIKit
#endif

struct InputBar: View {
    @Binding var text: String
#if os(macOS)
    @State private var macComposerHeight: CGFloat = macComposerMinimumHeight
#endif

    let queuedMessage: String?
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
    let selectModel: (String) -> Void
    let selectThinking: (ThinkingLevel) -> Void

    var body: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            if let queuedMessage, !queuedMessage.isEmpty {
                QueuedMessageChip(text: queuedMessage, dismiss: dismissQueuedMessage)
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                messageFieldPanel
                controlsRow
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface.opacity(0.98))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .shadow(color: .black.opacity(0.08), radius: 10, y: 3)
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
                onSend: performPrimaryAction
            )
            .frame(height: macComposerHeight)
        }
#else
        baseMessageField
#endif
    }

    private var baseMessageField: some View {
#if os(macOS)
        EmptyView()
#else
        TextField(effectivePlaceholder, text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .font(FawxTypography.input)
            .foregroundStyle(Color.fawxText)
            .lineLimit(1 ... 6)
            .accessibilityIdentifier("messageInput")
#if os(macOS)
            .padding(.vertical, FawxSpacing.paddingXS)
            .frame(
                maxWidth: .infinity,
                minHeight: FawxSpacing.inputBarMinHeight - (FawxSpacing.paddingMD * 2),
                alignment: .leading
            )
#else
            .frame(maxWidth: .infinity, alignment: .leading)
#endif
#endif
    }

    private var messageFieldPanel: some View {
        messageField
            .padding(.horizontal, FawxSpacing.paddingMD)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(messageFieldBackground)
            .overlay(
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .stroke(messageFieldBorderColor, lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }

    private var controlsRow: some View {
        HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
            modelMenu
            thinkingMenu
            Spacer(minLength: 0)
            primaryButton
        }
    }

    private var modelMenu: some View {
        Menu {
            ForEach(availableModels) { model in
                Button(compactModelName(model.modelID, limit: 28)) {
                    selectModel(model.modelID)
                }
            }
        } label: {
            ModelBadge(
                title: compactModelName(activeModel?.modelID ?? "Unavailable", limit: 20),
                accessibilityLabel: "Selected model \(abbreviateModelName(activeModel?.modelID ?? "Unavailable"))"
            )
        }
        .disabled(modelMenuDisabled)
        .help(modelHelpText)
    }

    private var thinkingMenu: some View {
        Menu {
            ForEach(availableThinkingLevels, id: \.self) { level in
                Button(level.displayName) {
                    selectThinking(level)
                }
            }
        } label: {
            ModelBadge(
                title: displayThinkingLevel(thinkingLevel),
                accessibilityLabel: "Thinking level \(displayThinkingLevel(thinkingLevel))"
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
        if isStreaming && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
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
            return Color.fawxAccent.opacity(0.2)
        }
        if connectionStatus != .connected && !isStreaming {
            return Color.fawxWarning.opacity(0.28)
        }
        return Color.fawxBorder.opacity(0.9)
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

    func refreshMeasuredHeight() {
        guard let textContainer, let layoutManager else {
            return
        }

        layoutManager.ensureLayout(for: textContainer)
        let usedRect = layoutManager.usedRect(for: textContainer)
        let measuredHeight = ceil(usedRect.height + (textContainerInset.height * 2))
        onHeightChange?(measuredHeight)
    }
}
#endif
