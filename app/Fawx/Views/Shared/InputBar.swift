import SwiftUI
#if os(iOS)
import UIKit
#endif

struct InputBar: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Binding var text: String

    let queuedMessage: String?
    let isStreaming: Bool
    let connectionStatus: ConnectionStatus
    let currentPhase: String?
    let activeModel: ModelInfo?
    let availableModels: [ModelInfo]
    let thinkingLevel: ThinkingLevel?
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

            if usesCompactMobileLayout {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    messageField
                    HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
                        modelMenu
                        thinkingMenu
                        Spacer(minLength: 0)
                        primaryButton
                    }
                }
            } else {
                HStack(alignment: .bottom, spacing: FawxSpacing.paddingMD) {
                    messageField

                    HStack(spacing: FawxSpacing.paddingSM) {
                        modelMenu
                        thinkingMenu
                        primaryButton
                    }
                }
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
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

    private var messageField: some View {
        TextField(effectivePlaceholder, text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .font(FawxTypography.input)
            .foregroundStyle(Color.fawxText)
            .lineLimit(1 ... 6)
            .accessibilityIdentifier("messageInput")
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var modelMenu: some View {
        Menu {
            ForEach(availableModels) { model in
                Button(compactModelName(model.modelID, limit: 28)) {
                    selectModel(model.modelID)
                }
            }
        } label: {
            ModelBadge(title: compactModelName(activeModel?.modelID ?? "Unavailable", limit: 20))
        }
        .disabled(modelMenuDisabled)
        .help(modelHelpText)
    }

    private var thinkingMenu: some View {
        Menu {
            ForEach(ThinkingLevel.phaseOneOptions, id: \.self) { level in
                Button(level.rawValue.capitalized) {
                    selectThinking(level)
                }
            }
        } label: {
            ModelBadge(title: displayThinkingLevel(thinkingLevel))
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
        .disabled(primaryButtonDisabled)
        .frame(minWidth: 72)
    }

    private var modelMenuDisabled: Bool {
        isStreaming || isUpdatingServerSettings || availableModels.isEmpty
    }

    private var thinkingMenuDisabled: Bool {
        isStreaming || isUpdatingServerSettings
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

    private var usesCompactMobileLayout: Bool {
#if os(iOS)
        horizontalSizeClass == .compact
#else
        false
#endif
    }

    private func performPrimaryAction() {
        if isStreaming && text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            stopAction()
            return
        }

        FawxHaptics.lightImpact()
        sendAction()
    }
}
