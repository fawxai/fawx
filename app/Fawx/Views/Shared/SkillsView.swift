import Observation
import SwiftUI

struct SkillsView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @Bindable var skillsViewModel: SkillsViewModel
    let isActive: Bool
    let showsHeader: Bool
    @State private var selectedSection: SkillsSection = .loadedOnServer
    @State private var searchText = ""

    init(skillsViewModel: SkillsViewModel, isActive: Bool = true, showsHeader: Bool = true) {
        _skillsViewModel = Bindable(skillsViewModel)
        self.isActive = isActive
        self.showsHeader = showsHeader
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingXL) {
                if showsHeader {
                    header
                }

                sectionPicker
                searchField
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(containerPadding)
        }
        .background(Color.fawxBackground)
        .task(id: isActive) {
            guard isActive else {
                return
            }
            await skillsViewModel.refresh()
        }
        .task(id: "\(isActive)|\(selectedSection == .marketplace ? searchText : "__loaded__")") {
            guard isActive, selectedSection == .marketplace else {
                return
            }

            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else {
                return
            }
            await skillsViewModel.searchMarketplace(query: searchText)
        }
        .refreshable {
            await skillsViewModel.refresh()
            if selectedSection == .marketplace {
                await skillsViewModel.searchMarketplace(query: searchText)
            }
        }
        .sheet(
            isPresented: Binding(
                get: { skillsViewModel.editingSkill != nil },
                set: { isPresented in
                    if !isPresented {
                        skillsViewModel.cancelEditingPermissions()
                    }
                }
            )
        ) {
            if let skill = skillsViewModel.editingSkill {
                SkillPermissionsEditor(
                    skill: skill,
                    selectedCapabilities: skillsViewModel.skillPermissionsDraft,
                    errorMessage: skillsViewModel.skillPermissionsErrorMessage,
                    isSaving: skillsViewModel.savingSkillPermissionsName == skill.name,
                    toggleCapability: { capability, enabled in
                        skillsViewModel.setCapability(capability, enabled: enabled)
                    },
                    saveAction: {
                        Task {
                            await skillsViewModel.saveEditingPermissions()
                        }
                    },
                    cancelAction: {
                        skillsViewModel.cancelEditingPermissions()
                    }
                )
                .fawxOpaqueModalPresentation()
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
            Text("Skills")
                .font(FawxTypography.heading1)
                .foregroundStyle(Color.fawxText)

            Text(selectedSection.subtitle)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
        }
    }

    private var sectionPicker: some View {
        Picker("Skill source", selection: $selectedSection) {
            ForEach(SkillsSection.allCases, id: \.self) { section in
                Text(section.title).tag(section)
            }
        }
        .pickerStyle(.segmented)
        .accessibilityLabel("Skill source")
    }

    private var searchField: some View {
        HStack(spacing: FawxSpacing.paddingSM) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(Color.fawxTextSecondary)

            TextField(searchPrompt, text: $searchText)
                .textFieldStyle(.plain)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxText)
                .accessibilityIdentifier("skillsSearchField")

            if !searchText.isEmpty {
                Button {
                    searchText = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Color.fawxTextSecondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, FawxSpacing.paddingMD)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch selectedSection {
        case .loadedOnServer:
            loadedSkillsContent
        case .marketplace:
            MarketplaceView(skillsViewModel: skillsViewModel, searchText: searchText)
        }
    }

    @ViewBuilder
    private var loadedSkillsContent: some View {
        if skillsViewModel.isLoading && skillsViewModel.skills.isEmpty {
            ProgressView("Loading skills...")
                .foregroundStyle(Color.fawxTextSecondary)
                .frame(maxWidth: .infinity, minHeight: 280)
        } else if let errorMessage = skillsViewModel.errorMessage, skillsViewModel.skills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "exclamationmark.triangle",
                title: "Could not load skills",
                message: errorMessage,
                actionTitle: "Try Again",
                action: {
                    Task {
                        await skillsViewModel.refresh()
                    }
                }
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        } else if skillsViewModel.skills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "puzzlepiece.extension",
                title: LoadedSkillsCopy.serverLoaded.emptyTitle,
                message: LoadedSkillsCopy.serverLoaded.emptyMessage
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        } else if filteredSkills.isEmpty {
            SkillsPlaceholderView(
                systemImage: "magnifyingglass",
                title: "No matching skills",
                message: "Try a different search term."
            )
            .frame(maxWidth: .infinity, minHeight: 280)
        } else {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                if !showsHeader {
                    Text(LoadedSkillsCopy.serverLoaded.subtitle)
                        .font(FawxTypography.chatBody)
                        .foregroundStyle(Color.fawxTextSecondary)
                }

                LazyVGrid(columns: gridColumns, spacing: FawxSpacing.paddingMD) {
                    ForEach(filteredSkills) { skill in
                        SkillCardView(
                            skill: skill,
                            isRemoving: skillsViewModel.removingSkillNames.contains(skill.name),
                            isSavingPermissions: skillsViewModel.savingSkillPermissionsName == skill.name,
                            editPermissionsAction: {
                                skillsViewModel.beginEditingPermissions(for: skill)
                            }
                        ) {
                            Task {
                                await skillsViewModel.removeInstalledSkill(named: skill.name)
                            }
                        }
                    }
                }
                .accessibilityIdentifier("skillsGrid")
                .accessibilityElement(children: .contain)
            }
        }
    }

    private var filteredSkills: [SkillSummary] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard query.isEmpty == false else {
            return skillsViewModel.skills
        }

        let normalizedQuery = query.localizedLowercase
        return skillsViewModel.skills.filter { skill in
            let haystacks = [
                skill.name,
                skill.displayDescription ?? "",
                skill.tools.joined(separator: " "),
                skill.capabilities.joined(separator: " "),
            ]

            return haystacks.contains { value in
                value.localizedLowercase.contains(normalizedQuery)
            }
        }
    }

    private var searchPrompt: String {
        switch selectedSection {
        case .loadedOnServer:
            LoadedSkillsCopy.serverLoaded.searchPrompt
        case .marketplace:
            "Search marketplace skills"
        }
    }

    private var gridColumns: [GridItem] {
#if os(macOS)
        return [
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
        ]
#else
        if horizontalSizeClass == .regular {
            return [
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
                GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD),
            ]
        }
        return [GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingMD)]
#endif
    }

    private var containerPadding: CGFloat {
#if os(macOS)
        FawxSpacing.paddingXL
#else
        FawxSpacing.paddingLG
#endif
    }
}

private struct SkillCardView: View {
    let skill: SkillSummary
    let isRemoving: Bool
    let isSavingPermissions: Bool
    let editPermissionsAction: () -> Void
    let removeAction: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingMD) {
            HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(Color.fawxAccentSubtle)
                    .frame(width: 32, height: 32)
                    .overlay {
                        Image(systemName: "puzzlepiece.extension.fill")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color.fawxAccent)
                    }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                    Text(skill.name)
                        .font(FawxTypography.heading2)
                        .foregroundStyle(Color.fawxText)
                        .lineLimit(1)

                    SkillStatusPill(label: "Loaded", tone: .loaded)
                }

                Spacer(minLength: 0)
            }

            Text(skill.displayDescription ?? "\(skill.tools.count) tools available on this server.")
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: FawxSpacing.paddingSM) {
                Label("\(skill.tools.count) tools", systemImage: "wrench.and.screwdriver")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                Spacer(minLength: 0)
            }

            FlowLayout(spacing: FawxSpacing.paddingXS) {
                ForEach(previewTools, id: \.self) { tool in
                    ToolChip(label: tool)
                }

                if remainingToolCount > 0 {
                    ToolChip(label: "+\(remainingToolCount) more")
                }
            }

            VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                Text("Permissions")
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)

                if skill.capabilities.isEmpty {
                    Text("No extra permissions requested.")
                        .font(FawxTypography.status)
                        .foregroundStyle(Color.fawxTextSecondary)
                } else {
                    FlowLayout(spacing: FawxSpacing.paddingXS) {
                        ForEach(skill.capabilities, id: \.self) { capability in
                            PermissionChip(label: humanizedCapability(capability))
                        }
                    }
                }
            }

            HStack {
                Button(isSavingPermissions ? "Saving..." : "Edit Permissions") {
                    editPermissionsAction()
                }
                .buttonStyle(.bordered)
                .disabled(isRemoving || isSavingPermissions)

                Spacer(minLength: 0)

                SkillStatusPill(label: LoadedSkillsCopy.serverLoaded.statusLabel, tone: .loaded)

                Spacer(minLength: 0)

                Button(isRemoving ? "Removing..." : "Remove", role: .destructive) {
                    removeAction()
                }
                .buttonStyle(.bordered)
                .disabled(isRemoving || isSavingPermissions)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(FawxSpacing.paddingLG)
        .background(Color.fawxBackground)
        .overlay(
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("skillCard_\(skill.name)")
    }

    private var previewTools: [String] {
        Array(skill.tools.prefix(4))
    }

    private var remainingToolCount: Int {
        max(skill.tools.count - previewTools.count, 0)
    }

}

struct LoadedSkillsCopy: Equatable {
    let sectionTitle: String
    let subtitle: String
    let searchPrompt: String
    let emptyTitle: String
    let emptyMessage: String
    let statusLabel: String

    static let serverLoaded = Self(
        sectionTitle: "Loaded",
        subtitle: "Loaded on server",
        searchPrompt: "Search loaded skills",
        emptyTitle: "No skills loaded",
        emptyMessage: "Skills appear here only after the running Fawx server reports them via /v1/skills.",
        statusLabel: "Loaded"
    )
}

enum SkillsSection: CaseIterable {
    case loadedOnServer
    case marketplace

    var title: String {
        switch self {
        case .loadedOnServer:
            LoadedSkillsCopy.serverLoaded.sectionTitle
        case .marketplace:
            "Marketplace"
        }
    }

    var subtitle: String {
        switch self {
        case .loadedOnServer:
            LoadedSkillsCopy.serverLoaded.subtitle
        case .marketplace:
            "Signed marketplace skills"
        }
    }
}

private struct SkillStatusPill: View {
    enum Tone {
        case loaded
        case inactive
    }

    let label: String
    let tone: Tone

    var body: some View {
        Text(label)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(tone == .loaded ? Color.fawxSuccess : Color.fawxTextSecondary)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 5)
            .background((tone == .loaded ? Color.fawxSuccess : Color.fawxSurfaceActive).opacity(0.12))
            .clipShape(Capsule())
    }
}

private struct ToolChip: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.system(size: 11, weight: .medium, design: .monospaced))
            .foregroundStyle(Color.fawxTextSecondary)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 6)
            .background(Color.fawxSurface)
            .clipShape(Capsule())
    }
}

private struct PermissionChip: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(Color.fawxWarning)
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, 6)
            .background(Color.fawxWarning.opacity(0.12))
            .clipShape(Capsule())
    }
}

private func humanizedCapability(_ rawValue: String) -> String {
    rawValue
        .replacingOccurrences(of: "_", with: " ")
        .replacingOccurrences(of: "-", with: " ")
        .localizedCapitalized
}

private struct FlowLayout: Layout {
    let spacing: CGFloat

    init(spacing: CGFloat) {
        self.spacing = spacing
    }

    func sizeThatFits(
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) -> CGSize {
        let maxWidth = proposal.width ?? .greatestFiniteMagnitude
        var currentX: CGFloat = 0
        var currentY: CGFloat = 0
        var currentLineHeight: CGFloat = 0
        var requiredWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if currentX + size.width > maxWidth, currentX > 0 {
                currentX = 0
                currentY += currentLineHeight + spacing
                currentLineHeight = 0
            }

            requiredWidth = max(requiredWidth, currentX + size.width)
            currentLineHeight = max(currentLineHeight, size.height)
            currentX += size.width + spacing
        }

        return CGSize(
            width: requiredWidth,
            height: currentY + currentLineHeight
        )
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) {
        var currentX = bounds.minX
        var currentY = bounds.minY
        var currentLineHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if currentX + size.width > bounds.maxX, currentX > bounds.minX {
                currentX = bounds.minX
                currentY += currentLineHeight + spacing
                currentLineHeight = 0
            }

            subview.place(
                at: CGPoint(x: currentX, y: currentY),
                proposal: ProposedViewSize(width: size.width, height: size.height)
            )

            currentX += size.width + spacing
            currentLineHeight = max(currentLineHeight, size.height)
        }
    }
}

struct SkillsPlaceholderView: View {
    let systemImage: String
    let title: String
    let message: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(spacing: FawxSpacing.paddingMD) {
            Image(systemName: systemImage)
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Color.fawxAccent.opacity(0.35))

            Text(title)
                .font(FawxTypography.heading2)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)

            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.bordered)
            }
        }
        .frame(maxWidth: .infinity)
    }
}

private struct SkillPermissionsEditor: View {
    let skill: SkillSummary
    let selectedCapabilities: Set<String>
    let errorMessage: String?
    let isSaving: Bool
    let toggleCapability: (String, Bool) -> Void
    let saveAction: () -> Void
    let cancelAction: () -> Void

    private let editableCapabilities = SkillCapabilityOption.allCases

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                    VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        Text(skill.name)
                            .font(FawxTypography.heading1)
                            .foregroundStyle(Color.fawxText)

                        Text("Choose what this skill is allowed to access. These changes update the installed manifest and require a server restart to affect the running skill.")
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxTextSecondary)
                    }

                    LazyVGrid(columns: capabilityColumns, alignment: .leading, spacing: FawxSpacing.paddingSM) {
                        ForEach(editableCapabilities) { capability in
                            SkillCapabilityRow(
                                option: capability,
                                isEnabled: selectedCapabilities.contains(capability.rawValue)
                            ) { enabled in
                                toggleCapability(capability.rawValue, enabled)
                            }
                        }
                    }

                    if !skill.unsupportedCapabilities.isEmpty {
                        VStack(alignment: .leading, spacing: FawxSpacing.paddingXS) {
                            Text("Unsupported in-app permissions")
                                .font(FawxTypography.sidebarTitle)
                                .foregroundStyle(Color.fawxWarning)

                            Text("This skill also declares advanced permissions that still need CLI editing.")
                                .font(FawxTypography.chatBody)
                                .foregroundStyle(Color.fawxTextSecondary)

                            FlowLayout(spacing: FawxSpacing.paddingXS) {
                                ForEach(skill.unsupportedCapabilities, id: \.self) { capability in
                                    PermissionChip(label: humanizedCapability(capability))
                                }
                            }
                        }
                        .padding(FawxSpacing.paddingMD)
                        .background(Color.fawxWarning.opacity(0.08))
                        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
                    }

                    if let errorMessage, !errorMessage.isEmpty {
                        Text(errorMessage)
                            .font(FawxTypography.chatBody)
                            .foregroundStyle(Color.fawxError)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(FawxSpacing.paddingLG)
            }
            .background(Color.fawxBackground)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancelAction)
                }

                ToolbarItem(placement: .confirmationAction) {
                    Button(isSaving ? "Saving..." : "Save", action: saveAction)
                        .disabled(isSaving)
                }
            }
        }
#if os(iOS)
        .presentationDetents([.medium, .large])
#endif
    }

    private var capabilityColumns: [GridItem] {
#if os(macOS)
        [
            GridItem(.flexible(minimum: 180), spacing: FawxSpacing.paddingSM),
            GridItem(.flexible(minimum: 180), spacing: FawxSpacing.paddingSM),
        ]
#else
        [GridItem(.flexible(minimum: 240), spacing: FawxSpacing.paddingSM)]
#endif
    }
}

private struct SkillCapabilityRow: View {
    let option: SkillCapabilityOption
    let isEnabled: Bool
    let setEnabled: (Bool) -> Void

    var body: some View {
        HStack(alignment: .top, spacing: FawxSpacing.paddingMD) {
            VStack(alignment: .leading, spacing: 4) {
                Text(option.title)
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxText)

                Text(option.description)
                    .font(FawxTypography.status)
                    .foregroundStyle(Color.fawxTextSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 0)

            Toggle(
                "",
                isOn: Binding(
                    get: { isEnabled },
                    set: { newValue in
                        setEnabled(newValue)
                    }
                )
            )
                .labelsHidden()
                .toggleStyle(.switch)
        }
        .frame(maxWidth: .infinity, minHeight: 88, alignment: .topLeading)
        .padding(FawxSpacing.paddingMD)
        .background(Color.fawxSurface)
        .clipShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        .overlay {
            RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                .stroke(Color.fawxBorder, lineWidth: 1)
        }
    }
}

private enum SkillCapabilityOption: String, CaseIterable, Identifiable {
    case network
    case storage
    case notifications
    case sensors
    case phoneActions = "phone_actions"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .network:
            "Network"
        case .storage:
            "Storage"
        case .notifications:
            "Notifications"
        case .sensors:
            "Sensors"
        case .phoneActions:
            "Phone Actions"
        }
    }

    var description: String {
        switch self {
        case .network:
            "Allow outbound web requests and API calls."
        case .storage:
            "Allow persistent local storage owned by the skill."
        case .notifications:
            "Allow posting local notifications."
        case .sensors:
            "Allow reading device sensor data."
        case .phoneActions:
            "Allow privileged phone actions such as calling or messaging."
        }
    }
}
