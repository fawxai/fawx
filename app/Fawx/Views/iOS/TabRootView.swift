import Observation
import SwiftUI

private enum RootTab: Hashable {
    case chat
    case skills
    case fleet
    case experiments
    case git
    case settings
}

struct TabRootView: View {
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
                        openFleet: {
                            selectedTab = .fleet
                        },
                        openExperiments: {
                            selectedTab = .experiments
                        },
                        openGit: {
                            selectedTab = .git
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
                            isActive: selectedTab == .skills,
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
                                        showFleet: {
                                            selectedTab = .fleet
                                        },
                                        showExperiments: {
                                            selectedTab = .experiments
                                        },
                                        showGit: {
                                            selectedTab = .git
                                        },
                                        showSettings: {
                                            selectedTab = .settings
                                        }
                                    )
                                }
                            }
                    }
                }

                rootSectionContainer(isActive: selectedTab == .fleet) {
                    NavigationStack {
                        FleetView(
                            viewModel: fleetViewModel,
                            isActive: selectedTab == .fleet
                        )
                            .navigationTitle("Fleet")
                            .iOSInlineNavigationTitle()
                            .toolbar {
                                ToolbarItem(placement: .fawxTopLeading) {
                                    SectionMenuButton(
                                        disabledSection: .fleet,
                                        showSessions: {
                                            selectedTab = .chat
                                        },
                                        showSkills: {
                                            selectedTab = .skills
                                        },
                                        showFleet: {},
                                        showExperiments: {
                                            selectedTab = .experiments
                                        },
                                        showGit: {
                                            selectedTab = .git
                                        },
                                        showSettings: {
                                            selectedTab = .settings
                                        }
                                    )
                                }
                            }
                    }
                }

                rootSectionContainer(isActive: selectedTab == .experiments) {
                    NavigationStack {
                        ExperimentsView(
                            viewModel: experimentsViewModel,
                            isActive: selectedTab == .experiments
                        )
                            .navigationTitle("Experiments")
                            .iOSInlineNavigationTitle()
                            .toolbar {
                                ToolbarItem(placement: .fawxTopLeading) {
                                    SectionMenuButton(
                                        disabledSection: .experiments,
                                        showSessions: {
                                            selectedTab = .chat
                                        },
                                        showSkills: {
                                            selectedTab = .skills
                                        },
                                        showFleet: {
                                            selectedTab = .fleet
                                        },
                                        showExperiments: {},
                                        showGit: {
                                            selectedTab = .git
                                        },
                                        showSettings: {
                                            selectedTab = .settings
                                        }
                                    )
                                }
                            }
                    }
                }

                rootSectionContainer(isActive: selectedTab == .git) {
                    NavigationStack {
                        GitView(
                            viewModel: gitViewModel,
                            isActive: selectedTab == .git
                        )
                            .navigationTitle("Git")
                            .iOSInlineNavigationTitle()
                            .toolbar {
                                ToolbarItem(placement: .fawxTopLeading) {
                                    SectionMenuButton(
                                        disabledSection: .git,
                                        showSessions: {
                                            selectedTab = .chat
                                        },
                                        showSkills: {
                                            selectedTab = .skills
                                        },
                                        showFleet: {
                                            selectedTab = .fleet
                                        },
                                        showExperiments: {
                                            selectedTab = .experiments
                                        },
                                        showGit: {},
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
                        telemetryViewModel: telemetryViewModel,
                        synthesisViewModel: synthesisViewModel,
                        usageViewModel: usageViewModel,
                        openSessions: {
                            selectedTab = .chat
                        },
                        openSkills: {
                            selectedTab = .skills
                        },
                        openFleet: {
                            selectedTab = .fleet
                        },
                        openExperiments: {
                            selectedTab = .experiments
                        },
                        openGit: {
                            selectedTab = .git
                        },
                        isActive: selectedTab == .settings
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
