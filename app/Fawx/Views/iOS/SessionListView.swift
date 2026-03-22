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
    let openSkills: () -> Void
    let openFleet: () -> Void
    let openExperiments: () -> Void
    let openGit: () -> Void
    let openSettings: () -> Void

    @State private var navigationPath: [SessionRoute] = []
    @State private var pendingClearSession: Session?
    @State private var hasPresentedInitialRoute = false
    @State private var searchText = ""

    var body: some View {
        Group {
            if usesSplitLayout {
                splitLayout
            } else {
                stackLayout
            }
        }
        .onChange(of: sessionViewModel.selectedSessionID) { _, newValue in
            guard usesSplitLayout else {
                return
            }

            if let sessionID = newValue {
                chatViewModel.prepareToDisplaySession(sessionID)
                chatViewModel.scheduleLoadMessages(for: sessionID, force: true)
            } else {
                chatViewModel.cancelScheduledLoad()
                chatViewModel.showEmptyState()
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
                .iOSInlineNavigationTitle()
                .toolbar {
                    rootMenuToolbarButton
                }
                .navigationDestination(for: SessionRoute.self) { route in
                    chatDetailView(for: route)
                }
                .onChange(of: navigationPath.last) { _, newValue in
                    switch newValue {
                    case nil, .newSession:
                        chatViewModel.cancelScheduledLoad()
                        sessionViewModel.select(nil)
                        chatViewModel.showEmptyState()
                    case .session(let sessionID):
                        sessionViewModel.select(sessionID)
                        chatViewModel.prepareToDisplaySession(sessionID)
                        chatViewModel.scheduleLoadMessages(for: sessionID, force: true)
                    }
                }
                .onAppear {
                    if UITestLaunchOptions.shouldResetState {
                        resetToEmptyConversation()
                        hasPresentedInitialRoute = false
                    }

                    presentInitialRouteIfNeeded()
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

    @ViewBuilder
    private var sessionList: some View {
#if os(iOS)
        if usesSplitLayout {
            sessionListBody
                .searchable(
                    text: $searchText,
                    placement: .navigationBarDrawer(displayMode: .always),
                    prompt: "Search sessions"
                )
        } else {
            sessionListBody
        }
#else
        sessionListBody
#endif
    }

    private var sessionListBody: some View {
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
            } else if filteredGroupedSections.isEmpty {
                sessionListPlaceholder(
                    title: "No sessions matching \"\(trimmedSearchText)\"",
                    message: "Try a different search term.",
                    actionTitle: "Clear Search"
                ) {
                    searchText = ""
                }
            } else {
                ForEach(filteredGroupedSections) { section in
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
#if os(iOS)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if usesSplitLayout == false {
                sessionBottomControls
            }
        }
#endif
    }

    @ViewBuilder
    private func chatDetailView(for route: SessionRoute) -> some View {
        switch route {
        case .newSession:
            mobileChatShell(
                ChatDetailView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel,
                    emptyStateTitle: "Start a new session",
                    emptyStateMessage: "Let's get started"
                )
                .navigationTitle("New Session")
            )
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
        case .session(let sessionID):
            mobileChatShell(
                ChatDetailView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel,
                    emptyStateTitle: "Start a new session",
                    emptyStateMessage: "Let's get started"
                )
                .navigationTitle(sessionTitle(for: sessionID))
            )
#if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
#endif
        }
    }

    private var splitDetailTitle: String {
        switch selectedRoute {
        case .newSession:
            return "New Session"
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
        chatViewModel.prepareToDisplaySession(sessionID)
        if usesSplitLayout {
            sessionViewModel.select(sessionID)
        }
        if usesSplitLayout == false {
            navigationPath = [.session(sessionID)]
        }
    }

    private func createNewSession() {
        chatViewModel.cancelScheduledLoad()
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
        if usesSplitLayout == false {
            navigationPath = [.newSession]
        }
    }

    private func resetToEmptyConversation() {
        chatViewModel.cancelScheduledLoad()
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
        if usesSplitLayout {
            return
        }
        navigationPath = []
    }

    private func presentInitialRouteIfNeeded() {
        guard hasPresentedInitialRoute == false else {
            return
        }

        chatViewModel.cancelScheduledLoad()
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()

        if usesSplitLayout == false {
            navigationPath = [.newSession]
        }

        hasPresentedInitialRoute = true
    }

    private func showSessionsList() {
        chatViewModel.cancelScheduledLoad()
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
        if usesSplitLayout == false {
            navigationPath = []
        }
    }

    @ViewBuilder
    private func mobileChatShell<Content: View>(_ content: Content) -> some View {
#if os(iOS)
        if usesSplitLayout {
            content
        } else {
            content
                .navigationBarBackButtonHidden(true)
                .toolbar(.hidden, for: .tabBar)
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) {
                        SectionMenuButton(
                            disabledSection: nil,
                            showSessions: showSessionsList,
                            showSkills: openSkills,
                            showFleet: openFleet,
                            showExperiments: openExperiments,
                            showGit: openGit,
                            showSettings: openSettings
                        )
                    }
                }
        }
#else
        content
#endif
    }

    private func clearSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionIDs.contains(sessionID) {
                chatViewModel.stopStreaming(sessionID: sessionID)
            }

            let didClear = await sessionViewModel.clearSession(id: sessionID)
            if didClear, sessionViewModel.selectedSessionID == sessionID {
                chatViewModel.invalidateSession(sessionID)
                chatViewModel.prepareToDisplaySession(sessionID)
                chatViewModel.scheduleLoadMessages(for: sessionID, force: true)
            }
        }
    }

    private func deleteSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionIDs.contains(sessionID) {
                chatViewModel.stopStreaming(sessionID: sessionID)
            }

            let didDelete = await sessionViewModel.deleteSession(id: sessionID)
            if didDelete {
                chatViewModel.invalidateSession(sessionID)
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

    @ToolbarContentBuilder
    private var rootMenuToolbarButton: some ToolbarContent {
        if usesSplitLayout == false {
            ToolbarItem(placement: .fawxTopLeading) {
                SectionMenuButton(
                    disabledSection: .sessions,
                    showSessions: showSessionsList,
                    showSkills: openSkills,
                    showFleet: openFleet,
                    showExperiments: openExperiments,
                    showGit: openGit,
                    showSettings: openSettings
                )
            }
        }
    }

    private var sessionBottomControls: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            BottomSearchBar(
                text: $searchText,
                prompt: "Search sessions",
                accessibilityIdentifier: "sessionSearchField",
                includesContainerChrome: false
            )
            .frame(maxWidth: .infinity)

            Button(action: createNewSession) {
                Image(systemName: "square.and.pencil")
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(Color.white)
                    .frame(width: 52, height: 52)
                    .background(Color.fawxAccent)
                    .clipShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("newSessionButton")
        }
        .padding(.horizontal, FawxSpacing.paddingLG)
        .padding(.top, FawxSpacing.paddingSM)
        .padding(.bottom, FawxSpacing.paddingMD)
        .background(Color.fawxBackground.opacity(0.96))
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

    private var filteredGroupedSections: [SessionSection] {
        SessionViewModel.filterSessionSections(sessionViewModel.groupedSections, query: searchText)
    }

    private var trimmedSearchText: String {
        searchText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    @ViewBuilder
    private func sessionRow(for session: Session) -> some View {
        let rowContent = SessionRowView(
            session: session,
            isSelected: session.id == sessionViewModel.selectedSessionID,
            isStreaming: chatViewModel.activeStreamSessionIDs.contains(session.id)
        )

        Button {
            selectSession(session.id)
        } label: {
            rowContent
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("sessionRow_\(session.id)")
        .accessibilityElement(children: .contain)
        .listRowInsets(
            EdgeInsets(
                top: 0,
                leading: FawxSpacing.paddingSM,
                bottom: 0,
                trailing: FawxSpacing.paddingSM
            )
        )
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
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

    private var usesSplitLayout: Bool {
#if os(iOS)
        horizontalSizeClass == .regular && UIDevice.current.userInterfaceIdiom == .pad
#else
        false
#endif
    }

}
