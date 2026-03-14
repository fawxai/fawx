import Observation
import SwiftUI

struct SettingsView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel
    @State private var isPresentingModelSelector = false

    var body: some View {
        Form {
            connectionSection
            modelThinkingSection
            authStatusSection
            appearanceSection
        }
        .formStyle(.grouped)
        .padding(FawxSpacing.paddingLG)
        .frame(minWidth: 520, minHeight: 360)
        .task {
            if appState.isConfigured {
                try? await appState.refreshServerState()
            }
        }
    }

    private var connectionSection: some View {
        Section("Connection") {
            LabeledContent("Server URL") {
                Text(settingsViewModel.serverURL.isEmpty ? "Not configured" : settingsViewModel.serverURL)
                    .foregroundStyle(settingsViewModel.serverURL.isEmpty ? Color.fawxTextSecondary : Color.fawxText)
                    .textSelection(.enabled)
            }

            LabeledContent("Paired as") {
                Text(settingsViewModel.pairedDeviceName ?? "Not paired")
                    .foregroundStyle(settingsViewModel.pairedDeviceName == nil ? Color.fawxTextSecondary : Color.fawxText)
            }

            LabeledContent("Status") {
                Text(connectionStatusLabel)
                    .foregroundStyle(Color.fawxTextSecondary)
            }

            HStack {
                Button(settingsViewModel.isTestingConnection ? "Checking..." : "Test Connection") {
                    Task {
                        await settingsViewModel.testConnection()
                    }
                }
                .disabled(settingsViewModel.isTestingConnection || settingsViewModel.serverURL.isEmpty)

                Button("Unpair", role: .destructive) {
                    Task {
                        await settingsViewModel.unpair()
                    }
                }
                .disabled(!settingsViewModel.isPaired)
            }

            if let status = settingsViewModel.testStatusMessage {
                Text(status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
    }

    private var modelThinkingSection: some View {
        Section("Model & Thinking") {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("Server Model")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                        Text(activeModelName)
                            .font(.system(size: 14, weight: .semibold, design: .monospaced))
                            .foregroundStyle(hasActiveModel ? Color.fawxText : Color.fawxTextSecondary)
                            .textSelection(.enabled)
                            .lineLimit(2)

                        if let activeModel = appState.activeModel {
                            Text(modelMetadataSummary(activeModel))
                                .font(FawxTypography.status)
                                .foregroundStyle(Color.fawxTextSecondary)
                        }
                    }

                    Spacer(minLength: FawxSpacing.paddingMD)

                    Button("Choose Model...") {
                        isPresentingModelSelector = true
                    }
                    .disabled(disableServerControls || appState.availableModels.isEmpty)
                    .accessibilityIdentifier("modelPicker")
                }
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                Text("Server Thinking Level")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Picker("Server Thinking Level", selection: Binding(
                    get: { appState.thinkingLevel?.rawValue ?? "" },
                    set: { newValue in
                        guard !newValue.isEmpty else { return }
                        Task {
                            try? await appState.setThinking(ThinkingLevel(rawValue: newValue))
                        }
                    }
                )) {
                    ForEach(ThinkingLevel.phaseOneOptions, id: \.self) { level in
                        Text(level.rawValue.capitalized).tag(level.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .disabled(disableServerControls)
                .accessibilityIdentifier("thinkingPicker")
            }

            if disableServerControls {
                Text("Cannot change model or thinking while a response is streaming.")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
            }
        }
        .sheet(isPresented: $isPresentingModelSelector) {
            NavigationStack {
                ModelSelectionList(
                    appState: appState,
                    disableSelection: disableServerControls,
                    selectModel: { modelID in
                        isPresentingModelSelector = false
                        Task {
                            try? await appState.setModel(modelID)
                        }
                    }
                )
                .navigationTitle("Server Model")
                .frame(minWidth: 500, minHeight: 420)
            }
        }
    }

    private var appearanceSection: some View {
        Section("Appearance") {
            AppearanceSettingsPanel(appState: appState)
        }
    }

    private var authStatusSection: some View {
        Section("Auth Status") {
            AuthStatusList(
                providers: appState.authProviders,
                errorMessage: appState.authProvidersError
            )
        }
    }

    private var disableServerControls: Bool {
        chatViewModel.isStreaming || appState.isUpdatingServerSettings
    }

    private var connectionStatusLabel: String {
        switch appState.connectionStatus {
        case .connected:
            return "Connected"
        case .connecting:
            return "Connecting"
        case .reconnecting:
            return "Reconnecting"
        case .disconnected:
            return "Disconnected"
        }
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
