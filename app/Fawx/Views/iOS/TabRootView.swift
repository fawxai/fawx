import Observation
import SwiftUI

private enum RootTab: Hashable {
    case chat
    case skills
    case settings
}

struct TabRootView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var skillsViewModel: SkillsViewModel
    @Bindable var settingsViewModel: SettingsViewModel
    @State private var selectedTab: RootTab = .chat

    var body: some View {
        VStack(spacing: 0) {
            if let banner = appState.connectionBanner {
                ConnectionBannerView(banner: banner) {
                    Task {
                        await appState.retryConnection()
                    }
                }
            }

            TabView(selection: $selectedTab) {
                SessionListView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel
                )
                    .tabItem {
                        Label("Chat", systemImage: "bubble.left.and.bubble.right")
                    }
                    .tag(RootTab.chat)

                NavigationStack {
                    SkillsView(skillsViewModel: skillsViewModel, showsHeader: false)
                        .navigationTitle("Skills")
                }
                    .tabItem {
                        Label("Skills", systemImage: "puzzlepiece.extension")
                    }
                    .tag(RootTab.skills)

                iOSSettingsView(
                    settingsViewModel: settingsViewModel,
                    appState: appState,
                    chatViewModel: chatViewModel
                )
                    .tabItem {
                        Label("Settings", systemImage: "gear")
                    }
                    .tag(RootTab.settings)
            }
        }
        .overlay(alignment: .top) {
            if let toast = appState.toast {
                ToastView(toast: toast)
                    .padding(.top, 60)
            }
        }
        .onAppear {
            if UITestLaunchOptions.shouldResetState {
                selectedTab = .chat
            }
        }
    }
}
