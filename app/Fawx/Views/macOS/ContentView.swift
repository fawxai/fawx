import Observation
import SwiftUI

struct ContentView: View {
    private enum Layout {
        static let sidebarMinWidth = FawxSpacing.sidebarWidth
        static let sidebarIdealWidth = FawxSpacing.sidebarWidth + FawxSpacing.paddingXL
        static let sidebarMaxWidth = FawxSpacing.sidebarWidth + (FawxSpacing.paddingXL * 2)
        static let chatDetailMinWidth: CGFloat = 400
        static let compactGitPanelMinWidth: CGFloat = 280
        static let compactGitPanelIdealWidth: CGFloat = 340
        static let compactGitPanelMaxWidth: CGFloat = 420
    }

    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var skillsViewModel: SkillsViewModel
    @Bindable var fleetViewModel: FleetViewModel
    @Bindable var experimentsViewModel: ExperimentsViewModel
    @Bindable var gitViewModel: GitViewModel
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var permissionsViewModel: PermissionsViewModel
    @Bindable var telemetryViewModel: TelemetryViewModel
    @Bindable var synthesisViewModel: SynthesisViewModel
    @Bindable var usageViewModel: UsageViewModel

    @SceneStorage("sidebar_selection") private var sidebarSelectionRawValue: String?
    @AppStorage("show_git_panel") private var showGitPanel = false

    var body: some View {
        VStack(spacing: 0) {
            connectionBannerView
            mainContent
            statusBarView
        }
        .background(Color.fawxBackground)
        .overlay(alignment: .top) {
            toastOverlay
        }
        .task {
            await loadInitialContent()
        }
        .onChange(of: sessionViewModel.selectedSessionID) { _, newValue in
            handleSelectedSessionChange(newValue)
        }
        .onChange(of: appState.sidebarSelection) { _, newValue in
            handleSidebarSelectionChange(newValue)
        }
    }

    @ViewBuilder
    private var connectionBannerView: some View {
        if let banner = appState.connectionBanner {
            ConnectionBannerView(banner: banner) {
                Task {
                    await appState.retryConnection()
                }
            }
        }
    }

    private var mainContent: some View {
        NavigationSplitView {
            sidebarView
        } detail: {
            detailView
        }
        .navigationSplitViewStyle(.balanced)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .layoutPriority(1)
        .toolbar {
            gitPanelToolbar
        }
    }

    private var sidebarView: some View {
        Sidebar(
            sessionViewModel: sessionViewModel,
            selection: sidebarSelection,
            streamingSessionIDs: chatViewModel.activeStreamSessionIDs,
            actions: sidebarActions
        )
        .navigationSplitViewColumnWidth(
            min: Layout.sidebarMinWidth,
            ideal: Layout.sidebarIdealWidth,
            max: Layout.sidebarMaxWidth
        )
    }

    @ViewBuilder
    private var detailView: some View {
        switch sidebarSelection {
        case .skills:
            SkillsView(skillsViewModel: skillsViewModel)
                .navigationTitle("Skills")
        case .fleet:
            FleetView(viewModel: fleetViewModel)
                .navigationTitle("Fleet")
        case .experiments:
            ExperimentsView(viewModel: experimentsViewModel)
                .navigationTitle("Experiments")
        case .git:
            GitView(viewModel: gitViewModel)
                .navigationTitle("Git")
        case .settings:
            SettingsView(
                settingsViewModel: settingsViewModel,
                appState: appState,
                chatViewModel: chatViewModel,
                permissionsViewModel: permissionsViewModel,
                telemetryViewModel: telemetryViewModel,
                synthesisViewModel: synthesisViewModel,
                usageViewModel: usageViewModel
            )
            .navigationTitle("Settings")
        case .session, .none:
            chatDetailContainer
                .navigationTitle(detailTitle)
        }
    }

    private var chatDetail: some View {
        ChatDetailView(
            appState: appState,
            sessionViewModel: sessionViewModel,
            chatViewModel: chatViewModel,
            emptyStateTitle: "What are we working on?",
            emptyStateMessage: "Create a new conversation from the sidebar, or start typing and Fawx will create one on your first message."
        )
    }

    @ViewBuilder
    private var chatDetailContainer: some View {
#if os(macOS)
        if shouldShowGitPanel {
            HSplitView {
                chatDetail
                    .frame(minWidth: Layout.chatDetailMinWidth, maxWidth: .infinity, maxHeight: .infinity)
                    .layoutPriority(1)

                CompactGitPanel(
                    viewModel: gitViewModel,
                    openFullViewAction: openGitView,
                    dismissAction: hideGitPanel
                )
                .frame(
                    minWidth: Layout.compactGitPanelMinWidth,
                    idealWidth: Layout.compactGitPanelIdealWidth,
                    maxWidth: Layout.compactGitPanelMaxWidth,
                    maxHeight: .infinity
                )
            }
        } else {
            chatDetail
        }
#else
        chatDetail
#endif
    }

    private var statusBarView: some View {
        StatusBar(
            connectionStatus: appState.connectionStatus,
            permissionPreset: appState.permissionPresetName,
            modelName: appState.activeModel?.modelID ?? appState.lastHealth?.model,
            context: appState.currentContext,
            selectedSessionMessageCount: sessionViewModel.selectedSession?.messageCount ?? 0
        )
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private var toastOverlay: some View {
        if let toast = appState.toast {
            ToastView(toast: toast)
                .padding(.top, FawxSpacing.paddingLG)
        }
    }

    @ToolbarContentBuilder
    private var gitPanelToolbar: some ToolbarContent {
        if isChatSectionSelected {
            ToolbarItem(placement: .primaryAction) {
                Button(action: toggleGitPanel) {
                    Label(gitPanelButtonTitle, systemImage: "sidebar.right")
                }
                .help(gitPanelButtonHelp)
            }
        }
    }

    private var detailTitle: String {
        if let session = sessionViewModel.selectedSession {
            return session.displayTitle
        }
        return "New Session"
    }

    private var sidebarSelection: SidebarSelection? {
        get { appState.sidebarSelection ?? sidebarSelectionRawValue.flatMap(SidebarSelection.init(rawValue:)) }
        nonmutating set {
            sidebarSelectionRawValue = newValue?.rawValue
            appState.sidebarSelection = newValue
        }
    }

    private var selectedSessionID: String? {
        if case .session(let sessionID) = sidebarSelection {
            return sessionID
        }
        return nil
    }

    private var sidebarActions: Sidebar.ActionHandlers {
        Sidebar.ActionHandlers(
            newSession: beginNewSession,
            selectSession: selectSession,
            showSkills: showSkills,
            showFleet: showFleet,
            showExperiments: showExperiments,
            showGit: showGit,
            openGitPanel: openGitPanel,
            openSettings: showSettings,
            clearSession: clearSession,
            deleteSession: deleteSession,
            deleteSessions: deleteSessions
        )
    }

    private var gitPanelSelectionID: String? {
        sessionViewModel.selectedSessionID ?? selectedSessionID
    }

    private var gitPanelButtonTitle: String {
        shouldShowGitPanel ? "Hide Git Panel" : "Show Git Panel"
    }

    private var gitPanelButtonHelp: String {
        shouldShowGitPanel ? "Hide Git side panel" : "Show Git side panel"
    }

    private func loadInitialContent() async {
        if appState.sidebarSelection == nil {
            appState.sidebarSelection = sidebarSelectionRawValue.flatMap(SidebarSelection.init(rawValue:))
        }

        await sessionViewModel.refresh()
        await restoreSelectionAfterRefresh()
    }

    private func handleSelectedSessionChange(_ newValue: String?) {
        if let newValue {
            if sidebarSelection != .session(newValue) {
                sidebarSelection = .session(newValue)
            }
            chatViewModel.prepareToDisplaySession(newValue)
            chatViewModel.scheduleLoadMessages(for: newValue, force: true)
        } else if case .session = sidebarSelection {
            beginNewSession()
        }
    }

    private func handleSidebarSelectionChange(_ newValue: SidebarSelection?) {
        sidebarSelectionRawValue = newValue?.rawValue
    }

    private func beginNewSession() {
        chatViewModel.cancelScheduledLoad()
        sidebarSelection = nil
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func selectSession(_ sessionID: String) {
        sidebarSelection = .session(sessionID)
        chatViewModel.prepareToDisplaySession(sessionID)
        sessionViewModel.select(sessionID)
    }

    private func showSkills() {
        switchToNonChatSection(.skills)
    }

    private func showFleet() {
        switchToNonChatSection(.fleet)
    }

    private func showExperiments() {
        switchToNonChatSection(.experiments)
    }

    private func showGit() {
        switchToNonChatSection(.git)
    }

    private func showSettings() {
        switchToNonChatSection(.settings)
    }

    private func restoreSelectionAfterRefresh() async {
        if let selectedSessionID, sessionViewModel.sessions.contains(where: { $0.id == selectedSessionID }) {
            sessionViewModel.select(selectedSessionID)
            chatViewModel.scheduleLoadMessages(for: selectedSessionID, force: true)
        } else {
            if case .session = sidebarSelection {
                sidebarSelection = nil
            }
            chatViewModel.showEmptyState()
        }
    }

    private func clearSession(_ sessionID: String) {
        Task {
            if chatViewModel.activeStreamSessionIDs.contains(sessionID) {
                chatViewModel.stopStreaming(sessionID: sessionID)
            }

            let didClear = await sessionViewModel.clearSession(id: sessionID)
            if didClear, sessionViewModel.selectedSessionID == sessionID {
                chatViewModel.invalidateSession(sessionID)
                chatViewModel.scheduleLoadMessages(for: sessionID, force: true)
            }
        }
    }

    private func deleteSession(_ sessionID: String) {
        Task {
            if chatViewModel.activeStreamSessionIDs.contains(sessionID) {
                chatViewModel.stopStreaming(sessionID: sessionID)
            }

            let didDelete = await sessionViewModel.deleteSession(id: sessionID)
            if didDelete {
                chatViewModel.invalidateSession(sessionID)
                if selectedSessionID == sessionID {
                    beginNewSession()
                } else if sessionViewModel.selectedSessionID == nil {
                    chatViewModel.showEmptyState()
                } else {
                    chatViewModel.scheduleLoadMessages(for: sessionViewModel.selectedSessionID)
                }
            }
        }
    }

    private func deleteSessions(_ sessionIDs: [String]) {
        let orderedSessionIDs = sessionViewModel.sessions
            .map(\.id)
            .filter { sessionIDs.contains($0) }
        guard orderedSessionIDs.isEmpty == false else {
            return
        }

        Task {
            let deletedCurrentSelection = orderedSessionIDs.contains { $0 == selectedSessionID }
            for sessionID in orderedSessionIDs where chatViewModel.activeStreamSessionIDs.contains(sessionID) {
                chatViewModel.stopStreaming(sessionID: sessionID)
            }

            let deletedSessionIDs = await sessionViewModel.deleteSessions(ids: orderedSessionIDs)
            for sessionID in deletedSessionIDs {
                chatViewModel.invalidateSession(sessionID)
            }

            if deletedCurrentSelection {
                beginNewSession()
            } else if sessionViewModel.selectedSessionID == nil {
                chatViewModel.showEmptyState()
            } else {
                chatViewModel.scheduleLoadMessages(for: sessionViewModel.selectedSessionID)
            }
        }
    }

    private func switchToNonChatSection(_ selection: SidebarSelection) {
        chatViewModel.cancelScheduledLoad()
        GitPanelPresentation.hide(showGitPanel: $showGitPanel)
        sidebarSelection = selection
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private var isChatSectionSelected: Bool {
        switch sidebarSelection {
        case .session, .none:
            true
        case .skills, .fleet, .experiments, .git, .settings:
            false
        }
    }

    private var shouldShowGitPanel: Bool {
        showGitPanel && isChatSectionSelected
    }

    private func toggleGitPanel() {
        GitPanelPresentation.toggle(
            showGitPanel: $showGitPanel,
            selectedSessionID: gitPanelSelectionID,
            appState: appState,
            sessionViewModel: sessionViewModel,
            chatViewModel: chatViewModel
        )
    }

    private func openGitPanel() {
        GitPanelPresentation.show(
            showGitPanel: $showGitPanel,
            selectedSessionID: gitPanelSelectionID,
            appState: appState,
            sessionViewModel: sessionViewModel,
            chatViewModel: chatViewModel
        )
    }

    private func hideGitPanel() {
        GitPanelPresentation.hide(showGitPanel: $showGitPanel)
    }

    private func openGitView() {
        switchToNonChatSection(.git)
    }

}
