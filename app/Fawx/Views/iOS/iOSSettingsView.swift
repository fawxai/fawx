import Observation
import SwiftUI

private enum SettingsRoute: Hashable {
    case server
    case pairing
    case modelThinking
    case authentication
    case permissions
    case telemetry
    case synthesis
    case usage
    case legal(LegalDocument)
}

struct iOSSettingsView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var permissionsViewModel: PermissionsViewModel
    @Bindable var telemetryViewModel: TelemetryViewModel
    @Bindable var synthesisViewModel: SynthesisViewModel
    @Bindable var usageViewModel: UsageViewModel
    let openSessions: () -> Void
    let openSkills: () -> Void
    let openFleet: () -> Void
    let openExperiments: () -> Void
    let openGit: () -> Void
    let isActive: Bool

    @State private var navigationPath: [SettingsRoute] = []
    @State private var isShowingQRScanner = false

    var body: some View {
        NavigationStack(path: $navigationPath) {
            List {
                Section("Connection") {
                    LabeledContent("Server") {
                        Text(appState.displayedServerURLString.isEmpty ? "Not configured" : appState.displayedServerURLString)
                            .foregroundStyle(appState.displayedServerURLString.isEmpty ? Color.fawxTextSecondary : Color.fawxText)
                    }

                    LabeledContent("Mode") {
                        Text(appState.isRemoteClient ? "Remote Client" : "Local Server")
                            .foregroundStyle(Color.fawxText)
                    }

                    LabeledContent("Paired as") {
                        Text(settingsViewModel.pairedDeviceName ?? "Not paired")
                            .foregroundStyle(settingsViewModel.pairedDeviceName == nil ? Color.fawxTextSecondary : Color.fawxText)
                    }

                    LabeledContent("Status") {
                        Text(appState.serverStatusLabel)
                            .foregroundStyle(appState.connectionStatus == .connected ? Color.fawxSuccess : Color.fawxTextSecondary)
                    }

                    Button(settingsViewModel.isTestingConnection ? "Checking..." : "Test Connection") {
                        Task {
                            await settingsViewModel.testConnection()
                        }
                    }
                    .disabled(settingsViewModel.isTestingConnection || settingsViewModel.serverURL.isEmpty)
                }

                Section("Manage") {
                    NavigationLink(value: SettingsRoute.pairing) {
                        Text("Server Connection")
                    }

                    NavigationLink(value: SettingsRoute.server) {
                        Text("Server")
                    }

                    NavigationLink(value: SettingsRoute.modelThinking) {
                        Text("Model & Thinking")
                    }

                    NavigationLink(value: SettingsRoute.authentication) {
                        Text("Authentication")
                    }

                    NavigationLink(value: SettingsRoute.permissions) {
                        Text("Permissions & Safety")
                    }

                    NavigationLink(value: SettingsRoute.telemetry) {
                        Text("Privacy & Telemetry")
                    }

                    NavigationLink(value: SettingsRoute.synthesis) {
                        Text("Custom Instructions")
                    }

                    NavigationLink(value: SettingsRoute.usage) {
                        Text("Usage")
                    }
                }

                Section("Appearance") {
                    AppearanceSettingsPanel(appState: appState)
                }

                Section("Legal") {
                    ForEach(LegalDocument.allCases) { document in
                        NavigationLink(value: SettingsRoute.legal(document)) {
                            Text(document.title)
                        }
                    }
                }

                if let status = settingsViewModel.testStatusMessage {
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
                            showFleet: openFleet,
                            showExperiments: openExperiments,
                            showGit: openGit,
                            showSettings: {}
                        )
                    }
                }
            }
            .navigationDestination(for: SettingsRoute.self) { route in
                switch route {
                case .server:
                    iOSServerSettingsDetail(appState: appState)
                case .pairing:
                    iOSPairingSettingsDetail(
                        settingsViewModel: settingsViewModel,
                        appState: appState,
                        openScanner: {
                            isShowingQRScanner = true
                        }
                    )
                case .modelThinking:
                    iOSModelThinkingSettingsView(appState: appState, chatViewModel: chatViewModel)
                case .authentication:
                    iOSAuthStatusSettingsView(appState: appState)
                case .permissions:
                    iOSPermissionsSettingsView(permissionsViewModel: permissionsViewModel)
                case .telemetry:
                    iOSTelemetrySettingsView(telemetryViewModel: telemetryViewModel)
                case .synthesis:
                    iOSSynthesisSettingsView(synthesisViewModel: synthesisViewModel)
                case .usage:
                    iOSUsageSettingsView(usageViewModel: usageViewModel)
                case .legal(let document):
                    LegalDocumentView(title: document.title, resourceName: document.resourceName)
                }
            }
            .task(id: isActive) {
                guard isActive else {
                    return
                }
                if appState.isConfigured {
                    await appState.revalidateConnection(allowReconnect: false)
                    await appState.refreshSettingsState()
                }
            }
        }
        .sheet(isPresented: $isShowingQRScanner) {
            QRCodeScannerSheet(
                onCancel: {
                    isShowingQRScanner = false
                },
                onCodeScanned: { rawValue in
                    isShowingQRScanner = false
                    Task {
                        await settingsViewModel.applyScannedConnectionLink(rawValue)
                    }
                }
            )
            .fawxOpaqueModalPresentation()
        }
    }

    private var testStatusColor: Color {
        switch settingsViewModel.testStatusKind {
        case .idle:
            .fawxTextSecondary
        case .success:
            .fawxSuccess
        case .warning:
            .fawxWarning
        case .failure:
            .fawxError
        }
    }
}

private struct iOSServerSettingsDetail: View {
    @Bindable var appState: AppState

    var body: some View {
        ScrollView {
            ServerSettingsPanel(
                appState: appState,
                isReadOnly: true
            )
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Server")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private struct iOSPairingSettingsDetail: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    let openScanner: () -> Void

    var body: some View {
        ScrollView {
            PairingSettingsPanel(
                appState: appState,
                settingsViewModel: settingsViewModel,
                isReadOnly: true,
                openScanner: openScanner
            )
            .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Server Connection")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
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
                        Text(displayThinkingLevel(level, modelID: appState.activeModel?.modelID)).tag(level.rawValue)
                    }
                }
                .pickerStyle(.menu)
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
        guard let activeModel = appState.activeModel else {
            return "Unavailable"
        }
        return displayModelName(activeModel)
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
            models: appState.availableModels,
            selectedModelID: appState.activeModel?.modelID,
            favoriteModelIDs: appState.favoriteModelIDs,
            disableSelection: disableControls,
            selectModel: { modelID in
                dismiss()
                Task {
                    try? await appState.setModel(modelID)
                }
            },
            toggleFavorite: appState.toggleFavoriteModel
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
            AuthStatusList(appState: appState)
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
                await appState.refreshSettingsState()
            }
        }
    }
}

private struct iOSPermissionsSettingsView: View {
    @Bindable var permissionsViewModel: PermissionsViewModel

    var body: some View {
        ScrollView {
            PermissionsSettingsPanel(viewModel: permissionsViewModel)
                .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Permissions")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private struct iOSSynthesisSettingsView: View {
    @Bindable var synthesisViewModel: SynthesisViewModel

    var body: some View {
        ScrollView {
            SynthesisSettingsPanel(viewModel: synthesisViewModel)
                .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Instructions")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private struct iOSTelemetrySettingsView: View {
    @Bindable var telemetryViewModel: TelemetryViewModel

    var body: some View {
        ScrollView {
            TelemetrySettingsPanel(viewModel: telemetryViewModel)
                .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Privacy & Telemetry")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}

private struct iOSUsageSettingsView: View {
    @Bindable var usageViewModel: UsageViewModel

    var body: some View {
        ScrollView {
            UsageSettingsPanel(viewModel: usageViewModel)
                .padding(FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .navigationTitle("Usage")
#if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
#endif
    }
}
