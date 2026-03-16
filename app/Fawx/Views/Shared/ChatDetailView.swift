import Observation
import SwiftUI
#if os(iOS)
import Combine
import UIKit
#endif

struct ChatDetailView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    let emptyStateTitle: String
    let emptyStateMessage: String

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: FawxSpacing.paddingLG) {
                    if sessionViewModel.selectedSessionID == nil && chatViewModel.transcriptItems.isEmpty {
                        emptyState
                    } else {
                        ForEach(chatViewModel.transcriptItems) { item in
                            transcriptItemView(item)
                                .id(item.id)
                        }

                        if chatViewModel.isCurrentSessionStreaming || !chatViewModel.visibleStreamingText.isEmpty {
                            MessageBubble(
                                role: .assistant,
                                content: streamingBubbleContent,
                                isStreaming: true
                            )
                            .id("streaming")
                        }
                    }
                }
                .padding(FawxSpacing.paddingXL)
            }
            .background(Color.fawxBackground)
            .accessibilityIdentifier("messageList")
            .overlay {
                if chatViewModel.isLoadingHistory && chatViewModel.transcriptItems.isEmpty {
                    loadingOverlay
                }
            }
            .overlay(alignment: .top) {
                if chatViewModel.isLoadingHistory && !chatViewModel.transcriptItems.isEmpty {
                    cachedRefreshIndicator
                        .padding(.top, FawxSpacing.paddingLG)
                }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                composerArea
            }
            .onAppear {
                scheduleScrollToBottom(using: proxy, animated: false)
            }
            .onChange(of: chatViewModel.transcriptItems.last?.id) { _, _ in
                let animated = chatViewModel.pendingTranscriptScrollBehavior == .animated && !chatViewModel.isLoadingHistory
                scheduleScrollToBottom(using: proxy, animated: animated, includeFollowUp: animated)
                chatViewModel.pendingTranscriptScrollBehavior = .animated
            }
            .onChange(of: chatViewModel.visibleStreamingText) { _, _ in
                scheduleScrollToBottom(using: proxy, animated: false, includeFollowUp: false)
            }
            .onChange(of: chatViewModel.isLoadingHistory) { oldValue, newValue in
                if oldValue && !newValue {
                    scheduleScrollToBottom(using: proxy, animated: false, includeFollowUp: false)
                }
            }
#if os(iOS)
            .scrollDismissesKeyboard(.interactively)
            .onReceive(keyboardFrameDidChange) { _ in
                scheduleScrollToBottom(using: proxy)
            }
#endif
        }
        .background(Color.fawxBackground)
    }

    private var loadingOverlay: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            ProgressView()
                .controlSize(.regular)

            Text("Loading conversation...")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(FawxSpacing.paddingXL)
        .background(Color.fawxSurface.opacity(0.96))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(0.8), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .shadow(color: .black.opacity(0.14), radius: 12, y: 4)
    }

    private var cachedRefreshIndicator: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            ProgressView()
                .controlSize(.small)

            Text("Refreshing conversation...")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface.opacity(0.94))
        .overlay(
            Capsule()
                .stroke(Color.fawxBorder.opacity(0.7), lineWidth: 1)
        )
        .clipShape(Capsule())
        .shadow(color: .black.opacity(0.08), radius: 6, y: 2)
    }

    @ViewBuilder
    private func transcriptItemView(_ item: ChatTranscriptItem) -> some View {
        switch item {
        case .message(let message):
            MessageBubble(message: message.message)
        case .toolCall(let toolCall):
            ToolCallCard(toolCall: toolCall)
        }
    }

    private var emptyState: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Text(emptyStateTitle)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text(emptyStateMessage)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, minHeight: 320)
    }

    private var composerArea: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            if let errorMessage = chatViewModel.errorMessage {
                HStack(alignment: .center, spacing: FawxSpacing.paddingMD) {
                    Text(errorMessage)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxError)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    if chatViewModel.canRetryLastMessage {
                        Button("Retry") {
                            chatViewModel.retryLastMessage()
                        }
                        .buttonStyle(.bordered)
                    }
                }
                .padding(FawxSpacing.paddingMD)
                .background(Color.fawxError.opacity(0.08))
                .overlay(
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                        .stroke(Color.fawxError.opacity(0.3), lineWidth: 1)
                )
                .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
            }

            InputBar(
                text: $chatViewModel.draftMessage,
                queuedMessage: chatViewModel.queuedMessage,
                isStreaming: chatViewModel.isCurrentSessionStreaming,
                connectionStatus: appState.connectionStatus,
                currentPhase: chatViewModel.composerPhaseLabel,
                activeModel: appState.activeModel,
                availableModels: appState.availableModels,
                thinkingLevel: appState.thinkingLevel,
                availableThinkingLevels: appState.availableThinkingLevels,
                isUpdatingServerSettings: appState.isUpdatingServerSettings,
                placeholder: sessionViewModel.selectedSessionID == nil ? "What are you working on?" : "Message Fawx...",
                sendAction: chatViewModel.sendDraft,
                stopAction: chatViewModel.stopStreaming,
                dismissQueuedMessage: chatViewModel.dismissQueuedMessage,
                selectModel: { modelID in
                    Task {
                        try? await appState.setModel(modelID)
                    }
                },
                selectThinking: { level in
                    Task {
                        try? await appState.setThinking(level)
                    }
                }
            )

#if os(iOS)
            compactStatusRow
#endif
        }
        .padding(.horizontal, FawxSpacing.paddingXL)
        .padding(.top, FawxSpacing.paddingSM)
        .padding(.bottom, composerBottomPadding)
        .background(alignment: .top) {
            Rectangle()
                .fill(Color.fawxBackground.opacity(0.96))
                .overlay(alignment: .top) {
                    Divider()
                        .opacity(0.35)
                }
                .ignoresSafeArea(edges: .bottom)
        }
    }

    private func scheduleScrollToBottom(
        using proxy: ScrollViewProxy,
        animated: Bool = true,
        includeFollowUp: Bool = true
    ) {
        scrollToBottom(using: proxy, animated: animated)

        guard includeFollowUp else {
            return
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) {
            scrollToBottom(using: proxy, animated: animated)
        }
    }

    private func scrollToBottom(using proxy: ScrollViewProxy, animated: Bool) {
        let target = chatViewModel.isCurrentSessionStreaming || !chatViewModel.visibleStreamingText.isEmpty
            ? "streaming"
            : chatViewModel.transcriptItems.last?.id

        guard let target else {
            return
        }

        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                proxy.scrollTo(target, anchor: .bottom)
            }
        } else {
            proxy.scrollTo(target, anchor: .bottom)
        }
    }

    private var composerBottomPadding: CGFloat {
#if os(iOS)
        return FawxSpacing.paddingSM
#else
        return FawxSpacing.paddingXL
#endif
    }

    private var streamingBubbleContent: String {
        let streamed = chatViewModel.visibleStreamingText.trimmingCharacters(in: .whitespacesAndNewlines)
        if !streamed.isEmpty {
            return chatViewModel.visibleStreamingText
        }

        return chatViewModel.visibleCurrentPhase?.streamingPlaceholder ?? "..."
    }

#if os(iOS)
    private var compactStatusRow: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            HStack(spacing: 6) {
                Circle()
                    .fill(connectionColor)
                    .frame(width: 6, height: 6)

                Text(connectionLabel)
                    .accessibilityIdentifier("connectionIndicator")
            }

            compactSeparator

            Text(appState.permissionPresetName)
                .lineLimit(1)

            compactSeparator

            Text(compactContextLabel)
                .lineLimit(1)
                .accessibilityIdentifier("contextLabel")

            Spacer(minLength: 0)
        }
        .font(FawxTypography.status)
        .foregroundStyle(Color.fawxTextSecondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, FawxSpacing.paddingXS)
    }

    private var compactSeparator: some View {
        Text("·")
            .foregroundStyle(Color.fawxTextSecondary)
    }

    private var compactContextLabel: String {
        guard let context = appState.currentContext else {
            return "—"
        }

        return "\(Int(context.normalizedPercentage.rounded()))% ctx"
    }

    private var connectionLabel: String {
        switch appState.connectionStatus {
        case .connected:
            return "Connected"
        case .connecting:
            return "Connecting"
        case .reconnecting:
            return "Reconnecting"
        case .disconnected:
            return "Disconnected"
        }
    }

    private var connectionColor: Color {
        switch appState.connectionStatus {
        case .connected:
            return .fawxSuccess
        case .connecting, .reconnecting:
            return .fawxWarning
        case .disconnected:
            return .fawxError
        }
    }

    private var keyboardFrameDidChange: NotificationCenter.Publisher {
        NotificationCenter.default.publisher(for: UIResponder.keyboardWillChangeFrameNotification)
    }
#endif
}
