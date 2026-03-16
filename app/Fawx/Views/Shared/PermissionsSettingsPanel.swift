import Observation
import SwiftUI

struct PermissionsSettingsPanel: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Bindable var viewModel: PermissionsViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
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
                                    isPending: viewModel.pendingActions.contains(permission.action)
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

    private var presetButtons: some View {
        ForEach(presetOptions, id: \.rawValue) { option in
            PermissionPresetButton(
                option: option,
                isSelected: viewModel.selectedPreset == option.rawValue,
                isDisabled: viewModel.isApplyingPreset
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

private enum PermissionPresetOption: String, CaseIterable {
    case power
    case cautious
    case experimental
    case custom
    case safe

    var title: String {
        switch self {
        case .power:
            "Power User"
        case .cautious:
            "Cautious"
        case .experimental:
            "Experimental"
        case .custom:
            "Custom"
        case .safe:
            "Safe"
        }
    }

    var subtitle: String {
        switch self {
        case .power:
            "Allows most workspace actions and asks before external or destructive changes."
        case .cautious:
            "Allows read-heavy work and asks before execution, writes, shell, and external changes."
        case .experimental:
            "Allows broad autonomy, including kernel changes, and only asks on the riskiest actions."
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

    var asksOrBlocksSummary: String {
        switch self {
        case .power:
            "Credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification still require approval."
        case .cautious:
            "Execution, writes, git, shell, self-modify, credential changes, installs, network listeners, outbound messaging, deletes, outside-workspace access, and kernel modification require approval."
        case .experimental:
            "Credential changes, installs, network listeners, outbound messaging, deletes, and outside-workspace access still require approval."
        case .custom:
            "Changing any action below keeps this preset in Custom."
        case .safe:
            "High-impact actions are heavily constrained."
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
                    ForEach(["allow", "propose", "deny"], id: \.self) { level in
                        Button(permissionLevelTitle(level)) {
                            setLevel(level)
                        }
                    }
                } label: {
                    PermissionLevelBadge(level: permission.level)
                }
                .accessibilityLabel("\(permission.title) permission level")
            }
        }
        .padding(FawxSpacing.paddingMD)
        .background(permissionLevelColor(permission.level).opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

private struct PermissionPresetSummaryCard: View {
    let option: PermissionPresetOption
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

            Text("Asks or blocks: \(option.asksOrBlocksSummary)")
                .font(FawxTypography.status)
                .foregroundStyle(Color.fawxWarning)

            HStack(spacing: FawxSpacing.paddingSM) {
                PermissionCountPill(title: "Allow", count: counts.allow, color: .fawxSuccess)
                PermissionCountPill(title: "Ask", count: counts.propose, color: .fawxWarning)
                PermissionCountPill(title: "Deny", count: counts.deny, color: .fawxError)
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
    let propose: Int
    let deny: Int

    init(permissions: [PermissionEntry]) {
        allow = permissions.filter { $0.level.lowercased() == "allow" }.count
        propose = permissions.filter { $0.level.lowercased() == "propose" }.count
        deny = permissions.filter { $0.level.lowercased() == "deny" }.count
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

private func permissionLevelTitle(_ level: String) -> String {
    switch level.lowercased() {
    case "allow":
        "Allow"
    case "deny":
        "Deny"
    default:
        "Ask"
    }
}

private func permissionLevelColor(_ level: String) -> Color {
    switch level.lowercased() {
    case "allow":
        .fawxSuccess
    case "deny":
        .fawxError
    default:
        .fawxWarning
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
