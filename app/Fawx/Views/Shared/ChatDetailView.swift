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
    @ScaledMetric(relativeTo: .title2) private var emptyStateEmojiSize = 30
    @State private var isShowingRipcordSheet = false
    @State private var isLoadingRipcordJournal = false
    @State private var ripcordJournalEntries: [JournalEntry] = []
    @State private var ripcordJournalErrorMessage: String?
    @State private var ripcordReport: RipcordReport?
    @State private var pendingRipcordConfirmation: RipcordConfirmationAction?
    @State private var ripcordActionInFlight: RipcordAction?

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

                    Color.clear
                        .frame(height: 1)
                        .id(scrollBottomAnchorID)
                }
                .padding(FawxSpacing.paddingXL)
            }
            .id(sessionScrollIdentity)
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
            .safeAreaInset(edge: .top, spacing: 0) {
                if let ripcordStatus = appState.activeRipcordStatus {
                    RipcordBanner(
                        status: ripcordStatus,
                        isPerformingAction: ripcordActionInFlight != nil,
                        reviewAction: presentRipcordJournal,
                        pullAction: {
                            pendingRipcordConfirmation = .pull
                        },
                        approveAction: {
                            pendingRipcordConfirmation = .approve
                        }
                    )
                    .padding(.horizontal, FawxSpacing.paddingXL)
                    .padding(.top, FawxSpacing.paddingSM)
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
            .onChange(of: chatViewModel.isCurrentSessionStreaming) { _, isStreaming in
                if isStreaming {
                    scheduleScrollToBottom(using: proxy, animated: false)
                }
            }
            .onChange(of: sessionViewModel.selectedSessionID) { _, _ in
                scheduleScrollToBottom(using: proxy, animated: false)
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
            .sheet(
                isPresented: $isShowingRipcordSheet,
                onDismiss: {
                    ripcordReport = nil
                }
            ) {
                Group {
                    if let ripcordReport {
                        RipcordReportView(report: ripcordReport, dismissAction: {
                            self.ripcordReport = nil
                            isShowingRipcordSheet = false
                        })
                    } else {
                        RipcordJournalPanel(
                            status: appState.activeRipcordStatus,
                            entries: ripcordJournalEntries,
                            isLoading: isLoadingRipcordJournal,
                            errorMessage: ripcordJournalErrorMessage,
                            isPerformingAction: ripcordActionInFlight != nil,
                            refreshAction: {
                                Task {
                                    await loadRipcordJournal()
                                }
                            },
                            pullAction: {
                                pendingRipcordConfirmation = .pull
                            },
                            approveAction: {
                                pendingRipcordConfirmation = .approve
                            },
                            dismissAction: {
                                isShowingRipcordSheet = false
                            }
                        )
                    }
                }
                .fawxOpaqueModalPresentation()
            }
            .confirmationDialog(
                pendingRipcordConfirmation?.title ?? "",
                isPresented: Binding(
                    get: { pendingRipcordConfirmation != nil },
                    set: { isPresented in
                        if !isPresented {
                            pendingRipcordConfirmation = nil
                        }
                    }
                ),
                titleVisibility: .visible
            ) {
                Button(
                    pendingRipcordConfirmation?.buttonTitle ?? "",
                    role: pendingRipcordConfirmation?.buttonRole
                ) {
                    guard let pendingRipcordConfirmation else {
                        return
                    }

                    Task {
                        await performRipcordAction(pendingRipcordConfirmation)
                    }
                }

                Button("Cancel", role: .cancel) {
                    pendingRipcordConfirmation = nil
                }
            } message: {
                Text(pendingRipcordConfirmation?.message ?? "")
            }
        }
        .background(Color.fawxBackground)
    }

    private func presentRipcordJournal() {
        ripcordReport = nil
        isShowingRipcordSheet = true

        Task {
            await loadRipcordJournal()
        }
    }

    @MainActor
    private func loadRipcordJournal() async {
        guard !isLoadingRipcordJournal else {
            return
        }

        isLoadingRipcordJournal = true
        ripcordJournalErrorMessage = nil
        defer { isLoadingRipcordJournal = false }

        do {
            ripcordJournalEntries = try await appState.loadRipcordJournal()
        } catch {
            ripcordJournalErrorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    @MainActor
    private func performRipcordAction(_ action: RipcordConfirmationAction) async {
        pendingRipcordConfirmation = nil
        ripcordActionInFlight = action.ripcordAction
        ripcordJournalErrorMessage = nil
        defer { ripcordActionInFlight = nil }

        do {
            switch action {
            case .pull:
                ripcordReport = try await appState.pullRipcord()
                ripcordJournalEntries = []
                isShowingRipcordSheet = true
            case .approve:
                try await appState.approveRipcord()
                ripcordReport = nil
                ripcordJournalEntries = []
                isShowingRipcordSheet = false
            }
        } catch {
            ripcordJournalErrorMessage = error.localizedDescription
            isShowingRipcordSheet = true
            await appState.noteRecoverableRequestFailure(error)
        }
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
        .background(Color.fawxSurface.opacity(FawxOpacity.surfaceOverlay))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderMedium), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .fawxShadow(FawxShadow.loadingOverlay)
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
        .background(Color.fawxSurface.opacity(FawxOpacity.surfaceMuted))
        .overlay(
            Capsule()
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderSubtle), lineWidth: 1)
        )
        .clipShape(Capsule())
        .fawxShadow(FawxShadow.elevatedCapsule)
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
            Text("🦊")
                .font(.system(size: emptyStateEmojiSize))
                .padding(FawxSpacing.paddingMD)
                .background(Color.fawxAccentSubtle)
                .clipShape(Circle())
                .accessibilityHidden(true)

            Text(emptyStateTitle)
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text(emptyStateMessage)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: 440)
        .padding(FawxSpacing.paddingXL)
        .background(Color.fawxSurface.opacity(FawxOpacity.surfaceStrong))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius + 4)
                .stroke(Color.fawxBorder.opacity(FawxOpacity.borderStrong), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius + 4))
        .fawxShadow(FawxShadow.floatingPanel)
        .frame(maxWidth: .infinity, minHeight: 320)
    }

    private var composerArea: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            if appState.permissionMode.showsPermissionPrompts,
               let indicatorText = chatViewModel.permissionPromptIndicatorText
            {
                PermissionPromptInlineNotice(
                    text: indicatorText,
                    tierLabel: chatViewModel.activePermissionPrompt?.tierLabel
                )
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
                .background(Color.fawxError.opacity(FawxOpacity.fillMuted))
                .overlay(
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                        .stroke(Color.fawxError.opacity(FawxOpacity.borderHighlight), lineWidth: 1)
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
            LinearGradient(
                colors: [
                    Color.fawxBackground.opacity(0),
                    Color.fawxBackground.opacity(FawxOpacity.backgroundScrim),
                    Color.fawxBackground
                ],
                startPoint: .top,
                endPoint: .bottom
            )
                .overlay(alignment: .top) {
                    Divider()
                        .opacity(FawxOpacity.iconSecondary)
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
        let hasVisibleTranscriptContent =
            !chatViewModel.transcriptItems.isEmpty
            || chatViewModel.isCurrentSessionStreaming
            || !chatViewModel.visibleStreamingText.isEmpty

        guard hasVisibleTranscriptContent else {
            return
        }

        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                proxy.scrollTo(scrollBottomAnchorID, anchor: .bottom)
            }
        } else {
            proxy.scrollTo(scrollBottomAnchorID, anchor: .bottom)
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

    private var sessionScrollIdentity: String {
        sessionViewModel.selectedSessionID ?? "new-session"
    }

    private var scrollBottomAnchorID: String {
        "chat-scroll-bottom"
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

private enum RipcordAction {
    case pull
    case approve
}

private enum RipcordConfirmationAction: String, Identifiable {
    case pull
    case approve

    var id: String { rawValue }

    var title: String {
        switch self {
        case .pull:
            return "Pull ripcord?"
        case .approve:
            return "Approve changes?"
        }
    }

    var message: String {
        switch self {
        case .pull:
            return "Fawx will try to undo every reversible journaled action and keep an audit record of anything it cannot revert."
        case .approve:
            return "This clears the ripcord journal and keeps the changes that have already been made."
        }
    }

    var buttonTitle: String {
        switch self {
        case .pull:
            return "Pull Ripcord"
        case .approve:
            return "Approve Changes"
        }
    }

    var buttonRole: ButtonRole? {
        switch self {
        case .pull:
            return .destructive
        case .approve:
            return nil
        }
    }

    var ripcordAction: RipcordAction {
        switch self {
        case .pull:
            return .pull
        case .approve:
            return .approve
        }
    }
}

struct PermissionPromptSheetView: View {
    let prompt: PermissionPrompt
    let isSubmitting: Bool
    let errorMessage: String?
    let allowAction: () -> Void
    let denyAction: () -> Void
    let allowSessionAction: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Image(systemName: "hand.raised.fill")
                    .foregroundStyle(promptAccentColor)

                Text("Permission Required")
                    .font(FawxTypography.heading2)
                    .foregroundStyle(Color.fawxText)

                Spacer()

                if let tierLabel = prompt.tierLabel {
                    Text(tierLabel)
                        .font(FawxTypography.status)
                        .foregroundStyle(promptAccentColor)
                        .padding(.horizontal, FawxSpacing.paddingSM)
                        .padding(.vertical, FawxSpacing.paddingXS)
                        .background(promptAccentColor.opacity(FawxOpacity.fillSubtle))
                        .clipShape(Capsule())
                }
            }

            Text(prompt.summaryText)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                permissionDetailRow(label: "Action", value: prompt.displayAction, monospaced: false)

                if !prompt.displayPath.isEmpty {
                    permissionDetailRow(label: "Path", value: prompt.displayPath, monospaced: true)
                }
            }
            .padding(FawxSpacing.paddingMD)
            .background(Color.fawxSurface)
            .overlay(
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .stroke(Color.fawxBorder, lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))

            if let errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxError)
                    .padding(FawxSpacing.paddingMD)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.fawxError.opacity(FawxOpacity.fillMuted))
                    .overlay(
                        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                            .stroke(Color.fawxError.opacity(FawxOpacity.errorBorder), lineWidth: 1)
                    )
                    .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
            }

            Text("This request auto-denies after \(ChatViewModel.permissionPromptTimeoutSeconds) seconds.")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            HStack(spacing: FawxSpacing.paddingMD) {
                if prompt.sessionScopedAllowAvailable {
                    Button(prompt.allowSessionActionTitle, action: allowSessionAction)
                        .buttonStyle(.borderedProminent)
                        .tint(Color.fawxAccent)
                        .disabled(isSubmitting)
                }

                Button(prompt.allowActionTitle, action: allowAction)
                    .buttonStyle(.bordered)
                    .disabled(isSubmitting)

                Button(prompt.denyActionTitle, role: .destructive, action: denyAction)
                    .buttonStyle(.bordered)
                    .disabled(isSubmitting)
            }

            if isSubmitting {
                HStack(spacing: FawxSpacing.paddingSM) {
                    ProgressView()
                        .controlSize(.small)

                    Text("Sending response...")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }
        }
        .padding(FawxSpacing.paddingXL)
        .frame(minWidth: 360, idealWidth: 440, maxWidth: 520, alignment: .leading)
        .background(Color.fawxBackground)
    }

    @ViewBuilder
    private func permissionDetailRow(label: String, value: String, monospaced: Bool) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text(label)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            if monospaced {
                Text(verbatim: value)
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(Color.fawxText)
                    .textSelection(.enabled)
            } else {
                Text(value)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var promptAccentColor: Color {
        switch prompt.tier ?? 1 {
        case 3...:
            return .fawxError
        case 2:
            return .fawxWarning
        default:
            return .fawxAccent
        }
    }
}

private struct PermissionPromptInlineNotice: View {
    let text: String
    let tierLabel: String?

    var body: some View {
        HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
            Image(systemName: "hand.raised")
                .foregroundStyle(Color.fawxWarning)

            Text(text)
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)

            if let tierLabel {
                Text(tierLabel)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxWarning)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.vertical, FawxSpacing.paddingXS)
                    .background(Color.fawxWarning.opacity(FawxOpacity.fillSubtle))
                    .clipShape(Capsule())
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxWarning.opacity(FawxOpacity.fillMuted))
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxWarning.opacity(FawxOpacity.accentBorder), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
    }
}

private extension PermissionPrompt {
    var allowActionTitle: String {
        PermissionPromptDecision.allow.buttonTitle
    }

    var denyActionTitle: String {
        PermissionPromptDecision.deny.buttonTitle
    }

    var allowSessionActionTitle: String {
        PermissionPromptDecision.allowSession.buttonTitle
    }
}
