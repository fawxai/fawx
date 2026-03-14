import SwiftUI

@main
struct FawxApp: App {
    @Environment(\.scenePhase) private var scenePhase

    @State private var appState: AppState
    @State private var sessionViewModel: SessionViewModel
    @State private var chatViewModel: ChatViewModel
    @State private var settingsViewModel: SettingsViewModel

    init() {
        let appState = AppState()
        let sessionViewModel = SessionViewModel(appState: appState)
        let chatViewModel = ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
        let settingsViewModel = SettingsViewModel(appState: appState)

        _appState = State(initialValue: appState)
        _sessionViewModel = State(initialValue: sessionViewModel)
        _chatViewModel = State(initialValue: chatViewModel)
        _settingsViewModel = State(initialValue: settingsViewModel)
    }

    var body: some Scene {
        WindowGroup {
            rootView
                .preferredColorScheme(appState.preferredColorScheme)
                .task(id: appState.configurationKey) {
                    await appState.bootstrap()
                    settingsViewModel.reloadStoredValues()
                    await sessionViewModel.refresh()
                    await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID, force: true)
                }
                .task(id: appState.configurationKey + "|polling") {
                    guard appState.isConfigured else {
                        return
                    }

                    while !Task.isCancelled {
                        try? await Task.sleep(for: .seconds(30))
                        guard appState.isConfigured else {
                            continue
                        }

                        try? await appState.refreshServerState()
                        await sessionViewModel.refresh()
                        await appState.refreshContext(for: sessionViewModel.selectedSessionID)
                    }
                }
                .onChange(of: scenePhase) { _, newPhase in
                    guard newPhase == .active, appState.isConfigured else {
                        return
                    }

                    Task {
                        await appState.bootstrap()
                        await sessionViewModel.refresh()
                        await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID, force: true)
                    }
                }
        }
#if os(macOS)
        Settings {
            SettingsView(
                settingsViewModel: settingsViewModel,
                appState: appState,
                chatViewModel: chatViewModel
            )
        }
#endif
    }

    @ViewBuilder
    private var rootView: some View {
        if appState.isConfigured {
#if os(macOS)
            ContentView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel
            )
#else
            TabRootView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                settingsViewModel: settingsViewModel
            )
#endif
        } else {
            OnboardingView(settingsViewModel: settingsViewModel)
        }
    }
}
