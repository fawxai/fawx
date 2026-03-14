import Observation
import SwiftUI

struct TabRootView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var settingsViewModel: SettingsViewModel

    var body: some View {
        TabView {
            SessionListView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel
            )
                .tabItem {
                    Label("Chat", systemImage: "bubble.left.and.bubble.right")
                }

            iOSSettingsView(
                settingsViewModel: settingsViewModel,
                appState: appState,
                chatViewModel: chatViewModel
            )
                .tabItem {
                    Label("Settings", systemImage: "gear")
                }
        }
    }
}
