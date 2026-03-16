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
    @Bindable var permissionsViewModel: PermissionsViewModel
    @Bindable var synthesisViewModel: SynthesisViewModel
    @Bindable var usageViewModel: UsageViewModel
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

            ZStack {
                rootSectionContainer(isActive: selectedTab == .chat) {
                    SessionListView(
                        appState: appState,
                        sessionViewModel: sessionViewModel,
                        chatViewModel: chatViewModel,
                        openSkills: {
                            selectedTab = .skills
                        },
                        openSettings: {
                            selectedTab = .settings
                        }
                    )
                }

                rootSectionContainer(isActive: selectedTab == .skills) {
                    NavigationStack {
                        SkillsView(
                            skillsViewModel: skillsViewModel,
                            showsHeader: false
                        )
                            .navigationTitle("Skills")
                            .iOSInlineNavigationTitle()
                            .toolbar {
                                ToolbarItem(placement: .fawxTopLeading) {
                                    SectionMenuButton(
                                        disabledSection: .skills,
                                        showSessions: {
                                            selectedTab = .chat
                                        },
                                        showSkills: {},
                                        showSettings: {
                                            selectedTab = .settings
                                        }
                                    )
                                }
                            }
                    }
                }

                rootSectionContainer(isActive: selectedTab == .settings) {
                    iOSSettingsView(
                        settingsViewModel: settingsViewModel,
                        appState: appState,
                        chatViewModel: chatViewModel,
                        permissionsViewModel: permissionsViewModel,
                        synthesisViewModel: synthesisViewModel,
                        usageViewModel: usageViewModel,
                        openSessions: {
                            selectedTab = .chat
                        },
                        openSkills: {
                            selectedTab = .skills
                        }
                    )
                }
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

    @ViewBuilder
    private func rootSectionContainer<Content: View>(
        isActive: Bool,
        @ViewBuilder content: () -> Content
    ) -> some View {
        content()
            .opacity(isActive ? 1 : 0)
            .allowsHitTesting(isActive)
            .accessibilityHidden(!isActive)
            .zIndex(isActive ? 1 : 0)
    }
}
