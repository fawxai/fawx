import Observation
import SwiftUI

struct TabRootView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var skillsViewModel: SkillsViewModel
    @Bindable var settingsViewModel: SettingsViewModel

    var body: some View {
        VStack(spacing: 0) {
            if let banner = appState.connectionBanner {
                ConnectionBannerView(banner: banner) {
                    Task {
                        await appState.retryConnection()
                    }
                }
            }

            TabView {
                SessionListView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel
                )
                    .tabItem {
                        Label("Chat", systemImage: "bubble.left.and.bubble.right")
                    }

                NavigationStack {
                    SkillsView(skillsViewModel: skillsViewModel, showsHeader: false)
                        .navigationTitle("Skills")
                }
                    .tabItem {
                        Label("Skills", systemImage: "puzzlepiece.extension")
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
        .overlay(alignment: .top) {
            if let toast = appState.toast {
                ToastView(toast: toast)
                    .padding(.top, 60)
            }
        }
    }
}
