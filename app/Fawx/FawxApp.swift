import SwiftUI
#if os(macOS)
import AppKit
#endif

@main
struct FawxApp: App {
    @Environment(\.scenePhase) private var scenePhase

    @State private var appState: AppState
    @State private var sessionViewModel: SessionViewModel
    @State private var chatViewModel: ChatViewModel
    @State private var skillsViewModel: SkillsViewModel
    @State private var settingsViewModel: SettingsViewModel

    init() {
        let appState = AppState()
        let sessionViewModel = SessionViewModel(appState: appState)
        let chatViewModel = ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
        let skillsViewModel = SkillsViewModel(appState: appState)
        let settingsViewModel = SettingsViewModel(appState: appState)

        _appState = State(initialValue: appState)
        _sessionViewModel = State(initialValue: sessionViewModel)
        _chatViewModel = State(initialValue: chatViewModel)
        _skillsViewModel = State(initialValue: skillsViewModel)
        _settingsViewModel = State(initialValue: settingsViewModel)
    }

    var body: some Scene {
        let selectedTheme = appState.theme
        let selectedFontSize = appState.fontSize
        let _ = FawxTypography.setScale(selectedFontSize.scale)
#if os(macOS)
        let _ = applyMacAppearance(selectedTheme)
#endif

        mainWindowScene(selectedTheme: selectedTheme)
    }

    private func mainWindowScene(selectedTheme: AppTheme) -> some Scene {
        WindowGroup {
            themedRootView(selectedTheme: selectedTheme)
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
                        guard appState.isConfigured, appState.connectionStatus == .connected else {
                            continue
                        }

                        do {
                            _ = try await appState.client.health()
                            try await appState.refreshServerState()
                            await sessionViewModel.refresh()
                            await appState.refreshContext(for: sessionViewModel.selectedSessionID)
                        } catch {
                            await appState.noteRecoverableRequestFailure(error)
                        }
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
        .commands {
            FawxMacCommands(
                appState: appState,
                sessionViewModel: sessionViewModel,
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
                chatViewModel: chatViewModel,
                skillsViewModel: skillsViewModel,
                settingsViewModel: settingsViewModel
            )
#else
            TabRootView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                skillsViewModel: skillsViewModel,
                settingsViewModel: settingsViewModel
            )
#endif
        } else {
            OnboardingView(settingsViewModel: settingsViewModel)
        }
    }

    @ViewBuilder
    private func themedRootView(selectedTheme: AppTheme) -> some View {
#if os(macOS)
        rootView
#else
        rootView
            .preferredColorScheme(selectedTheme.colorScheme)
#endif
    }

#if os(macOS)
    @discardableResult
    private func applyMacAppearance(_ theme: AppTheme) -> Bool {
        let appearance: NSAppearance? = switch theme {
        case .system:
            nil
        case .light:
            NSAppearance(named: .aqua)
        case .dark:
            NSAppearance(named: .darkAqua)
        }

        NSApp.appearance = appearance
        return true
    }
#endif
}
