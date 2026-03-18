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
    @State private var fleetViewModel: FleetViewModel
    @State private var experimentsViewModel: ExperimentsViewModel
    @State private var gitViewModel: GitViewModel
    @State private var settingsViewModel: SettingsViewModel
    @State private var permissionsViewModel: PermissionsViewModel
    @State private var telemetryViewModel: TelemetryViewModel
    @State private var synthesisViewModel: SynthesisViewModel
    @State private var usageViewModel: UsageViewModel
    @State private var setupViewModel: SetupViewModel
    @State private var bootstrappedConfigurationKey: String?
#if os(macOS)
    @State private var menuBarManager: MenuBarManager
#endif

    init() {
        let appState = AppState()
        let sessionViewModel = SessionViewModel(appState: appState)
        let chatViewModel = ChatViewModel(appState: appState, sessionViewModel: sessionViewModel)
        let skillsViewModel = SkillsViewModel(appState: appState)
        let fleetViewModel = FleetViewModel(appState: appState)
        let experimentsViewModel = ExperimentsViewModel(appState: appState)
        let gitViewModel = GitViewModel(appState: appState)
        let settingsViewModel = SettingsViewModel(appState: appState)
        let permissionsViewModel = PermissionsViewModel(appState: appState)
        let telemetryViewModel = TelemetryViewModel(appState: appState)
        let synthesisViewModel = SynthesisViewModel(appState: appState)
        let usageViewModel = UsageViewModel(appState: appState)
        let setupViewModel = SetupViewModel(appState: appState)

        _appState = State(initialValue: appState)
        _sessionViewModel = State(initialValue: sessionViewModel)
        _chatViewModel = State(initialValue: chatViewModel)
        _skillsViewModel = State(initialValue: skillsViewModel)
        _fleetViewModel = State(initialValue: fleetViewModel)
        _experimentsViewModel = State(initialValue: experimentsViewModel)
        _gitViewModel = State(initialValue: gitViewModel)
        _settingsViewModel = State(initialValue: settingsViewModel)
        _permissionsViewModel = State(initialValue: permissionsViewModel)
        _telemetryViewModel = State(initialValue: telemetryViewModel)
        _synthesisViewModel = State(initialValue: synthesisViewModel)
        _usageViewModel = State(initialValue: usageViewModel)
        _setupViewModel = State(initialValue: setupViewModel)
#if os(macOS)
        _menuBarManager = State(initialValue: MenuBarManager(appState: appState))
#endif
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
                    settingsViewModel.reloadStoredValues()
#if os(macOS)
                    menuBarManager.updateAppState(appState)
#endif
                    await handleConfigurationChange()
                }
                .task(id: appState.configurationKey + "|polling") {
                    guard appState.showsMainExperience, appState.isConfigured else {
                        return
                    }

                    while !Task.isCancelled {
                        try? await Task.sleep(for: pollingInterval)
                        guard appState.showsMainExperience, appState.isConfigured, appState.connectionStatus == .connected else {
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
                .task(id: appState.configurationKey + "|ripcord") {
                    await appState.refreshRipcordState()

                    guard appState.showsMainExperience, appState.isConfigured else {
                        return
                    }

                    while !Task.isCancelled {
                        try? await Task.sleep(for: ripcordPollingInterval)
                        guard !Task.isCancelled else {
                            break
                        }
                        guard appState.showsMainExperience, appState.isConfigured, appState.connectionStatus == .connected else {
                            continue
                        }

                        await appState.refreshRipcordState()
                    }
                }
                .onChange(of: scenePhase) { _, newPhase in
                    guard newPhase == .active else {
                        return
                    }

                    Task {
                        if appState.showsMainExperience, appState.isConfigured {
                            await refreshForForegroundActivation()
                        } else if appState.rootDestination == .setupWizard {
                            await setupViewModel.prepareCurrentStep()
                        }
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
        switch appState.rootDestination {
        case .main:
#if os(macOS)
            ContentView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                skillsViewModel: skillsViewModel,
                fleetViewModel: fleetViewModel,
                experimentsViewModel: experimentsViewModel,
                gitViewModel: gitViewModel,
                settingsViewModel: settingsViewModel,
                permissionsViewModel: permissionsViewModel,
                telemetryViewModel: telemetryViewModel,
                synthesisViewModel: synthesisViewModel,
                usageViewModel: usageViewModel
            )
#else
            TabRootView(
                appState: appState,
                sessionViewModel: sessionViewModel,
                chatViewModel: chatViewModel,
                skillsViewModel: skillsViewModel,
                fleetViewModel: fleetViewModel,
                experimentsViewModel: experimentsViewModel,
                gitViewModel: gitViewModel,
                settingsViewModel: settingsViewModel,
                permissionsViewModel: permissionsViewModel,
                telemetryViewModel: telemetryViewModel,
                synthesisViewModel: synthesisViewModel,
                usageViewModel: usageViewModel
            )
#endif
        case .setupWizard:
            SetupWizardView(viewModel: setupViewModel, appState: appState)
        case .remoteOnboarding:
            OnboardingView(settingsViewModel: settingsViewModel, appState: appState)
        }
    }

    @ViewBuilder
    private func themedRootView(selectedTheme: AppTheme) -> some View {
#if os(macOS)
        rootViewWithPermissionSheet
#else
        rootViewWithPermissionSheet
            .preferredColorScheme(selectedTheme.colorScheme)
#endif
    }

    private var rootViewWithPermissionSheet: some View {
        rootView.sheet(item: activePermissionPromptBinding) { prompt in
            PermissionPromptSheetView(
                prompt: prompt,
                isSubmitting: chatViewModel.isRespondingToPermissionPrompt,
                errorMessage: chatViewModel.permissionPromptErrorMessage,
                allowAction: {
                    chatViewModel.respondToPermissionPrompt(.allow)
                },
                denyAction: {
                    chatViewModel.respondToPermissionPrompt(.deny)
                },
                allowSessionAction: {
                    chatViewModel.respondToPermissionPrompt(.allowSession)
                }
            )
            .interactiveDismissDisabled(true)
            .fawxOpaqueModalPresentation()
        }
    }

    private var activePermissionPromptBinding: Binding<PermissionPrompt?> {
        Binding(
            get: {
                appState.permissionMode.showsPermissionPrompts
                    ? chatViewModel.activePermissionPrompt
                    : nil
            },
            set: { _ in }
        )
    }

    private var pollingInterval: Duration {
        let defaultSeconds: Double
#if os(iOS)
        defaultSeconds = 60
#else
        defaultSeconds = 30
#endif
        let override = ProcessInfo.processInfo.environment["FAWX_POLL_INTERVAL_SECONDS"]
            .flatMap(Double.init)
            .map { min(max($0, 10), 300) }
        return .seconds(override ?? defaultSeconds)
    }

    private var ripcordPollingInterval: Duration {
        .seconds(5)
    }

    @MainActor
    private func handleConfigurationChange() async {
        if appState.rootDestination == .setupWizard {
            await setupViewModel.prepareCurrentStep()
            return
        }

        guard appState.showsMainExperience else {
            return
        }

        guard bootstrappedConfigurationKey != appState.configurationKey else {
            return
        }

        bootstrappedConfigurationKey = appState.configurationKey
        await refreshMainExperience()
    }

    @MainActor
    private func refreshMainExperience() async {
        await appState.bootstrap()
        await sessionViewModel.refresh()
        await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID, force: true)
    }

    @MainActor
    private func refreshForForegroundActivation() async {
        if bootstrappedConfigurationKey != appState.configurationKey {
            await handleConfigurationChange()
            return
        }

        do {
            _ = try await appState.client.health()
            try await appState.refreshServerState()
            await appState.refreshRipcordState()
            await sessionViewModel.refresh()
            await appState.refreshContext(for: sessionViewModel.selectedSessionID)
            await chatViewModel.loadMessages(for: sessionViewModel.selectedSessionID, force: true)
        } catch {
            await appState.noteRecoverableRequestFailure(error)
        }
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
