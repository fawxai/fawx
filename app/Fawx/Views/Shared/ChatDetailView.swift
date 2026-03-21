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
    @State private var transcriptScrollTracker = TranscriptScrollTracker()
    @State private var isShowingRipcordReviewTray = false
    @State private var isLoadingRipcordJournal = false
    @State private var ripcordJournalEntries: [JournalEntry] = []
    @State private var ripcordJournalErrorMessage: String?
    @State private var ripcordReport: RipcordReport?
    @State private var pendingRipcordConfirmation: RipcordConfirmationAction?
    @State private var ripcordActionInFlight: RipcordAction?

    let emptyStateTitle: String
    let emptyStateMessage: String

    var body: some View {
        GeometryReader(content: detailContainer)
            .background(Color.fawxBackground)
    }

    @ViewBuilder
    private func detailContainer(_ containerProxy: GeometryProxy) -> some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            modernDetailContainer(containerProxy)
        } else {
            legacyDetailContainer(containerProxy)
        }
    }

    @available(iOS 18.0, macOS 15.0, *)
    private func modernDetailContainer(_ containerProxy: GeometryProxy) -> some View {
        applyingRipcordPresentation(
            to: decoratedTranscriptScrollView(
                ModernTranscriptScrollView(
                    sessionID: sessionViewModel.selectedSessionID,
                    lastTranscriptItemID: chatViewModel.transcriptItems.last?.id,
                    hasVisibleTranscriptContent: hasVisibleTranscriptContent,
                    isLoadingHistory: chatViewModel.isLoadingHistory,
                    isCurrentSessionStreaming: chatViewModel.isCurrentSessionStreaming,
                    visibleStreamingText: chatViewModel.visibleStreamingText,
                    pendingTranscriptScrollBehavior: pendingTranscriptScrollBehaviorBinding,
                    updateStreamingPinnedState: { isPinnedToBottom, distanceFromBottom in
                        chatViewModel.updateStreamingPinnedState(
                            isPinnedToBottom: isPinnedToBottom,
                            distanceFromBottom: distanceFromBottom
                        )
                    },
                    content: modernTranscriptStack,
                    composer: composerArea,
                    containerWidth: FawxSpacing.resolvedChatContainerWidth(for: containerProxy.size.width)
                )
            )
        )
    }

    private func legacyDetailContainer(_ containerProxy: GeometryProxy) -> some View {
        ScrollViewReader { proxy in
            applyingRipcordPresentation(
                to: applyingPlatformScrollHandlers(
                    to: observingTranscriptScrollView(
                        decoratedTranscriptScrollView(
                            baseTranscriptScrollView(for: containerProxy)
                        ),
                        proxy: proxy
                    ),
                    proxy: proxy
                )
            )
        }
    }

    private func baseTranscriptScrollView(for containerProxy: GeometryProxy) -> some View {
        ScrollView {
            legacyTranscriptStack
        }
        .id(sessionScrollIdentity)
        .background(scrollViewportReader)
        .background(Color.fawxBackground)
        .accessibilityIdentifier("messageList")
        .environment(\.containerWidth, FawxSpacing.resolvedChatContainerWidth(for: containerProxy.size.width))
        .safeAreaInset(edge: .bottom, spacing: 0) {
            composerArea
        }
    }

    @available(iOS 18.0, macOS 15.0, *)
    private var modernTranscriptStack: some View {
        VStack(spacing: FawxSpacing.paddingLG) {
            transcriptContent
            scrollBottomAnchor
        }
        .padding(FawxSpacing.paddingXL)
    }

    private var legacyTranscriptStack: some View {
        LazyVStack(spacing: FawxSpacing.paddingLG) {
            transcriptContent
            scrollBottomAnchor
        }
        .padding(FawxSpacing.paddingXL)
    }

    @ViewBuilder
    private var transcriptContent: some View {
        if sessionViewModel.selectedSessionID == nil && chatViewModel.transcriptItems.isEmpty {
            emptyState
        } else {
            ForEach(chatViewModel.transcriptItems) { item in
                transcriptItemView(item)
                    .id(item.id)
            }

            if chatViewModel.isCurrentSessionStreaming || !chatViewModel.visibleStreamingText.isEmpty {
                streamingBubble
            }
        }
    }

    private var streamingBubble: some View {
        MessageBubble(
            role: .assistant,
            content: streamingBubbleContent,
            isStreaming: true
        )
        .id("streaming")
    }

    private var scrollBottomAnchor: some View {
        Color.clear
            .frame(height: 1)
            .background(scrollContentReader)
            .id(scrollBottomAnchorID)
    }

    private var scrollViewportReader: some View {
        GeometryReader { proxy in
            Color.clear.preference(
                key: ScrollViewportBottomPreferenceKey.self,
                value: proxy.frame(in: .global).maxY
            )
        }
    }

    private var scrollContentReader: some View {
        GeometryReader { proxy in
            Color.clear.preference(
                key: ScrollContentBottomPreferenceKey.self,
                value: proxy.frame(in: .global).maxY
            )
        }
    }

    private func decoratedTranscriptScrollView<Content: View>(_ content: Content) -> some View {
        content
            .overlay {
                historyLoadingOverlay
            }
            .overlay(alignment: .top) {
                refreshIndicatorOverlay
            }
    }

    @ViewBuilder
    private var historyLoadingOverlay: some View {
        if chatViewModel.isLoadingHistory && chatViewModel.transcriptItems.isEmpty {
            loadingOverlay
        }
    }

    @ViewBuilder
    private var refreshIndicatorOverlay: some View {
        if chatViewModel.isLoadingHistory && !chatViewModel.transcriptItems.isEmpty {
            cachedRefreshIndicator
                .padding(.top, FawxSpacing.paddingLG)
        }
    }

    private func observingTranscriptScrollView<Content: View>(
        _ content: Content,
        proxy: ScrollViewProxy
    ) -> some View {
        applyingSessionScrollHandlers(
            to: applyingTranscriptUpdateHandlers(
                to: applyingPreferenceHandlers(to: content),
                proxy: proxy
            ),
            proxy: proxy
        )
    }

    private func applyingPreferenceHandlers<Content: View>(to content: Content) -> some View {
        content
            .onPreferenceChange(ScrollViewportBottomPreferenceKey.self) { viewportBottomY in
                handleScrollMetricUpdate(viewportBottomY: viewportBottomY)
            }
            .onPreferenceChange(ScrollContentBottomPreferenceKey.self) { contentBottomY in
                handleScrollMetricUpdate(contentBottomY: contentBottomY)
            }
    }

    private func applyingTranscriptUpdateHandlers<Content: View>(
        to content: Content,
        proxy: ScrollViewProxy
    ) -> some View {
        content
            .onAppear {
                scrollToBottom(using: proxy, animated: false)
            }
            .onChange(of: chatViewModel.transcriptItems.last?.id) { _, _ in
                let scrollBehavior = chatViewModel.pendingTranscriptScrollBehavior
                let animated = scrollBehavior == .animated && !chatViewModel.isLoadingHistory
                let shouldPreserveScrollPosition = scrollBehavior == .preservePosition
                let shouldSkipStreamingScroll =
                    chatViewModel.isCurrentSessionStreaming
                    && !chatViewModel.shouldAutoScrollStreamingUpdates
                if !shouldPreserveScrollPosition && !shouldSkipStreamingScroll {
                    scrollToBottom(using: proxy, animated: animated)
                }
                chatViewModel.pendingTranscriptScrollBehavior = .animated
            }
            .onChange(of: chatViewModel.visibleStreamingText) { _, _ in
                guard chatViewModel.shouldAutoScrollStreamingUpdates else {
                    return
                }

                scrollToBottom(using: proxy, animated: false)
            }
    }

    private func applyingSessionScrollHandlers<Content: View>(
        to content: Content,
        proxy: ScrollViewProxy
    ) -> some View {
        content
            .onChange(of: chatViewModel.isCurrentSessionStreaming) { _, isStreaming in
                if isStreaming {
                    scrollToBottom(using: proxy, animated: false)
                }
            }
            .onChange(of: sessionViewModel.selectedSessionID) { _, _ in
                transcriptScrollTracker.reset()
                scrollToBottom(using: proxy, animated: false)
            }
            .onChange(of: chatViewModel.isLoadingHistory) { oldValue, newValue in
                if oldValue && !newValue {
                    scrollToBottom(using: proxy, animated: false)
                }
            }
    }

    private func applyingPlatformScrollHandlers<Content: View>(
        to content: Content,
        proxy: ScrollViewProxy
    ) -> some View {
#if os(iOS)
        content
            .scrollDismissesKeyboard(.interactively)
            .onReceive(keyboardFrameDidChange) { _ in
                scrollToBottom(using: proxy)
            }
#else
        content
#endif
    }

    private func applyingRipcordPresentation<Content: View>(to content: Content) -> some View {
        content
            .sheet(
                isPresented: isShowingRipcordReportBinding,
                onDismiss: {
                    ripcordReport = nil
                }
            ) {
                if let ripcordReport {
                    RipcordReportView(report: ripcordReport, dismissAction: {
                        self.ripcordReport = nil
                    })
                    .fawxOpaqueModalPresentation()
                }
            }
            .onChange(of: appState.activeRipcordStatus?.notificationID) { oldValue, newValue in
                guard oldValue != newValue else {
                    return
                }

                isShowingRipcordReviewTray = false
                ripcordJournalEntries = []
                ripcordJournalErrorMessage = nil

                if newValue == nil {
                    ripcordJournalErrorMessage = nil
                }
            }
            .animation(.easeInOut(duration: 0.22), value: isShowingRipcordReviewTray)
            .animation(.easeInOut(duration: 0.22), value: appState.activeRipcordStatus?.notificationID)
            .confirmationDialog(
                pendingRipcordConfirmation?.title ?? "",
                isPresented: pendingRipcordConfirmationBinding,
                titleVisibility: .visible
            ) {
                ripcordConfirmationActions
            } message: {
                Text(pendingRipcordConfirmation?.message ?? "")
            }
    }

    private var isShowingRipcordReportBinding: Binding<Bool> {
        Binding(
            get: { ripcordReport != nil },
            set: { isPresented in
                if !isPresented {
                    ripcordReport = nil
                }
            }
        )
    }

    private var pendingRipcordConfirmationBinding: Binding<Bool> {
        Binding(
            get: { pendingRipcordConfirmation != nil },
            set: { isPresented in
                if !isPresented {
                    pendingRipcordConfirmation = nil
                }
            }
        )
    }

    @ViewBuilder
    private var ripcordConfirmationActions: some View {
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
    }

    private func presentRipcordJournal() {
        ripcordReport = nil
        isShowingRipcordReviewTray = true

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
                isShowingRipcordReviewTray = false
            case .approve:
                try await appState.approveRipcord()
                ripcordReport = nil
                ripcordJournalEntries = []
                isShowingRipcordReviewTray = false
            }
        } catch {
            ripcordJournalErrorMessage = error.localizedDescription
            isShowingRipcordReviewTray = true
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
            MessageBubble(
                role: message.message.role,
                content: message.displayText,
                timestamp: message.message.timestamp
            )
        case .toolActivityGroup(let group):
            ToolActivityGroupCard(group: group)
        }
    }

    private var emptyState: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Image("FawxLogo")
                .resizable()
                .aspectRatio(contentMode: .fit)
                .frame(width: emptyStateEmojiSize, height: emptyStateEmojiSize)
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

            ripcordComposerSurface

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

    @ViewBuilder
    private var ripcordComposerSurface: some View {
        if let ripcordStatus = appState.activeRipcordStatus {
            ripcordComposerContent(for: ripcordStatus)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    @ViewBuilder
    private func ripcordComposerContent(for status: RipcordStatusResponse) -> some View {
        if isShowingRipcordReviewTray {
            ripcordReviewTray(for: status)
        } else {
            ripcordNotification(for: status)
        }
    }

    private func ripcordNotification(for status: RipcordStatusResponse) -> some View {
        RipcordNotification(
            snapshot: RipcordNotificationSnapshot(
                status: status,
                isPerformingAction: ripcordActionInFlight != nil,
                resolutionActionKind: ripcordResolutionActionKind
            ),
            actions: ripcordNotificationActions
        )
    }

    private func ripcordReviewTray(for status: RipcordStatusResponse) -> some View {
        RipcordReviewTray(
            snapshot: RipcordReviewTraySnapshot(
                status: status,
                entries: ripcordJournalEntries,
                isLoading: isLoadingRipcordJournal,
                errorMessage: ripcordJournalErrorMessage,
                isPerformingAction: ripcordActionInFlight != nil,
                resolutionActionKind: ripcordResolutionActionKind
            ),
            actions: ripcordReviewTrayActions
        )
    }

    private var ripcordNotificationActions: RipcordNotificationActions {
        RipcordNotificationActions(
            review: presentRipcordJournal,
            pull: {
                pendingRipcordConfirmation = .pull
            },
            resolution: performRipcordResolutionAction
        )
    }

    private var ripcordReviewTrayActions: RipcordReviewTrayActions {
        RipcordReviewTrayActions(
            refresh: {
                Task {
                    await loadRipcordJournal()
                }
            },
            pull: {
                pendingRipcordConfirmation = .pull
            },
            resolution: performRipcordResolutionAction,
            close: {
                isShowingRipcordReviewTray = false
            }
        )
    }

    private var ripcordResolutionActionKind: RipcordResolutionActionKind {
        RipcordResolutionActionKind.forPermissionMode(appState.permissionMode)
    }

    private func performRipcordResolutionAction() {
        switch ripcordResolutionActionKind {
        case .dismiss:
            isShowingRipcordReviewTray = false
            appState.dismissRipcordNotification()
        case .approve:
            pendingRipcordConfirmation = .approve
        }
    }

    private func scrollToBottom(using proxy: ScrollViewProxy, animated: Bool = true) {
        let hasVisibleTranscriptContent =
            !chatViewModel.transcriptItems.isEmpty
            || chatViewModel.isCurrentSessionStreaming
            || !chatViewModel.visibleStreamingText.isEmpty

        guard hasVisibleTranscriptContent else {
            return
        }

        if animated {
            withAnimation(.easeOut(duration: 0.15)) {
                proxy.scrollTo(scrollBottomAnchorID, anchor: .bottom)
            }
        } else {
            proxy.scrollTo(scrollBottomAnchorID, anchor: .bottom)
        }
    }

    private func handleScrollMetricUpdate(
        viewportBottomY: CGFloat? = nil,
        contentBottomY: CGFloat? = nil
    ) {
        guard let distanceFromBottom = transcriptScrollTracker.update(
            viewportBottomY: viewportBottomY,
            contentBottomY: contentBottomY
        ) else {
            return
        }
        guard chatViewModel.isCurrentSessionStreaming || !chatViewModel.visibleStreamingText.isEmpty else {
            return
        }

        chatViewModel.updateStreamingDistanceFromBottom(distanceFromBottom)
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

    private var hasVisibleTranscriptContent: Bool {
        !chatViewModel.transcriptItems.isEmpty
            || chatViewModel.isCurrentSessionStreaming
            || !chatViewModel.visibleStreamingText.isEmpty
    }

    private var pendingTranscriptScrollBehaviorBinding: Binding<ChatViewModel.TranscriptScrollBehavior> {
        Binding(
            get: { chatViewModel.pendingTranscriptScrollBehavior },
            set: { chatViewModel.pendingTranscriptScrollBehavior = $0 }
        )
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

@available(iOS 18.0, macOS 15.0, *)
private struct ModernTranscriptScrollView<Content: View, Composer: View>: View {
    let sessionID: String?
    let lastTranscriptItemID: String?
    let hasVisibleTranscriptContent: Bool
    let isLoadingHistory: Bool
    let isCurrentSessionStreaming: Bool
    let visibleStreamingText: String
    @Binding var pendingTranscriptScrollBehavior: ChatViewModel.TranscriptScrollBehavior
    let updateStreamingPinnedState: (_ isPinnedToBottom: Bool, _ distanceFromBottom: CGFloat) -> Void
    let content: Content
    let composer: Composer
    let containerWidth: CGFloat

    @State private var scrollPosition = ScrollPosition(idType: String.self)
    @State private var scrollCoordinator = TranscriptScrollCoordinator()
    @State private var scrollInteractionTracker = TranscriptScrollInteractionTracker()

    var body: some View {
        ScrollView {
            content
                .scrollTargetLayout()
        }
        .scrollPosition($scrollPosition)
        .background(Color.fawxBackground)
        .accessibilityIdentifier("messageList")
        .environment(\.containerWidth, containerWidth)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            composer
        }
        .onAppear {
            scrollCoordinator.activateSession(sessionID)
            restoreScrollPositionIfNeeded()
        }
        .onChange(of: sessionID) { _, newValue in
            scrollCoordinator.activateSession(newValue)
            restoreScrollPositionIfNeeded()
        }
        .onChange(of: hasVisibleTranscriptContent) { _, _ in
            restoreScrollPositionIfNeeded()
        }
        .onChange(of: isLoadingHistory) { _, _ in
            restoreScrollPositionIfNeeded()
        }
        .onChange(of: isCurrentSessionStreaming) { _, isStreaming in
            if isStreaming && scrollCoordinator.shouldFollowLiveOutput {
                scrollToBottom(animated: false)
            }
            if isStreaming {
                let pinnedStateUpdate = scrollCoordinator.seedPinnedState(distanceFromBottom: 0)
                updateStreamingPinnedState(
                    pinnedStateUpdate.isPinnedToBottom,
                    pinnedStateUpdate.distanceFromBottom
                )
            }
        }
        .onChange(of: lastTranscriptItemID) { _, _ in
            handleTranscriptItemChange()
        }
        .onChange(of: visibleStreamingText) { _, _ in
            guard scrollCoordinator.shouldFollowLiveOutput else {
                return
            }

            scrollToBottom(animated: false)
        }
        .onScrollPhaseChange { _, newPhase in
            scrollInteractionTracker.updateScrollPhase(newPhase)
        }
        .onScrollGeometryChange(
            for: TranscriptScrollObservation.self,
            of: { geometry in
                TranscriptScrollObservation(
                    contentOffsetY: max(0, geometry.contentOffset.y),
                    distanceFromBottom: max(0, geometry.contentSize.height - geometry.visibleRect.maxY)
                )
            },
            action: { _, observation in
                handleScrollObservation(observation)
            }
        )
#if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        .onReceive(keyboardFrameDidChange) { _ in
            if scrollCoordinator.shouldFollowLiveOutput {
                scrollToBottom(animated: false)
            }
        }
#endif
    }

    private var isUserDrivenScroll: Bool {
        scrollInteractionTracker.isUserDrivenScroll(isPositionedByUser: scrollPosition.isPositionedByUser)
    }

    private func restoreScrollPositionIfNeeded() {
        guard let intent = scrollCoordinator.restoreIntentIfNeeded(
            hasVisibleTranscriptContent: hasVisibleTranscriptContent,
            isLoadingHistory: isLoadingHistory
        ) else {
            return
        }

        applyScrollIntent(intent, animated: false)
    }

    private func handleTranscriptItemChange() {
        let scrollBehavior = pendingTranscriptScrollBehavior
        let shouldPreserveScrollPosition =
            scrollBehavior == .preservePosition || !scrollCoordinator.shouldFollowLiveOutput
        let animated = scrollBehavior == .animated && !isLoadingHistory

        if !shouldPreserveScrollPosition {
            scrollToBottom(animated: animated)
        }

        pendingTranscriptScrollBehavior = .animated
    }

    private func handleScrollObservation(_ observation: TranscriptScrollObservation) {
        guard let update = scrollCoordinator.update(
            observation: observation,
            userDriven: isUserDrivenScroll
        ) else {
            return
        }

        updateStreamingPinnedState(update.isPinnedToBottom, update.distanceFromBottom)
    }

    private func scrollToBottom(animated: Bool) {
        applyScrollIntent(.bottom, animated: animated)
    }

    private func applyScrollIntent(_ intent: TranscriptScrollRestoreIntent, animated: Bool) {
        let performScroll = {
            switch intent {
            case .bottom:
                scrollPosition.scrollTo(edge: .bottom)
            case .point(let y):
                scrollPosition.scrollTo(y: y)
            }
        }

        if animated {
            withAnimation(.easeOut(duration: 0.15)) {
                performScroll()
            }
        } else {
            performScroll()
        }
    }

#if os(iOS)
    private var keyboardFrameDidChange: NotificationCenter.Publisher {
        NotificationCenter.default.publisher(for: UIResponder.keyboardWillChangeFrameNotification)
    }
#endif
}

struct TranscriptScrollObservation: Equatable {
    var contentOffsetY: CGFloat
    var distanceFromBottom: CGFloat
}

enum TranscriptScrollRestoreIntent: Equatable {
    case bottom
    case point(CGFloat)
}

struct TranscriptPinnedStateUpdate: Equatable {
    var distanceFromBottom: CGFloat
    var isPinnedToBottom: Bool
}

@MainActor
final class TranscriptScrollCoordinator {
    enum Mode: Equatable {
        case followingLive
        case detached
        case restoringSession
    }

    private struct SessionSnapshot: Equatable {
        var contentOffsetY: CGFloat
        var followsLive: Bool
    }

    private static let repinThreshold = StreamingDisplayController.bottomThreshold
    private static let detachThreshold = StreamingDisplayController.bottomThreshold * 1.5
    private static let maxTrackedSnapshots = 32

    private(set) var mode: Mode = .followingLive
    private var activeSessionID: String?
    private var pendingRestoreSessionID: String?
    private var snapshotsBySession: [String: SessionSnapshot] = [:]
    private var snapshotAccessOrder: [String] = []
    private var lastPublishedPinnedState: Bool?

    var shouldFollowLiveOutput: Bool {
        mode != .detached
    }

    func activateSession(_ sessionID: String?) {
        activeSessionID = sessionID
        pendingRestoreSessionID = sessionID
        mode = .restoringSession
        lastPublishedPinnedState = nil
    }

    func seedPinnedState(distanceFromBottom: CGFloat) -> TranscriptPinnedStateUpdate {
        let isPinnedToBottom = mode != .detached
        lastPublishedPinnedState = isPinnedToBottom
        return makePinnedStateUpdate(
            distanceFromBottom: distanceFromBottom,
            isPinnedToBottom: isPinnedToBottom
        )
    }

    func restoreIntentIfNeeded(
        hasVisibleTranscriptContent: Bool,
        isLoadingHistory: Bool
    ) -> TranscriptScrollRestoreIntent? {
        guard
            let activeSessionID,
            pendingRestoreSessionID == activeSessionID,
            hasVisibleTranscriptContent,
            !isLoadingHistory
        else {
            return nil
        }

        pendingRestoreSessionID = nil

        if let snapshot = snapshot(for: activeSessionID), !snapshot.followsLive {
            mode = .detached
            return .point(snapshot.contentOffsetY)
        }

        mode = .followingLive
        return .bottom
    }

    func update(
        observation: TranscriptScrollObservation,
        userDriven: Bool
    ) -> TranscriptPinnedStateUpdate? {
        guard let activeSessionID else {
            return nil
        }

        let distanceFromBottom = max(0, observation.distanceFromBottom)

        if distanceFromBottom <= Self.repinThreshold {
            mode = .followingLive
        } else if userDriven && distanceFromBottom >= Self.detachThreshold {
            mode = .detached
        }

        storeSnapshot(
            SessionSnapshot(
                contentOffsetY: max(0, observation.contentOffsetY),
                followsLive: mode != .detached
            ),
            for: activeSessionID
        )

        return pinnedStateUpdateIfNeeded(distanceFromBottom: distanceFromBottom)
    }

    private func snapshot(for sessionID: String) -> SessionSnapshot? {
        guard let snapshot = snapshotsBySession[sessionID] else {
            return nil
        }

        touchSnapshot(sessionID)
        return snapshot
    }

    private func storeSnapshot(_ snapshot: SessionSnapshot, for sessionID: String) {
        snapshotsBySession[sessionID] = snapshot
        touchSnapshot(sessionID)
        evictSnapshotsIfNeeded()
    }

    private func touchSnapshot(_ sessionID: String) {
        snapshotAccessOrder.removeAll(where: { $0 == sessionID })
        snapshotAccessOrder.append(sessionID)
    }

    private func evictSnapshotsIfNeeded() {
        while snapshotsBySession.count > Self.maxTrackedSnapshots,
              let leastRecentlyUsedSessionID = snapshotAccessOrder.first
        {
            snapshotAccessOrder.removeFirst()
            snapshotsBySession.removeValue(forKey: leastRecentlyUsedSessionID)
        }
    }

    private func pinnedStateUpdateIfNeeded(distanceFromBottom: CGFloat) -> TranscriptPinnedStateUpdate? {
        let isPinnedToBottom = mode != .detached
        guard lastPublishedPinnedState != isPinnedToBottom else {
            return nil
        }

        lastPublishedPinnedState = isPinnedToBottom
        return makePinnedStateUpdate(
            distanceFromBottom: distanceFromBottom,
            isPinnedToBottom: isPinnedToBottom
        )
    }

    private func makePinnedStateUpdate(
        distanceFromBottom: CGFloat,
        isPinnedToBottom: Bool
    ) -> TranscriptPinnedStateUpdate {
        TranscriptPinnedStateUpdate(
            distanceFromBottom: max(0, distanceFromBottom),
            isPinnedToBottom: isPinnedToBottom
        )
    }
}

@MainActor
final class TranscriptScrollInteractionTracker {
    private var isUserInteracting = false

    @available(iOS 18.0, macOS 15.0, *)
    func updateScrollPhase(_ scrollPhase: ScrollPhase) {
        switch scrollPhase {
        case .tracking, .interacting, .decelerating:
            isUserInteracting = true
        case .idle, .animating:
            isUserInteracting = false
        @unknown default:
            isUserInteracting = false
        }
    }

    func isUserDrivenScroll(isPositionedByUser: Bool) -> Bool {
        isUserInteracting || isPositionedByUser
    }
}

@MainActor
final class TranscriptScrollTracker {
    private var viewportBottomY: CGFloat = 0
    private var contentBottomY: CGFloat = 0
    private var lastDistanceFromBottom: CGFloat?

    func update(
        viewportBottomY: CGFloat? = nil,
        contentBottomY: CGFloat? = nil
    ) -> CGFloat? {
        var didUpdateMetrics = false

        if let viewportBottomY, viewportBottomY.isFinite, viewportBottomY >= 0,
           viewportBottomY != self.viewportBottomY
        {
            self.viewportBottomY = viewportBottomY
            didUpdateMetrics = true
        }

        if let contentBottomY, contentBottomY.isFinite, contentBottomY >= 0,
           contentBottomY != self.contentBottomY
        {
            self.contentBottomY = contentBottomY
            didUpdateMetrics = true
        }

        guard didUpdateMetrics else {
            return nil
        }

        guard self.viewportBottomY > 0, self.contentBottomY > 0 else {
            return nil
        }

        let distanceFromBottom = max(0, self.contentBottomY - self.viewportBottomY)
        guard distanceFromBottom != lastDistanceFromBottom else {
            return nil
        }

        lastDistanceFromBottom = distanceFromBottom
        return distanceFromBottom
    }

    func reset() {
        viewportBottomY = 0
        contentBottomY = 0
        lastDistanceFromBottom = nil
    }
}

private struct ScrollViewportBottomPreferenceKey: PreferenceKey {
    static let defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

private struct ScrollContentBottomPreferenceKey: PreferenceKey {
    static let defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
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
