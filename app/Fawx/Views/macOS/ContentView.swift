import Observation
import SwiftUI

struct ContentView: View {
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

    var body: some View {
        VStack(spacing: 0) {
            if let banner = appState.connectionBanner {
                ConnectionBannerView(banner: banner) {
                    Task {
                        await appState.retryConnection()
                    }
                }
            }

            NavigationSplitView {
                Sidebar(
                    sessionViewModel: sessionViewModel,
                    selection: sidebarSelection,
                    streamingSessionID: chatViewModel.activeStreamSessionID,
                    newSessionAction: beginNewSession,
                    selectSessionAction: selectSession,
                    showSkillsAction: showSkills,
                    showFleetAction: showFleet,
                    showExperimentsAction: showExperiments,
                    showGitAction: showGit,
                    openSettingsAction: showSettings,
                    clearSessionAction: clearSession,
                    deleteSessionAction: deleteSession,
                    deleteSessionsAction: deleteSessions
                )
                .frame(minWidth: 260)
            } detail: {
                detailView
            }
            .navigationSplitViewStyle(.balanced)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .layoutPriority(1)

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
        .background(Color.fawxBackground)
        .overlay(alignment: .top) {
            if let toast = appState.toast {
                ToastView(toast: toast)
                    .padding(.top, FawxSpacing.paddingLG)
            }
        }
        .task {
            if appState.sidebarSelection == nil {
                appState.sidebarSelection = sidebarSelectionRawValue.flatMap(SidebarSelection.init(rawValue:))
            }
            await sessionViewModel.refresh()
            await restoreSelectionAfterRefresh()
        }
        .onChange(of: sessionViewModel.selectedSessionID) { _, newValue in
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
        .onChange(of: appState.sidebarSelection) { _, newValue in
            sidebarSelectionRawValue = newValue?.rawValue
        }
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
            ChatDetailView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                emptyStateTitle: "What are you working on?",
                emptyStateMessage: "Create a new conversation from the sidebar, or start typing and Fawx will create one on your first message."
            )
            .navigationTitle(detailTitle)
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
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
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
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
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

            for sessionID in orderedSessionIDs {
                if chatViewModel.activeStreamSessionID == sessionID {
                    chatViewModel.stopStreaming()
                }

                let didDelete = await sessionViewModel.deleteSession(id: sessionID)
                if didDelete {
                    chatViewModel.invalidateSession(sessionID)
                }
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
        sidebarSelection = selection
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

}
