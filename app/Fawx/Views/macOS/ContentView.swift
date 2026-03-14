import Observation
import SwiftUI
#if os(macOS)
import AppKit
#endif

struct ContentView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var skillsViewModel: SkillsViewModel

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
                    openSettingsAction: openSettingsWindow,
                    clearSessionAction: clearSession,
                    deleteSessionAction: deleteSession
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
                context: appState.currentContext
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
            Task {
                guard selectedSessionID != newValue else {
                    return
                }
                if let newValue {
                    sidebarSelection = .session(newValue)
                    await chatViewModel.loadMessages(for: newValue)
                } else if selectedSessionID != nil {
                    beginNewSession()
                }
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
        case .session, .none:
            ChatDetailView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                emptyStateTitle: "What are you working on?",
                emptyStateMessage: "Create a new conversation from the sidebar, or start typing and Fawx will create one on your first message."
            )
        }
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
        sidebarSelection = nil
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func selectSession(_ sessionID: String) {
        sidebarSelection = .session(sessionID)
        sessionViewModel.select(sessionID)
    }

    private func showSkills() {
        sidebarSelection = .skills
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func openSettingsWindow() {
#if os(macOS)
        NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
#endif
    }

    private func restoreSelectionAfterRefresh() async {
        if let selectedSessionID, sessionViewModel.sessions.contains(where: { $0.id == selectedSessionID }) {
            sessionViewModel.select(selectedSessionID)
            await chatViewModel.loadMessages(for: selectedSessionID, force: true)
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
                await chatViewModel.loadMessages(for: sessionID, force: true)
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
                if selectedSessionID == sessionID {
                    beginNewSession()
                } else if sessionViewModel.selectedSessionID == nil {
                    chatViewModel.showEmptyState()
                } else {
                    await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID)
                }
            }
        }
    }
}
