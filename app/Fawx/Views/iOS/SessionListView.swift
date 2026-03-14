import Observation
import SwiftUI
#if os(iOS)
import UIKit
#endif

private enum SessionRoute: Hashable {
    case newSession
    case session(String)
}

struct SessionListView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    @State private var navigationPath: [SessionRoute] = []
    @State private var pendingClearSession: Session?

    var body: some View {
        Group {
            if usesSplitLayout {
                splitLayout
            } else {
                stackLayout
            }
        }
        .alert("Clear this session?", isPresented: pendingClearAlertBinding) {
            Button("Cancel", role: .cancel) {
                pendingClearSession = nil
            }

            Button("Clear", role: .destructive) {
                if let session = pendingClearSession {
                    clearSession(session.id)
                }
                pendingClearSession = nil
            }
        } message: {
            Text("This removes the conversation history but keeps the session.")
        }
    }

    private var stackLayout: some View {
        NavigationStack(path: $navigationPath) {
            sessionList
                .navigationTitle("Sessions")
                .toolbar {
                    newSessionToolbarButton
                }
                .navigationDestination(for: SessionRoute.self) { route in
                    chatDetailView(for: route)
                }
                .onChange(of: navigationPath.last) { _, newValue in
                    if newValue == nil {
                        sessionViewModel.select(nil)
                        chatViewModel.showEmptyState()
                    }
                }
                .onAppear {
                    if UITestLaunchOptions.shouldResetState {
                        resetToEmptyConversation()
                    }
                }
        }
    }

    private var splitLayout: some View {
        NavigationSplitView {
            sessionList
                .navigationTitle("Sessions")
                .toolbar {
                    newSessionToolbarButton
                }
                .navigationSplitViewColumnWidth(min: 300, ideal: 340, max: 400)
        } detail: {
            chatDetailView(for: selectedRoute)
                .navigationTitle(splitDetailTitle)
        }
        .navigationSplitViewStyle(.balanced)
    }

    private var sessionList: some View {
        List {
            if sessionViewModel.isLoading {
                ProgressView("Loading sessions...")
                    .foregroundStyle(Color.fawxTextSecondary)
            } else if let errorMessage = sessionViewModel.errorMessage, sessionViewModel.sessions.isEmpty {
                sessionListPlaceholder(
                    title: "Could not load sessions",
                    message: errorMessage,
                    actionTitle: "Retry"
                ) {
                    Task {
                        await sessionViewModel.refresh()
                    }
                }
            } else if sessionViewModel.sessions.isEmpty {
                sessionListPlaceholder(
                    title: "No conversations yet",
                    message: "Start a new one!",
                    actionTitle: "New Session",
                    action: createNewSession
                )
            } else {
                ForEach(sessionViewModel.groupedSections) { section in
                    Section(section.title) {
                        ForEach(section.sessions) { session in
                            sessionRow(for: session)
                        }
                    }
                }
            }
        }
        .accessibilityIdentifier("sessionList")
        .refreshable {
            await sessionViewModel.refresh()
        }
    }

    @ViewBuilder
    private func chatDetailView(for route: SessionRoute) -> some View {
        switch route {
        case .newSession:
            ChatDetailView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                emptyStateTitle: "Conversation",
                emptyStateMessage: "Start typing to ask Fawx for help."
            )
            .navigationTitle("Conversation")
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
        case .session(let sessionID):
            ChatDetailView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                emptyStateTitle: "Conversation",
                emptyStateMessage: "Start typing to ask Fawx for help."
            )
            .navigationTitle(sessionTitle(for: sessionID))
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
            .task(id: sessionID) {
                sessionViewModel.select(sessionID)
                await chatViewModel.loadMessages(for: sessionID)
            }
        }
    }

    private var splitDetailTitle: String {
        switch selectedRoute {
        case .newSession:
            return "Conversation"
        case .session(let sessionID):
            return sessionTitle(for: sessionID)
        }
    }

    private var selectedRoute: SessionRoute {
        if let selectedSessionID = sessionViewModel.selectedSessionID {
            return .session(selectedSessionID)
        }
        return .newSession
    }

    private func sessionTitle(for sessionID: String) -> String {
        sessionViewModel.sessions.first(where: { $0.id == sessionID })?.displayTitle ?? "Conversation"
    }

    private func selectSession(_ sessionID: String) {
        FawxHaptics.lightImpact()
        sessionViewModel.select(sessionID)
        if usesSplitLayout == false {
            navigationPath = [.session(sessionID)]
        }
    }

    private func createNewSession() {
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
        if usesSplitLayout == false {
            navigationPath = [.newSession]
        }
    }

    private func resetToEmptyConversation() {
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
        if usesSplitLayout {
            return
        }
        navigationPath = []
    }

    private func clearSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
            }

            let didClear = await sessionViewModel.clearSession(id: sessionID)
            if didClear, sessionViewModel.selectedSessionID == sessionID {
                await chatViewModel.loadMessages(for: sessionID, force: true)
            }
        }
    }

    private func deleteSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
            }

            let didDelete = await sessionViewModel.deleteSession(id: sessionID)
            if didDelete {
                if sessionViewModel.selectedSessionID == nil {
                    resetToEmptyConversation()
                } else if usesSplitLayout == false, navigationPath.last == .session(sessionID) {
                    navigationPath = []
                }
            }
        }
    }

    private var pendingClearAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingClearSession != nil },
            set: { if !$0 { pendingClearSession = nil } }
        )
    }

    private var newSessionToolbarPlacement: ToolbarItemPlacement {
#if os(iOS)
        .topBarTrailing
#else
        .automatic
#endif
    }

    @ToolbarContentBuilder
    private var newSessionToolbarButton: some ToolbarContent {
        ToolbarItem(placement: newSessionToolbarPlacement) {
            Button {
                createNewSession()
            } label: {
                Label("New", systemImage: "square.and.pencil")
            }
            .accessibilityIdentifier("newSessionButton")
        }
    }

    private func sessionListPlaceholder(
        title: String,
        message: String,
        actionTitle: String,
        action: @escaping () -> Void
    ) -> some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)

            Button(actionTitle, action: action)
                .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, minHeight: 240)
        .listRowBackground(Color.clear)
    }

    @ViewBuilder
    private func sessionRow(for session: Session) -> some View {
        let rowContent = SessionRowView(
            session: session,
            isSelected: session.id == sessionViewModel.selectedSessionID,
            isStreaming: session.id == chatViewModel.activeStreamSessionID
        )

        if usesSplitLayout {
            Button {
                selectSession(session.id)
            } label: {
                rowContent
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("sessionRow_\(session.id)")
            .accessibilityElement(children: .contain)
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                Button("Delete", role: .destructive) {
                    deleteSession(session.id)
                }

                Button("Clear") {
                    pendingClearSession = session
                }
                .tint(.fawxWarning)
            }
        } else {
            Button {
                selectSession(session.id)
            } label: {
                rowContent
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("sessionRow_\(session.id)")
            .accessibilityElement(children: .contain)
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                Button("Delete", role: .destructive) {
                    deleteSession(session.id)
                }

                Button("Clear") {
                    pendingClearSession = session
                }
                .tint(.fawxWarning)
            }
        }
    }

    private var usesSplitLayout: Bool {
#if os(iOS)
        horizontalSizeClass == .regular && UIDevice.current.userInterfaceIdiom == .pad
#else
        false
#endif
    }
}
