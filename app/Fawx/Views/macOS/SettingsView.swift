import Observation
import SwiftUI

struct SettingsView: View {
    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var permissionsViewModel: PermissionsViewModel
    @Bindable var telemetryViewModel: TelemetryViewModel
    @Bindable var synthesisViewModel: SynthesisViewModel
    @Bindable var usageViewModel: UsageViewModel
    @State private var isPresentingModelSelector = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
                autosaveNotice
                connectionSection
                serverSection
                pairingSection
                modelThinkingSection
                authStatusSection
                permissionsSection
                telemetrySection
                synthesisSection
                usageSection
                appearanceSection
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, FawxSpacing.paddingXL)
            .padding(.vertical, FawxSpacing.paddingLG)
        }
        .background(Color.fawxBackground.ignoresSafeArea())
        .task {
            if appState.isConfigured {
                await appState.revalidateConnection(allowReconnect: false)
                await appState.refreshSettingsState()
            }
        }
    }

    private var connectionSection: some View {
        settingsSection("Connection") {
            settingsCard {
                settingsValueRow(
                    label: "Mode",
                    value: appState.isRemoteClient ? "Remote Client" : "Local Server",
                    isSecondary: false
                )

                settingsDivider

                settingsValueRow(
                    label: "Configured URL",
                    value: appState.displayedServerURLString.isEmpty ? "Not configured" : appState.displayedServerURLString,
                    isSecondary: appState.displayedServerURLString.isEmpty,
                    allowsSelection: true
                )

                settingsDivider

                settingsValueRow(
                    label: "Paired as",
                    value: appState.isRemoteClient
                        ? (settingsViewModel.pairedDeviceName ?? "Not paired")
                        : "This Mac",
                    isSecondary: settingsViewModel.pairedDeviceName == nil && appState.isRemoteClient
                )

                settingsDivider

                settingsValueRow(
                    label: "Status",
                    value: connectionStatusLabel,
                    isSecondary: true
                )

                HStack(spacing: FawxSpacing.paddingMD) {
                    Button(settingsViewModel.isTestingConnection ? "Checking..." : "Test Connection") {
                        Task {
                            await settingsViewModel.testConnection()
                        }
                    }
                    .disabled(settingsViewModel.isTestingConnection || settingsViewModel.serverURL.isEmpty)

                    if appState.isRemoteClient {
                        Button("Unpair", role: .destructive) {
                            Task {
                                await settingsViewModel.unpair()
                            }
                        }
                        .disabled(!settingsViewModel.isPaired)
                    }

                    Spacer(minLength: 0)
                }
            }

            if let status = settingsViewModel.testStatusMessage {
                Text(status)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(testStatusColor)
            }
        }
    }

    private var serverSection: some View {
        settingsSection("Server") {
            ServerSettingsPanel(
                appState: appState,
                isReadOnly: false
            )
        }
    }

    private var pairingSection: some View {
        settingsSection("iPhone Pairing") {
            PairingSettingsPanel(
                appState: appState,
                settingsViewModel: settingsViewModel,
                isReadOnly: false,
                openScanner: nil
            )
        }
    }

    private var modelThinkingSection: some View {
        settingsSection("Model & Thinking") {
            settingsCard {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                            Text("Server Model")
                                .font(FawxTypography.sidebarTitle)
                                .foregroundStyle(Color.fawxText)

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

                settingsDivider

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
                        ForEach(appState.availableThinkingLevels, id: \.self) { level in
                            Text(level.displayName).tag(level.rawValue)
                        }
                    }
                    .pickerStyle(.segmented)
                    .disabled(disableServerControls || appState.availableThinkingLevels.isEmpty)
                    .accessibilityIdentifier("thinkingPicker")

                    if disableServerControls {
                        Text("Cannot change model or thinking while a response is streaming.")
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                }
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
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") {
                            isPresentingModelSelector = false
                        }
                    }
                }
            }
        }
    }

    private var appearanceSection: some View {
        settingsSection("Appearance") {
            AppearanceSettingsPanel(appState: appState)
        }
    }

    private var authStatusSection: some View {
        settingsSection("Auth Status") {
            AuthStatusList(appState: appState)
        }
    }

    private var permissionsSection: some View {
        settingsSection("Permissions & Safety") {
            PermissionsSettingsPanel(viewModel: permissionsViewModel)
        }
    }

    private var synthesisSection: some View {
        settingsSection("Custom Instructions") {
            SynthesisSettingsPanel(viewModel: synthesisViewModel)
        }
    }

    private var telemetrySection: some View {
        settingsSection("Privacy & Telemetry") {
            TelemetrySettingsPanel(viewModel: telemetryViewModel)
        }
    }

    private var usageSection: some View {
        settingsSection("Usage") {
            UsageSettingsPanel(viewModel: usageViewModel)
        }
    }

    private var disableServerControls: Bool {
        chatViewModel.isStreaming || appState.isUpdatingServerSettings
    }

    private var connectionStatusLabel: String {
        switch appState.connectionStatus {
        case .connected:
            "Connected"
        case .connecting:
            "Connecting"
        case .reconnecting:
            "Reconnecting"
        case .disconnected:
            "Disconnected"
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

    private var activeModelName: String {
        guard let modelID = appState.activeModel?.modelID else {
            return "Unavailable"
        }
        return abbreviateModelName(modelID)
    }

    private var hasActiveModel: Bool {
        appState.activeModel != nil
    }

    private var autosaveNotice: some View {
        settingsCard {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                Image(systemName: "checkmark.circle")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color.fawxSuccess)

                Text("Settings save automatically. Use buttons labeled Save, Update, or Generate when you want to apply a specific action.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func settingsSection<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            content()
        }
    }

    private func settingsCard<Content: View>(
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    private func settingsValueRow(
        label: String,
        value: String,
        isSecondary: Bool = false,
        allowsSelection: Bool = false
    ) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
            Text(label)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Spacer(minLength: FawxSpacing.paddingLG)

            Text(value)
                .font(FawxTypography.chatBody)
                .foregroundStyle(isSecondary ? Color.fawxTextSecondary : Color.fawxText)
                .multilineTextAlignment(.trailing)
                .modifier(SelectableTextModifier(isSelectable: allowsSelection))
        }
    }

    private var settingsDivider: some View {
        Divider()
            .overlay(Color.fawxBorder)
    }
}

private struct SelectableTextModifier: ViewModifier {
    let isSelectable: Bool

    func body(content: Content) -> some View {
        if isSelectable {
            content.textSelection(.enabled)
        } else {
            content
        }
    }
}
