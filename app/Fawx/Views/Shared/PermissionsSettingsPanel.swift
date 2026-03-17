import Observation
import SwiftUI

struct PermissionsSettingsPanel: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Bindable var viewModel: PermissionsViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
            securityModeSection
            presetSection

            if viewModel.isLoading && viewModel.permissions.isEmpty {
                ProgressView("Loading permissions...")
                    .frame(maxWidth: .infinity, minHeight: 160)
            } else {
                permissionGroups
            }

            SetupStatusMessageView(
                kind: viewModel.errorMessage == nil ? .idle : .failure,
                message: viewModel.errorMessage
            )
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
        .task {
            await viewModel.refresh()
        }
    }

    private var securityModeSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Text("Security Mode")
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                if viewModel.isApplyingMode {
                    ProgressView()
                        .controlSize(.small)
                }
            }

            Text("Choose whether restricted actions are silently denied or pause to ask for approval.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            if usesCompactModeStack {
                VStack(spacing: FawxSpacing.paddingSM) {
                    modeButtons
                }
            } else {
                HStack(alignment: .top, spacing: FawxSpacing.paddingSM) {
                    modeButtons
                }
            }
        }
    }

    private var presetSection: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            Text("Permission Preset")
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text("Choose a default trust posture, then fine-tune individual actions below.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            if usesCompactPresetGrid {
                LazyVGrid(
                    columns: [
                        GridItem(.flexible(), spacing: FawxSpacing.paddingSM),
                        GridItem(.flexible(), spacing: FawxSpacing.paddingSM)
                    ],
                    spacing: FawxSpacing.paddingSM
                ) {
                    presetButtons
                }
            } else {
                HStack(spacing: FawxSpacing.paddingSM) {
                    presetButtons
                }
            }

            if let selectedPresetOption {
                PermissionPresetSummaryCard(
                    option: selectedPresetOption,
                    mode: viewModel.permissionMode,
                    counts: permissionLevelCounts
                )
            }
        }
    }

    @ViewBuilder
    private var permissionGroups: some View {
        if viewModel.permissions.isEmpty {
            Text("No permissions reported by the server.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        } else {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                ForEach(groupedPermissions, id: \.title) { group in
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text(group.title)
                            .font(FawxTypography.sidebarTitle)
                            .foregroundStyle(Color.fawxText)

                        VStack(spacing: FawxSpacing.paddingSM) {
                            ForEach(group.entries) { permission in
                                PermissionRow(
                                    permission: permission,
                                    isPending: viewModel.pendingActions.contains(permission.action),
                                    isDisabled: viewModel.isApplyingMode || viewModel.isApplyingPreset
                                ) { newLevel in
                                    Task {
                                        await viewModel.setActionLevel(
                                            action: permission.action,
                                            level: newLevel
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    private var presetOptions: [PermissionPresetOption] {
        let active = viewModel.activePreset
        let desiredOrder = ["power", "cautious", "experimental", "custom", "safe"]
        let available = Set(viewModel.availablePresets + [active])

        let ordered = desiredOrder
            .filter { available.contains($0) }
            .compactMap(PermissionPresetOption.init(rawValue:))

        if ordered.isEmpty {
            return [.power, .cautious, .experimental, .custom]
        }

        return ordered
    }

    private var selectedPresetOption: PermissionPresetOption? {
        PermissionPresetOption(rawValue: viewModel.selectedPreset)
            ?? PermissionPresetOption(rawValue: viewModel.activePreset)
    }

    private var permissionLevelCounts: PermissionLevelCounts {
        PermissionLevelCounts(permissions: viewModel.permissions)
    }

    private var usesCompactPresetGrid: Bool {
        horizontalSizeClass == .compact
    }

    private var usesCompactModeStack: Bool {
        horizontalSizeClass == .compact
    }

    private var modeButtons: some View {
        ForEach(PermissionMode.allCases, id: \.self) { mode in
            PermissionModeButton(
                mode: mode,
                isSelected: viewModel.permissionMode == mode,
                isDisabled: viewModel.isApplyingMode || viewModel.isApplyingPreset
            ) {
                Task {
                    await viewModel.setMode(mode)
                }
            }
        }
    }

    private var presetButtons: some View {
        ForEach(presetOptions, id: \.rawValue) { option in
            PermissionPresetButton(
                option: option,
                isSelected: viewModel.selectedPreset == option.rawValue,
                isDisabled: viewModel.isApplyingPreset || viewModel.isApplyingMode
            ) {
                if option.rawValue == "custom" {
                    viewModel.showCustomEditor()
                } else {
                    Task {
                        await viewModel.applyPreset(option.rawValue)
                    }
                }
            }
        }
    }

    private var groupedPermissions: [PermissionGroup] {
        let grouped = Dictionary(grouping: viewModel.permissions) { permission in
            PermissionGroupTitle(titleFor: permission)
        }

        let order = PermissionGroupTitle.allCases
        return order.compactMap { title in
            guard let entries = grouped[title], !entries.isEmpty else {
                return nil
            }
            return PermissionGroup(
                title: title.displayName,
                entries: entries.sorted { lhs, rhs in
                    lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
                }
            )
        }
    }
}

private struct PermissionGroup: Hashable {
    let title: String
    let entries: [PermissionEntry]
}

private enum PermissionGroupTitle: CaseIterable {
    case fileAccess
    case shell
    case network
    case memory
    case other

    var displayName: String {
        switch self {
        case .fileAccess:
            "File Access"
        case .shell:
            "Shell"
        case .network:
            "Network"
        case .memory:
            "Memory"
        case .other:
            "Other"
        }
    }

    init(titleFor permission: PermissionEntry) {
        let haystack = "\(permission.action) \(permission.title)".lowercased()

        if haystack.contains("file") {
            self = .fileAccess
        } else if haystack.contains("shell") || haystack.contains("command") {
            self = .shell
        } else if haystack.contains("web") || haystack.contains("network") || haystack.contains("api") {
            self = .network
        } else if haystack.contains("memory") {
            self = .memory
        } else {
            self = .other
        }
    }
}

private extension PermissionMode {
    var title: String {
        switch self {
        case .capability:
            "Capability"
        case .prompt:
            "Interactive"
        }
    }

    var subtitle: String {
        switch self {
        case .capability:
            "Actions are allowed or silently denied based on your preset. No interruptions."
        case .prompt:
            "Restricted actions pause and ask for your approval before proceeding."
        }
    }

    var symbolName: String {
        switch self {
        case .capability:
            "shield.fill"
        case .prompt:
            "bell.badge.fill"
        }
    }

    var accentColor: Color {
        switch self {
        case .capability:
            .fawxSuccess
        case .prompt:
            .fawxWarning
        }
    }

    var recommendationLabel: String? {
        switch self {
        case .capability:
            "Recommended"
        case .prompt:
            nil
        }
    }
}

private enum PermissionPresetOption: String, CaseIterable {
    case power
    case cautious
    case experimental
    case custom
    case safe

    var title: String {
        switch self {
        case .power:
            "Standard"
        case .cautious:
            "Restricted"
        case .experimental:
            "Open"
        case .custom:
            "Custom"
        case .safe:
            "Safe"
        }
    }

    var subtitle: String {
        switch self {
        case .power:
            "Balanced workspace autonomy with extra protection for external or destructive changes."
        case .cautious:
            "Read-heavy by default with tighter controls around execution, shell, writes, and external changes."
        case .experimental:
            "Broad autonomy, including kernel changes, with only the riskiest actions still restricted."
        case .custom:
            "Tune every action yourself."
        case .safe:
            "Conservative defaults."
        }
    }

    var allowsSummary: String {
        switch self {
        case .power:
            "Read files, search/fetch the web, run code, write files, use git, run shell commands, call tools, and self-modify."
        case .cautious:
            "Read files, search/fetch the web, and call tools without extra approval."
        case .experimental:
            "Most actions run without interruption, including shell, file writes, and kernel modification."
        case .custom:
            "Whatever you select below."
        case .safe:
            "Only the lowest-risk actions."
        }
    }

    func restrictionSummary(for mode: PermissionMode) -> String {
        switch self {
        case .power:
            switch mode {
            case .capability:
                "Credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification are silently denied."
            case .prompt:
                "Credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification still require approval."
            }
        case .cautious:
            switch mode {
            case .capability:
                "Execution, writes, git, shell, self-modify, credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification are silently denied."
            case .prompt:
                "Execution, writes, git, shell, self-modify, credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification require approval."
            }
        case .experimental:
            switch mode {
            case .capability:
                "Credential changes, installs, network listeners, outbound messaging, deletes, and outside-workspace access are silently denied."
            case .prompt:
                "Credential changes, installs, network listeners, outbound messaging, deletes, and outside-workspace access still require approval."
            }
        case .custom:
            "Changing any action below keeps this preset in Custom."
        case .safe:
            switch mode {
            case .capability:
                "High-impact actions are denied by default."
            case .prompt:
                "High-impact actions are heavily constrained."
            }
        }
    }

    var symbolName: String {
        switch self {
        case .power:
            "bolt.fill"
        case .cautious:
            "shield.lefthalf.filled"
        case .experimental:
            "flame.fill"
        case .custom:
            "slider.horizontal.3"
        case .safe:
            "shield"
        }
    }
}

private struct PermissionModeButton: View {
    let mode: PermissionMode
    let isSelected: Bool
    let isDisabled: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                HStack(alignment: .center, spacing: FawxSpacing.paddingSM) {
                    Image(systemName: mode.symbolName)
                        .foregroundStyle(isSelected ? mode.accentColor : Color.fawxTextSecondary)

                    Text(mode.title)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    if let recommendationLabel = mode.recommendationLabel {
                        Text(recommendationLabel)
                            .font(FawxTypography.status)
                            .foregroundStyle(mode.accentColor)
                            .padding(.horizontal, FawxSpacing.paddingSM)
                            .padding(.vertical, FawxSpacing.paddingXS)
                            .background(mode.accentColor.opacity(0.12))
                            .clipShape(Capsule())
                    }

                    Spacer(minLength: 0)

                    Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(isSelected ? mode.accentColor : Color.fawxTextSecondary)
                }

                Text(mode.subtitle)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .multilineTextAlignment(.leading)
            }
            .frame(maxWidth: .infinity, minHeight: 96, alignment: .leading)
            .padding(FawxSpacing.paddingMD)
            .background(isSelected ? mode.accentColor.opacity(0.12) : Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isSelected ? mode.accentColor.opacity(0.35) : Color.fawxBorder, lineWidth: 1)
            }
            .opacity(isDisabled && !isSelected ? 0.55 : 1)
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .accessibilityLabel(mode.title)
    }
}

private struct PermissionPresetButton: View {
    let option: PermissionPresetOption
    let isSelected: Bool
    let isDisabled: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: FawxSpacing.paddingXS) {
                HStack(spacing: FawxSpacing.paddingXS) {
                    Image(systemName: option.symbolName)
                        .foregroundStyle(isSelected ? Color.fawxAccent : Color.fawxTextSecondary)

                    Text(option.title)
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                }
            }
            .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
            .padding(FawxSpacing.paddingMD)
            .background(isSelected ? Color.fawxAccentSubtle : Color.fawxBackground)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isSelected ? Color.fawxAccent.opacity(0.35) : Color.fawxBorder, lineWidth: 1)
            }
            .opacity(isDisabled && !isSelected ? 0.55 : 1)
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .accessibilityLabel(option.title)
    }
}

private struct PermissionRow: View {
    let permission: PermissionEntry
    let isPending: Bool
    let isDisabled: Bool
    let setLevel: (String) -> Void

    var body: some View {
        HStack(spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 2) {
                Text(permission.title)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text(permissionDescription(permission.action))
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Text(permission.action)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .textSelection(.enabled)
            }

            Spacer(minLength: 0)

            if isPending {
                ProgressView()
                    .controlSize(.small)
            } else {
                Menu {
                    ForEach(EditablePermissionLevel.allCases, id: \.self) { level in
                        Button {
                            setLevel(level.rawValue)
                        } label: {
                            if editablePermissionLevel(permission.level) == level.rawValue {
                                Label(level.title, systemImage: "checkmark")
                            } else {
                                Text(level.title)
                            }
                        }
                    }
                } label: {
                    PermissionLevelBadge(level: permission.level)
                }
                .accessibilityLabel("\(permission.title) permission level")
                .disabled(isDisabled)
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(permissionLevelColor(permission.level).opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

private struct PermissionPresetSummaryCard: View {
    let option: PermissionPresetOption
    let mode: PermissionMode
    let counts: PermissionLevelCounts

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text(option.title)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text(option.subtitle)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)

            Text("Allows: \(option.allowsSummary)")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxSuccess)

            Text(option.restrictionSummary(for: mode))
                .font(FawxTypography.status)
                .foregroundStyle(mode == .capability ? Color.fawxError : Color.fawxWarning)

            HStack(spacing: FawxSpacing.paddingSM) {
                PermissionCountPill(title: "Allowed", count: counts.allow, color: .fawxSuccess)
                PermissionCountPill(
                    title: mode == .capability ? "Denied" : "Requires Approval",
                    count: counts.restricted,
                    color: mode == .capability ? .fawxError : .fawxWarning
                )
                PermissionCountPill(title: "Blocked", count: counts.blocked, color: .fawxTextSecondary)
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private struct PermissionCountPill: View {
    let title: String
    let count: Int
    let color: Color

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(color)
                .frame(width: 8, height: 8)

            Text("\(title) \(count)")
                .font(FawxTypography.status)
                .foregroundStyle(color)
        }
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, 7)
        .background(color.opacity(0.12))
        .clipShape(Capsule())
    }
}

private struct PermissionLevelCounts {
    let allow: Int
    let restricted: Int
    let blocked: Int

    init(permissions: [PermissionEntry]) {
        allow = permissions.filter { permissionVisualState($0.level) == .allowed }.count
        restricted = permissions.filter {
            let state = permissionVisualState($0.level)
            return state == .approvalRequired || state == .denied
        }.count
        blocked = permissions.filter { permissionVisualState($0.level) == .blocked }.count
    }
}

private struct PermissionLevelBadge: View {
    let level: String

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(permissionLevelColor(level))
                .frame(width: 8, height: 8)

            Text(permissionLevelTitle(level))
                .font(FawxTypography.status)
                .foregroundStyle(permissionLevelColor(level))
        }
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, 7)
        .background(permissionLevelColor(level).opacity(0.12))
        .clipShape(Capsule())
    }
}

private enum EditablePermissionLevel: String, CaseIterable {
    case allow
    case ask
    case deny

    var title: String {
        switch self {
        case .allow:
            "Allowed"
        case .ask:
            "Requires Approval"
        case .deny:
            "Blocked"
        }
    }
}

private enum PermissionVisualState {
    case allowed
    case approvalRequired
    case denied
    case blocked
}

private func editablePermissionLevel(_ level: String) -> String {
    switch level.lowercased() {
    case "allow":
        "allow"
    case "deny":
        "deny"
    case "ask", "propose", "denied":
        "ask"
    default:
        "ask"
    }
}

private func permissionVisualState(_ level: String) -> PermissionVisualState {
    switch level.lowercased() {
    case "allow":
        .allowed
    case "deny":
        .blocked
    case "denied":
        .denied
    case "ask", "propose":
        .approvalRequired
    default:
        .approvalRequired
    }
}

private func permissionLevelTitle(_ level: String) -> String {
    switch permissionVisualState(level) {
    case .allowed:
        "Allowed"
    case .approvalRequired:
        "Requires Approval"
    case .denied:
        "Denied"
    case .blocked:
        "Blocked"
    }
}

private func permissionLevelColor(_ level: String) -> Color {
    switch permissionVisualState(level) {
    case .allowed:
        .fawxSuccess
    case .approvalRequired:
        .fawxWarning
    case .denied:
        .fawxError
    case .blocked:
        .fawxTextSecondary
    }
}

private func permissionDescription(_ action: String) -> String {
    switch action {
    case "read_any":
        "Read files anywhere the server can access."
    case "web_search":
        "Search the public web."
    case "web_fetch":
        "Fetch web pages or API responses."
    case "code_execute":
        "Run code locally on the server."
    case "file_write":
        "Create or edit files."
    case "git":
        "Run git operations in repos."
    case "shell":
        "Run shell commands."
    case "tool_call":
        "Use installed tools and built-in actions."
    case "self_modify":
        "Modify Fawx-managed code or prompts."
    case "credential_change":
        "Create, replace, or remove secrets and credentials."
    case "system_install":
        "Install packages or system dependencies."
    case "network_listen":
        "Open a listening network port."
    case "outbound_message":
        "Send messages outside the app."
    case "file_delete":
        "Delete files from disk."
    case "outside_workspace":
        "Access files outside the current workspace."
    case "kernel_modify":
        "Change the engine or runtime internals."
    default:
        "Control how Fawx can use this action."
    }
}
