import SwiftUI

enum GitPanelPresentation {
    @MainActor
    static func toggle(
        showGitPanel: Binding<Bool>,
        selectedSessionID: String?,
        appState: AppState,
        sessionViewModel: SessionViewModel,
        chatViewModel: ChatViewModel
    ) {
        if showGitPanel.wrappedValue {
            hide(showGitPanel: showGitPanel)
        } else {
            show(
                showGitPanel: showGitPanel,
                selectedSessionID: selectedSessionID,
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel
            )
        }
    }

    @MainActor
    static func show(
        showGitPanel: Binding<Bool>,
        selectedSessionID: String?,
        appState: AppState,
        sessionViewModel: SessionViewModel,
        chatViewModel: ChatViewModel
    ) {
        let activeSessionID = selectedSessionID ?? sessionViewModel.selectedSessionID

        if let activeSessionID {
            sessionViewModel.select(activeSessionID)
            chatViewModel.prepareToDisplaySession(activeSessionID)
        } else {
            sessionViewModel.select(nil)
            chatViewModel.showEmptyState()
        }

        showGitPanel.wrappedValue = true
    }

    static func hide(showGitPanel: Binding<Bool>) {
        showGitPanel.wrappedValue = false
    }
}
