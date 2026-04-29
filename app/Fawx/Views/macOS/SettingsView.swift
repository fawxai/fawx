import Observation
import SwiftUI

struct SettingsView: View {
    private enum SettingsSectionID: String, CaseIterable {
        case connection
        case server
        case pairing
        case modelThinking
        case authStatus
        case permissions
        case telemetry
        case synthesis
        case usage
        case threads
        case appearance
        case legal

        var title: String {
            switch self {
            case .connection:
                "Connection"
            case .server:
                "Server"
            case .pairing:
                "iPhone Pairing"
            case .modelThinking:
                "Model & Thinking"
            case .authStatus:
                "Auth Status"
            case .permissions:
                "Permissions & Safety"
            case .telemetry:
                "Privacy & Telemetry"
            case .synthesis:
                "Custom Instructions"
            case .usage:
                "Usage"
            case .threads:
                "Threads"
            case .appearance:
                "Appearance"
            case .legal:
                "Legal"
            }
        }

        var systemImage: String {
            switch self {
            case .connection:
                "network"
            case .server:
                "server.rack"
            case .pairing:
                "iphone.gen3.radiowaves.left.and.right"
            case .modelThinking:
                "brain.head.profile"
            case .authStatus:
                "key"
            case .permissions:
                "shield.lefthalf.filled"
            case .telemetry:
                "hand.raised"
            case .synthesis:
                "text.badge.checkmark"
            case .usage:
                "chart.bar"
            case .threads:
                "text.bubble"
            case .appearance:
                "paintpalette"
            case .legal:
                "doc.text"
            }
        }
    }

    private enum ThreadManagementConfirmation: Identifiable {
        case archive(threadID: String, title: String)
        case clear(sessionID: String, title: String)
        case delete(sessionID: String, title: String)

        var id: String {
            switch self {
            case .archive(let threadID, _):
                "archive-\(threadID)"
            case .clear(let sessionID, _):
                "clear-\(sessionID)"
            case .delete(let sessionID, _):
                "delete-\(sessionID)"
            }
        }
    }

    @Bindable var settingsViewModel: SettingsViewModel
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    @Bindable var permissionsViewModel: PermissionsViewModel
    @Bindable var telemetryViewModel: TelemetryViewModel
    @Bindable var synthesisViewModel: SynthesisViewModel
    @Bindable var usageViewModel: UsageViewModel
    @State private var isShowingModelSelector = false
    @State private var pendingThreadManagementConfirmation: ThreadManagementConfirmation?
    @State private var selectedSettingsSection: SettingsSectionID = .connection

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
            settingsCategorySidebar

            Divider()

            settingsDetailPane
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(FawxSpacing.paddingSM)
        .background(Color.fawxBackground.ignoresSafeArea())
        .task {
            if appState.isConfigured {
                await appState.revalidateConnection(allowReconnect: false)
                await appState.refreshSettingsState()
                await sessionViewModel.loadArchivedThreadsIfNeeded()
            }
        }
        .alert(item: $pendingThreadManagementConfirmation) { confirmation in
            threadManagementAlert(for: confirmation)
        }
    }

    private var settingsCategorySidebar: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text("Settings")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .padding(.horizontal, FawxSpacing.paddingSM)
                    .padding(.top, FawxSpacing.paddingXS)
                    .padding(.bottom, FawxSpacing.paddingXS)

                ForEach(SettingsSectionID.allCases, id: \.rawValue) { section in
                    Button {
                        selectedSettingsSection = section
                    } label: {
                        SettingsCategoryRow(
                            title: section.title,
                            systemImage: section.systemImage,
                            isSelected: selectedSettingsSection == section
                        )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("settingsCategory_\(section.rawValue)")
                }
            }
            .padding(FawxSpacing.paddingXS)
        }
        .frame(width: 190, alignment: .topLeading)
        .frame(maxHeight: .infinity, alignment: .topLeading)
        .fawxSurface(.rail)
    }

    private var settingsDetailPane: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                autosaveNotice
                selectedSettingsContent
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.trailing, FawxSpacing.paddingXS)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private var selectedSettingsContent: some View {
        switch selectedSettingsSection {
        case .connection:
            connectionSection
        case .server:
            serverSection
        case .pairing:
            pairingSection
        case .modelThinking:
            modelThinkingSection
        case .authStatus:
            authStatusSection
        case .permissions:
            permissionsSection
        case .telemetry:
            telemetrySection
        case .synthesis:
            synthesisSection
        case .usage:
            usageSection
        case .threads:
            threadManagementSection
        case .appearance:
            appearanceSection
        case .legal:
            legalSection
        }
    }

    private var connectionSection: some View {
        settingsSection(.connection) {
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
        settingsSection(.server) {
            ServerSettingsPanel(
                appState: appState,
                isReadOnly: false
            )
        }
    }

    private var pairingSection: some View {
        settingsSection(.pairing) {
            PairingSettingsPanel(
                appState: appState,
                settingsViewModel: settingsViewModel,
                isReadOnly: false,
                openScanner: nil
            )
        }
    }

    private var modelThinkingSection: some View {
        settingsSection(.modelThinking) {
            settingsCard {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    HStack(alignment: .center, spacing: FawxSpacing.paddingLG) {
                        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                            Text("Server Model")
                                .font(FawxTypography.sidebarTitle)
                                .foregroundStyle(Color.fawxText)

                            Text(activeModelName)
                                .font(.system(size: 14, weight: .semibold, design: .monospaced))
                                .foregroundStyle(hasActiveModel ? Color.fawxText : Color.fawxTextSecondary)
                                .textSelection(.enabled)
                                .lineLimit(2)

                            modelPickerButton
                        }

                        Spacer(minLength: FawxSpacing.paddingMD)

                        thinkingLevelControl
                    }

                    if let activeModel = appState.activeModel {
                        Text(modelMetadataSummary(activeModel))
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }
                }

                if disableServerControls {
                    Text("Cannot change model or thinking while a response is streaming.")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                if isShowingModelSelector {
                    settingsDivider
                    inlineModelSelector
                }
            }
        }
    }

    private var modelPickerButton: some View {
        Button(isShowingModelSelector ? "Hide Models" : "Choose Model...") {
            withAnimation(.easeInOut(duration: 0.16)) {
                isShowingModelSelector.toggle()
            }
        }
        .disabled(disableServerControls || appState.availableModels.isEmpty)
        .accessibilityIdentifier("modelPicker")
    }

    private var thinkingLevelControl: some View {
        VStack(alignment: .trailing, spacing: FawxSpacing.paddingXS) {
            Text("Thinking")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxTextSecondary)

            FawxDropdownMenu(minWidth: 150) {
                HStack(spacing: FawxSpacing.paddingSM) {
                    Text(displayThinkingLevel(appState.thinkingLevel, modelID: appState.activeModel?.modelID))
                        .lineLimit(1)
                    Image(systemName: "chevron.down")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(Color.fawxTextSecondary)
                }
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxText)
                .padding(.horizontal, FawxSpacing.paddingSM)
                .padding(.vertical, FawxSpacing.paddingXS)
                .background(Color.fawxSurfaceHover)
                .clipShape(RoundedRectangle(cornerRadius: 7))
            } content: { dismiss in
                ForEach(appState.availableThinkingLevels, id: \.self) { level in
                    FawxDropdownActionRow(
                        title: displayThinkingLevel(level, modelID: appState.activeModel?.modelID),
                        isSelected: appState.thinkingLevel == level
                    ) {
                        thinkingLevelBinding.wrappedValue = level.rawValue
                        dismiss()
                    }
                }
            }
            .disabled(disableServerControls || appState.availableThinkingLevels.isEmpty)
            .accessibilityIdentifier("thinkingPicker")
        }
    }

    private var inlineModelSelector: some View {
        ModelSelectionList(
            models: appState.availableModels,
            selectedModelID: appState.activeModel?.modelID,
            favoriteModelIDs: appState.favoriteModelIDs,
            disableSelection: disableServerControls,
            selectModel: { modelID in
                withAnimation(.easeInOut(duration: 0.16)) {
                    isShowingModelSelector = false
                }
                Task {
                    try? await appState.setModel(modelID)
                }
            },
            toggleFavorite: appState.toggleFavoriteModel,
            contentInsets: EdgeInsets(
                top: FawxSpacing.paddingLG,
                leading: FawxSpacing.paddingLG,
                bottom: FawxSpacing.paddingSM,
                trailing: FawxSpacing.paddingLG
            )
        )
        .frame(maxHeight: 420)
        .fawxSurface(.field)
    }

    private var thinkingLevelBinding: Binding<String> {
        Binding(
            get: { appState.thinkingLevel?.rawValue ?? "" },
            set: { newValue in
                guard !newValue.isEmpty else { return }
                Task {
                    try? await appState.setThinking(ThinkingLevel(rawValue: newValue))
                }
            }
        )
    }

    private var appearanceSection: some View {
        settingsSection(.appearance) {
            AppearanceSettingsPanel(appState: appState)
        }
    }

    private var legalSection: some View {
        settingsSection(.legal) {
            LegalSection()
        }
    }

    private var authStatusSection: some View {
        settingsSection(.authStatus) {
            AuthStatusList(appState: appState)
        }
    }

    private var permissionsSection: some View {
        settingsSection(.permissions) {
            PermissionsSettingsPanel(viewModel: permissionsViewModel)
        }
    }

    private var synthesisSection: some View {
        settingsSection(.synthesis) {
            SynthesisSettingsPanel(viewModel: synthesisViewModel)
        }
    }

    private var telemetrySection: some View {
        settingsSection(.telemetry) {
            TelemetrySettingsPanel(viewModel: telemetryViewModel)
        }
    }

    private var usageSection: some View {
        settingsSection(.usage) {
            UsageSettingsPanel(viewModel: usageViewModel)
        }
    }

    private var threadManagementSection: some View {
        settingsSection(.threads) {
            settingsCard {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                    HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                        Text("Archive-first management")
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)

                        Spacer(minLength: 0)

                        if sessionViewModel.isLoadingArchivedThreads {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    Text("Archive threads from the sidebar. Restore or permanently delete them here when needed.")
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                settingsDivider

                VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                    Text("Active threads")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    if sessionViewModel.activeThreadManagementEntries.isEmpty {
                        threadEmptyState("No active threads right now.")
                    } else {
                        ForEach(sessionViewModel.activeThreadManagementEntries) { entry in
                            ThreadManagementRow(
                                title: sessionViewModel.threadDisplayTitle(entry.thread),
                                subtitle: sessionViewModel.threadContextLabel(
                                    entry.thread,
                                    includeWorkspace: true
                                ) ?? entry.workspace?.path,
                                timestamp: relativeTimestampString(entry.thread.updatedAt),
                                statusText: entry.worktree.map { $0.label.nonEmpty ?? $0.branch },
                                primaryActionTitle: "Archive",
                                primaryActionRole: nil,
                                primaryAction: {
                                    requestArchiveThread(
                                        id: entry.thread.id,
                                        title: sessionViewModel.threadDisplayTitle(entry.thread)
                                    )
                                },
                                secondaryActions: [
                                    ThreadManagementAction(title: "Clear history") {
                                        requestClearThread(
                                            sessionID: entry.thread.activeSessionID,
                                            title: sessionViewModel.threadDisplayTitle(entry.thread)
                                        )
                                    },
                                    ThreadManagementAction(title: "Delete", role: .destructive) {
                                        requestDeleteThread(
                                            sessionID: entry.thread.activeSessionID,
                                            title: sessionViewModel.threadDisplayTitle(entry.thread)
                                        )
                                    },
                                ]
                            )
                        }
                    }
                }

                settingsDivider

                VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
                    HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                        Text("Archived threads")
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)

                        Spacer(minLength: 0)

                        Button("Refresh") {
                            Task {
                                await sessionViewModel.refreshArchivedSessions()
                            }
                        }
                        .disabled(sessionViewModel.isLoadingArchivedThreads || appState.isConfigured == false)
                    }

                    if sessionViewModel.archivedSessions.isEmpty {
                        threadEmptyState("No archived threads.")
                    } else {
                        ForEach(sessionViewModel.archivedSessions) { session in
                            ThreadManagementRow(
                                title: sessionViewModel.threadDisplayTitle(for: session),
                                subtitle: session.preview,
                                timestamp: relativeTimestampString(session.archivedAt ?? session.updatedAt),
                                statusText: "Archived",
                                primaryActionTitle: "Restore",
                                primaryActionRole: nil,
                                primaryAction: {
                                    restoreThread(session.id)
                                },
                                secondaryActions: [
                                    ThreadManagementAction(title: "Delete", role: .destructive) {
                                        requestDeleteThread(
                                            sessionID: session.id,
                                            title: sessionViewModel.threadDisplayTitle(for: session)
                                        )
                                    },
                                ]
                            )
                        }
                    }
                }
            }
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
        guard let activeModel = appState.activeModel else {
            return "Unavailable"
        }
        return displayModelName(activeModel)
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
        _ section: SettingsSectionID,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Image(systemName: section.systemImage)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Color.fawxAccent)
                    .frame(width: 18, alignment: .center)

                Text(section.title)
                    .font(FawxTypography.heading2)
                    .foregroundStyle(Color.fawxText)
            }

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
        .padding(FawxSpacing.paddingMD)
        .fawxSurface(.section)
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

    private func threadEmptyState(_ text: String) -> some View {
        Text(text)
            .font(FawxTypography.chatBody)
            .foregroundStyle(Color.fawxTextSecondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func archiveThread(_ threadID: String) {
        Task {
            _ = await sessionViewModel.archiveThread(id: threadID)
        }
    }

    private func requestArchiveThread(id threadID: String, title: String) {
        pendingThreadManagementConfirmation = .archive(threadID: threadID, title: title)
    }

    private func restoreThread(_ sessionID: String) {
        Task {
            _ = await sessionViewModel.unarchiveSession(id: sessionID)
        }
    }

    private func requestClearThread(sessionID: String, title: String) {
        pendingThreadManagementConfirmation = .clear(sessionID: sessionID, title: title)
    }

    private func clearThread(_ sessionID: String) {
        Task {
            _ = await sessionViewModel.clearSession(id: sessionID)
        }
    }

    private func requestDeleteThread(sessionID: String, title: String) {
        pendingThreadManagementConfirmation = .delete(sessionID: sessionID, title: title)
    }

    private func deleteThread(_ sessionID: String) {
        Task {
            _ = await sessionViewModel.deleteSession(id: sessionID)
        }
    }

    private func threadManagementAlert(for confirmation: ThreadManagementConfirmation) -> Alert {
        switch confirmation {
        case .archive(_, let title):
            return Alert(
                title: Text("Archive Thread?"),
                message: Text("\"\(title)\" will leave the active list and can be restored from Settings."),
                primaryButton: .default(Text("Archive")) {
                    performThreadManagementConfirmation(confirmation)
                },
                secondaryButton: .cancel()
            )
        case .clear(_, let title):
            return Alert(
                title: Text("Clear Thread History?"),
                message: Text("This removes the transcript for \"\(title)\" but keeps the thread available in the shell."),
                primaryButton: .destructive(Text("Clear History")) {
                    performThreadManagementConfirmation(confirmation)
                },
                secondaryButton: .cancel()
            )
        case .delete(_, let title):
            return Alert(
                title: Text("Delete Thread?"),
                message: Text("Permanently delete \"\(title)\" and its history. This cannot be undone."),
                primaryButton: .destructive(Text("Delete")) {
                    performThreadManagementConfirmation(confirmation)
                },
                secondaryButton: .cancel()
            )
        }
    }

    private func performThreadManagementConfirmation(_ confirmation: ThreadManagementConfirmation) {
        switch confirmation {
        case .archive(let threadID, _):
            archiveThread(threadID)
        case .clear(let sessionID, _):
            clearThread(sessionID)
        case .delete(let sessionID, _):
            deleteThread(sessionID)
        }
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

private struct SettingsCategoryRow: View {
    let title: String
    let systemImage: String
    let isSelected: Bool

    @State private var isHovering = false

    var body: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: systemImage)
                .font(.system(size: 12, weight: .semibold))
                .frame(width: 16, alignment: .center)

            Text(title)
                .font(FawxTypography.sidebarTitle)
                .lineLimit(1)

            Spacer(minLength: 0)
        }
        .foregroundStyle(isSelected ? Color.fawxText : Color.fawxTextSecondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, FawxSpacing.paddingSM)
        .fawxRowChrome(
            isSelected: isSelected,
            isHovering: isHovering,
            selectionStyle: .accentOnly,
            cornerRadius: FawxSpacing.cornerRadiusSM
        )
        .overlay(alignment: .leading) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(isSelected ? Color.fawxAccent : .clear)
                .frame(width: 3)
                .padding(.vertical, FawxSpacing.paddingXS)
        }
        .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM))
        #if os(macOS)
            .onHover { isHovering = $0 }
        #endif
    }
}

private struct ThreadManagementAction: Identifiable {
    let id = UUID()
    let title: String
    let role: ButtonRole?
    let action: () -> Void

    init(
        title: String,
        role: ButtonRole? = nil,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.role = role
        self.action = action
    }
}

private struct ThreadManagementRow: View {
    let title: String
    let subtitle: String?
    let timestamp: String
    let statusText: String?
    let primaryActionTitle: String
    let primaryActionRole: ButtonRole?
    let primaryAction: () -> Void
    let secondaryActions: [ThreadManagementAction]

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            HStack(alignment: .firstTextBaseline, spacing: FawxSpacing.paddingMD) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)

                    if let subtitle, subtitle.isEmpty == false {
                        Text(subtitle)
                            .font(FawxTypography.status)
                            .foregroundStyle(Color.fawxTextSecondary)
                            .lineLimit(2)
                    }
                }

                Spacer(minLength: FawxSpacing.paddingMD)

                Text(timestamp)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .monospacedDigit()
                    .lineLimit(1)
            }

            HStack(spacing: FawxSpacing.paddingSM) {
                if let statusText, statusText.isEmpty == false {
                    Text(statusText)
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                        .padding(.horizontal, FawxSpacing.paddingSM)
                        .padding(.vertical, FawxSpacing.paddingXS)
                        .background(
                            Capsule()
                                .fill(Color.fawxAccentSubtle.opacity(0.7))
                        )
                }

                Spacer(minLength: 0)

                ForEach(secondaryActions) { action in
                    Button(action.title, role: action.role, action: action.action)
                        .buttonStyle(.bordered)
                }

                Button(primaryActionTitle, role: primaryActionRole, action: primaryAction)
                    .buttonStyle(.borderedProminent)
                    .tint(.fawxAccent)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingMD)
        .fawxSurface(.field)
    }
}
