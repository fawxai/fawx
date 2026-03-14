import Observation
import SwiftUI

struct iOSSettingsView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel

    var body: some View {
        NavigationStack {
            List {
                Section("Connection") {
                    LabeledContent("Server URL") {
                        Text(settingsViewModel.serverURL.isEmpty ? "Not configured" : settingsViewModel.serverURL)
                            .foregroundStyle(settingsViewModel.serverURL.isEmpty ? Color.fawxTextSecondary : Color.fawxText)
                    }

                    LabeledContent("Paired as") {
                        Text(settingsViewModel.pairedDeviceName ?? "Not paired")
                            .foregroundStyle(settingsViewModel.pairedDeviceName == nil ? Color.fawxTextSecondary : Color.fawxText)
                    }

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

                Section("Server") {
                    NavigationLink("Model & Thinking") {
                        iOSModelThinkingSettingsView(appState: appState, chatViewModel: chatViewModel)
                    }
                }

                Section("Appearance") {
                    Picker("Theme", selection: Binding(
                        get: { appState.theme },
                        set: { appState.setTheme($0) }
                    )) {
                        ForEach(AppTheme.allCases, id: \.self) { theme in
                            Text(theme.rawValue.capitalized).tag(theme)
                        }
                    }
                }

                if let status = settingsViewModel.testStatusMessage {
                    Section("Status") {
                        Text(status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                }
            }
            .navigationTitle("Settings")
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
                    ForEach(ThinkingLevel.phaseOneOptions, id: \.self) { level in
                        Text(level.rawValue.capitalized).tag(level.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .disabled(disableControls)
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
