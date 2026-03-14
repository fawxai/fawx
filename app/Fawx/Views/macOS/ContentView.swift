import Observation
import SwiftUI

struct ContentView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    var body: some View {
        VStack(spacing: 0) {
            NavigationSplitView {
                Sidebar(
                    sessionViewModel: sessionViewModel,
                    streamingSessionID: chatViewModel.activeStreamSessionID,
                    newSessionAction: createNewSession,
                    clearSessionAction: clearSession,
                    deleteSessionAction: deleteSession
                )
                .frame(minWidth: 260)
            } detail: {
                ChatDetailView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel,
                    emptyStateTitle: "What are you working on?",
                    emptyStateMessage: "Create a new conversation from the sidebar, or start typing and Fawx will create one on your first message."
                )
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
        .task {
            await sessionViewModel.refresh()
            await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID, force: true)
        }
        .onChange(of: sessionViewModel.selectedSessionID) { _, newValue in
            Task {
                await chatViewModel.loadMessages(for: newValue)
            }
        }
    }

    private func createNewSession() {
        Task {
            if let sessionID = await sessionViewModel.createNewSession() {
                await chatViewModel.loadMessages(for: sessionID)
            }
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
                if sessionViewModel.selectedSessionID == nil {
                    chatViewModel.showEmptyState()
                } else {
                    await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID)
                }
            }
        }
    }
}
