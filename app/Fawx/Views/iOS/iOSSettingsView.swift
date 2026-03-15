import Observation
import SwiftUI

private enum SettingsRoute: Hashable {
    case modelThinking
    case authentication
}

struct iOSSettingsView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel
    let openSessions: () -> Void
    let openSkills: () -> Void

    @State private var navigationPath: [SettingsRoute] = []

    var body: some View {
        NavigationStack(path: $navigationPath) {
            List {
                if showsConnectionSection {
                    Section("Connection") {
                        if matchesSettingsSearch(
                            "server url",
                            "server",
                            settingsViewModel.serverURL,
                            "connection"
                        ) {
                            LabeledContent("Server URL") {
                                Text(settingsViewModel.serverURL.isEmpty ? "Not configured" : settingsViewModel.serverURL)
                                    .foregroundStyle(settingsViewModel.serverURL.isEmpty ? Color.fawxTextSecondary : Color.fawxText)
                            }
                        }

                        if matchesSettingsSearch(
                            "paired as",
                            "pairing",
                            "device",
                            settingsViewModel.pairedDeviceName ?? "",
                            "connection"
                        ) {
                            LabeledContent("Paired as") {
                                Text(settingsViewModel.pairedDeviceName ?? "Not paired")
                                    .foregroundStyle(settingsViewModel.pairedDeviceName == nil ? Color.fawxTextSecondary : Color.fawxText)
                            }
                        }

                        if matchesSettingsSearch("test connection", "check connection", "connection") {
                            Button(settingsViewModel.isTestingConnection ? "Checking..." : "Test Connection") {
                                Task {
                                    await settingsViewModel.testConnection()
                                }
                            }
                            .disabled(settingsViewModel.isTestingConnection || settingsViewModel.serverURL.isEmpty)
                        }

                        if matchesSettingsSearch("unpair", "remove pairing", "device") {
                            Button("Unpair", role: .destructive) {
                                Task {
                                    await settingsViewModel.unpair()
                                }
                            }
                            .disabled(!settingsViewModel.isPaired)
                        }
                    }
                }

                if showsServerSection {
                    Section("Server") {
                        if matchesSettingsSearch("model", "thinking", "server model") {
                            NavigationLink(value: SettingsRoute.modelThinking) {
                                Text("Model & Thinking")
                            }
                        }

                        if matchesSettingsSearch("authentication", "auth", "providers") {
                            NavigationLink(value: SettingsRoute.authentication) {
                                Text("Authentication")
                            }
                        }
                    }
                }

                if showsAppearanceSection {
                    Section("Appearance") {
                        AppearanceSettingsPanel(appState: appState)
                    }
                }

                if let status = settingsViewModel.testStatusMessage, showsStatusSection {
                    Section("Status") {
                        Text(status)
                            .foregroundStyle(testStatusColor)
                    }
                }
            }
            .navigationTitle("Settings")
            .iOSInlineNavigationTitle()
            .toolbar {
                if navigationPath.isEmpty {
                    ToolbarItem(placement: .fawxTopLeading) {
                        SectionMenuButton(
                            disabledSection: .settings,
                            showSessions: openSessions,
                            showSkills: openSkills,
                            showSettings: {}
                        )
                    }
                }
            }
            .navigationDestination(for: SettingsRoute.self) { route in
                switch route {
                case .modelThinking:
                    iOSModelThinkingSettingsView(appState: appState, chatViewModel: chatViewModel)
                case .authentication:
                    iOSAuthStatusSettingsView(appState: appState)
                }
            }
            .task {
                if appState.isConfigured {
                    await appState.revalidateConnection(allowReconnect: false)
                }
            }
        }
    }

    private var showsConnectionSection: Bool {
        true
    }

    private var showsServerSection: Bool {
        true
    }

    private var showsAppearanceSection: Bool {
        true
    }

    private var showsStatusSection: Bool {
        true
    }

    private func matchesSettingsSearch(_ values: String...) -> Bool {
        true
    }

    private var testStatusColor: Color {
        switch settingsViewModel.testStatusKind {
        case .idle:
            return .fawxTextSecondary
        case .success:
            return .fawxSuccess
        case .warning:
            return .fawxWarning
        case .failure:
            return .fawxError
        }
    }
}

private struct iOSModelThinkingSettingsView: View {
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel

    var body: some View {
        Form {
            Section("Server Model") {
                NavigationLink {
                    iOSModelSelectionView(
                        appState: appState,
                        disableControls: disableControls
                    )
                } label: {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                        Text("Active Model")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)

                        Text(activeModelName)
                            .font(.system(size: 15, weight: .semibold, design: .monospaced))
                            .foregroundStyle(hasActiveModel ? Color.fawxText : Color.fawxTextSecondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .multilineTextAlignment(.leading)
                            .lineLimit(2)

                        if let activeModel = appState.activeModel {
                            Text(modelMetadataSummary(activeModel))
                                .font(FawxTypography.status)
                                .foregroundStyle(Color.fawxTextSecondary)
                        }
                    }
                }
                .accessibilityIdentifier("modelPicker")
                .disabled(appState.availableModels.isEmpty)
            }

            Section("Thinking") {
                Picker("Thinking", selection: Binding(
                    get: { appState.thinkingLevel?.rawValue ?? "" },
                    set: { newValue in
                        guard !newValue.isEmpty else { return }
                        Task {
                            try? await appState.setThinking(ThinkingLevel(rawValue: newValue))
                        }
                    }
                )) {
                    ForEach(appState.availableThinkingLevels, id: \.self) { level in
                        Text(level.displayName).tag(level.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .disabled(disableControls || appState.availableThinkingLevels.isEmpty)
                .accessibilityIdentifier("thinkingPicker")

                if disableControls {
                    Text("Cannot change model or thinking while a response is streaming.")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }
            }
        }
        .navigationTitle("Model & Thinking")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }

    private var disableControls: Bool {
        chatViewModel.isStreaming || appState.isUpdatingServerSettings
    }

    private var activeModelName: String {
        guard let modelID = appState.activeModel?.modelID else {
            return "Unavailable"
        }
        return abbreviateModelName(modelID)
    }

    private var hasActiveModel: Bool {
        appState.activeModel != nil
    }
}

private struct iOSModelSelectionView: View {
    @Environment(\.dismiss) private var dismiss

    @Bindable var appState: AppState
    let disableControls: Bool

    var body: some View {
        ModelSelectionList(
            appState: appState,
            disableSelection: disableControls,
            selectModel: { modelID in
                dismiss()
                Task {
                    try? await appState.setModel(modelID)
                }
            }
        )
        .navigationTitle("Server Model")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private struct iOSAuthStatusSettingsView: View {
    @Bindable var appState: AppState

    var body: some View {
        ScrollView {
            AuthStatusList(
                providers: appState.authProviders,
                errorMessage: appState.authProvidersError
            )
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Authentication")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
        .task {
            if appState.isConfigured {
                await appState.revalidateConnection(allowReconnect: false)
            }
        }
    }
}
