import Observation
import SwiftUI

struct ChatDetailView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    let emptyStateTitle: String
    let emptyStateMessage: String

    var body: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: FawxSpacing.paddingLG) {
                        if chatViewModel.isLoadingHistory {
                            ProgressView("Loading conversation...")
                                .foregroundStyle(Color.fawxTextSecondary)
                        } else if sessionViewModel.selectedSessionID == nil && chatViewModel.transcriptItems.isEmpty {
                            emptyState
                        } else {
                            ForEach(chatViewModel.transcriptItems) { item in
                                transcriptItemView(item)
                                    .id(item.id)
                            }

                            if chatViewModel.isStreaming || !chatViewModel.streamingText.isEmpty {
                                MessageBubble(
                                    role: .assistant,
                                    content: chatViewModel.streamingText.isEmpty ? "..." : chatViewModel.streamingText,
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
                .onChange(of: chatViewModel.transcriptItems.count) { _, _ in
                    scrollToBottom(using: proxy)
                }
                .onChange(of: chatViewModel.streamingText) { _, _ in
                    scrollToBottom(using: proxy)
                }
            }

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
                .padding(.horizontal, FawxSpacing.paddingXL)
            }

            InputBar(
                text: $chatViewModel.draftMessage,
                queuedMessage: chatViewModel.queuedMessage,
                isStreaming: chatViewModel.isStreaming,
                connectionStatus: appState.connectionStatus,
                currentPhase: chatViewModel.currentPhase,
                activeModel: appState.activeModel,
                availableModels: appState.availableModels,
                thinkingLevel: appState.thinkingLevel,
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
            .padding(.horizontal, FawxSpacing.paddingXL)
            .padding(.bottom, FawxSpacing.paddingXL)
        }
    }

    @ViewBuilder
    private func transcriptItemView(_ item: ChatTranscriptItem) -> some View {
        switch item {
        case .message(let message):
            MessageBubble(message: message)
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

    private func scrollToBottom(using proxy: ScrollViewProxy) {
        let target = chatViewModel.isStreaming || !chatViewModel.streamingText.isEmpty
            ? "streaming"
            : chatViewModel.transcriptItems.last?.id

        guard let target else {
            return
        }

        withAnimation(.easeOut(duration: 0.2)) {
            proxy.scrollTo(target, anchor: .bottom)
        }
    }
}
