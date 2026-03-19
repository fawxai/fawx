#if os(macOS)
import Observation
import SwiftUI

struct FawxMacCommands: Commands {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Session") {
                beginNewSession()
            }
            .keyboardShortcut("n", modifiers: .command)
        }

        CommandMenu("Session") {
            Button("Clear Session History") {
                clearSelectedSession()
            }
            .keyboardShortcut("k", modifiers: .command)
            .disabled(sessionViewModel.selectedSessionID == nil)
        }

        CommandMenu("Navigate") {
            Button("Sessions") {
                showSessions()
            }
            .keyboardShortcut("1", modifiers: .command)

            Button("Skills") {
                showSkills()
            }
            .keyboardShortcut("2", modifiers: .command)

            Button("Settings") {
                showSettings()
            }
            .keyboardShortcut("3", modifiers: .command)
        }
    }

    private func beginNewSession() {
        appState.sidebarSelection = nil
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func showSessions() {
        if let selectedSessionID = sessionViewModel.selectedSessionID {
            appState.sidebarSelection = .session(selectedSessionID)
            sessionViewModel.select(selectedSessionID)
        } else {
            beginNewSession()
        }
    }

    private func showSkills() {
        appState.sidebarSelection = .skills
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func showSettings() {
        appState.sidebarSelection = .settings
        sessionViewModel.select(nil)
        chatViewModel.showEmptyState()
    }

    private func clearSelectedSession() {
        guard let selectedSessionID = sessionViewModel.selectedSessionID else {
            return
        }

        Task { @MainActor in
            if chatViewModel.activeStreamSessionID == selectedSessionID {
                chatViewModel.stopStreaming()
            }

            let didClear = await sessionViewModel.clearSession(id: selectedSessionID)
            if didClear {
                await chatViewModel.loadMessages(for: selectedSessionID, force: true)
            }
        }
    }

}
#endif
