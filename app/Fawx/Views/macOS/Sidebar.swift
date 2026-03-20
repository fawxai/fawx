import Observation
import SwiftUI

struct Sidebar: View {
    @Environment(\.colorScheme) private var colorScheme
#if os(macOS)
    @Environment(\.controlActiveState) private var controlActiveState
#endif

    @Bindable var sessionViewModel: SessionViewModel
    let selection: SidebarSelection?

    let streamingSessionID: String?
    let newSessionAction: () -> Void
    let selectSessionAction: (String) -> Void
    let showSkillsAction: () -> Void
    let showFleetAction: () -> Void
    let showExperimentsAction: () -> Void
    let showGitAction: () -> Void
    let openSettingsAction: () -> Void
    let clearSessionAction: (String) -> Void
    let deleteSessionAction: (String) -> Void
    let deleteSessionsAction: ([String]) -> Void

    @State private var pendingClearSession: Session?
    @State private var pendingDeleteSessions: [Session] = []
    @State private var searchText = ""
    @State private var isSelectingSessions = false
    @State private var selectedSessionIDs: Set<String> = []

    var body: some View {
        VStack(spacing: 0) {
            sessionList
            footerContent
        }
        .accessibilityIdentifier("sessionList")
        .alert("Clear this session?", isPresented: pendingClearAlertBinding) {
            Button("Cancel", role: .cancel) {
                pendingClearSession = nil
            }

            Button("Clear", role: .destructive) {
                if let session = pendingClearSession {
                    clearSessionAction(session.id)
                }
                pendingClearSession = nil
            }
        } message: {
            Text("This removes the conversation history but keeps the session.")
        }
        .alert(deleteAlertTitle, isPresented: pendingDeleteAlertBinding) {
            Button("Cancel", role: .cancel) {
                pendingDeleteSessions = []
            }

            Button(deleteButtonTitle, role: .destructive) {
                let sessionIDs = pendingDeleteSessions.map(\.id)
                if sessionIDs.isEmpty == false {
                    if isSelectingSessions {
                        deleteSessionsAction(sessionIDs)
                        cancelSelectionMode()
                    } else if let sessionID = sessionIDs.first {
                        deleteSessionAction(sessionID)
                    }
                }
                pendingDeleteSessions = []
            }
        } message: {
            Text(deleteAlertMessage)
        }
    }

    private var sessionList: some View {
        List {
            newSessionButton
            sessionListContent
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .searchable(text: $searchText, placement: .sidebar, prompt: "Search sessions")
        .scrollContentBackground(.hidden)
        .background(Color.fawxSurface)
    }

    private var newSessionButton: some View {
        Button(action: newSessionAction) {
            Label {
                Text("New Session")
            } icon: {
                if shouldUseActiveLightSessionIconColor {
                    Image(systemName: "plus")
                        .symbolRenderingMode(.monochrome)
                        .foregroundStyle(Color.white)
                } else {
                    Image(systemName: "plus")
                }
            }
                .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
        .tint(.fawxAccent)
        .accessibilityIdentifier("newSessionButton")
        .disabled(isSelectingSessions)
        .listRowBackground(Color.clear)
    }

    private var shouldUseActiveLightSessionIconColor: Bool {
        guard colorScheme == .light, isSelectingSessions == false else {
            return false
        }

#if os(macOS)
        return controlActiveState == .key
#else
        return true
#endif
    }

    @ViewBuilder
    private var sessionListContent: some View {
        if sessionViewModel.isLoading {
            ProgressView("Loading sessions...")
                .foregroundStyle(Color.fawxTextSecondary)
                .listRowBackground(Color.clear)
        } else if let errorMessage = sessionViewModel.errorMessage, sessionViewModel.sessions.isEmpty {
            sessionListPlaceholder(
                title: "Could not load sessions",
                message: errorMessage,
                actionTitle: "Retry"
            ) {
                Task {
                    await sessionViewModel.refresh()
                }
            }
            .listRowBackground(Color.clear)
        } else if sessionViewModel.sessions.isEmpty {
            sessionListPlaceholder(
                title: "No conversations yet",
                message: "Start a new one!",
                actionTitle: "New Session",
                action: newSessionAction
            )
            .listRowBackground(Color.clear)
        } else if filteredGroupedSections.isEmpty {
            sessionListPlaceholder(
                title: "No sessions matching \"\(trimmedSearchText)\"",
                message: "Try a different search term.",
                actionTitle: "Clear Search"
            ) {
                searchText = ""
            }
            .listRowBackground(Color.clear)
        } else {
            ForEach(filteredGroupedSections) { section in
                sessionSection(section)
            }
        }
    }

    private func sessionSection(_ section: SessionSection) -> some View {
        Section(section.title.uppercased()) {
            ForEach(section.sessions) { session in
                sessionRowButton(for: session)
            }
        }
    }

    private func sessionRowButton(for session: Session) -> some View {
        Button {
            handleSessionRowTap(session.id)
        } label: {
            SessionRowView(
                session: session,
                isSelected: rowIsSelected(session.id),
                isStreaming: session.id == streamingSessionID,
                showsSelectionControl: isSelectingSessions,
                isMarkedForBulkAction: selectedSessionIDs.contains(session.id)
            )
        }
        .buttonStyle(.plain)
        .contextMenu {
            sessionContextMenu(for: session)
        }
        .listRowInsets(
            EdgeInsets(
                top: 0,
                leading: FawxSpacing.paddingSM,
                bottom: 0,
                trailing: FawxSpacing.paddingSM
            )
        )
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }

    @ViewBuilder
    private func sessionContextMenu(for session: Session) -> some View {
        if isSelectingSessions {
            Button(
                selectedSessionIDs.contains(session.id)
                    ? "Deselect Session"
                    : "Select Session"
            ) {
                toggleSelection(for: session.id)
            }
        } else {
            Button("Select Multiple…") {
                beginSelectionMode(selecting: session.id)
            }

            Divider()
        }

        Button("Clear History") {
            pendingClearSession = session
        }

        Button("Delete Session", role: .destructive) {
            pendingDeleteSessions = [session]
        }
    }

    @ViewBuilder
    private var footerContent: some View {
        if isSelectingSessions {
            multiSelectActionBar
        } else {
            sidebarFooterButtons
        }
    }

    private var sidebarFooterButtons: some View {
        VStack(spacing: 0) {
            Divider()

            sidebarButton(
                title: "Skills",
                systemImage: "puzzlepiece.extension",
                isSelected: selection == .skills,
                action: showSkillsAction
            )
            sidebarButton(
                title: "Fleet",
                systemImage: "point.3.connected.trianglepath.dotted",
                isSelected: selection == .fleet,
                action: showFleetAction
            )
            sidebarButton(
                title: "Experiments",
                systemImage: "waveform.path.ecg.rectangle",
                isSelected: selection == .experiments,
                action: showExperimentsAction
            )
            sidebarButton(
                title: "Git",
                systemImage: "arrow.trianglehead.branch",
                isSelected: selection == .git,
                action: showGitAction
            )
            sidebarButton(
                title: "Settings",
                systemImage: "gearshape",
                isSelected: selection == .settings,
                action: openSettingsAction
            )
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface)
    }

    private var multiSelectActionBar: some View {
        VStack(spacing: FawxSpacing.paddingSM) {
            Divider()

            HStack(spacing: FawxSpacing.paddingSM) {
                Button("Cancel") {
                    cancelSelectionMode()
                }
                .buttonStyle(.bordered)

                Spacer(minLength: FawxSpacing.paddingSM)

                Text(selectionSummary)
                    .font(FawxTypography.sidebarTitle)
                    .foregroundStyle(Color.fawxText)

                Spacer(minLength: FawxSpacing.paddingSM)

                Button("Select All") {
                    selectAllVisibleSessions()
                }
                .buttonStyle(.bordered)
                .disabled(visibleSessionIDs.isEmpty)

                Button(deleteButtonTitleForSelection) {
                    pendingDeleteSessions = selectedSessions
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxError)
                .disabled(selectedSessionIDs.isEmpty)
            }
        }
        .padding(.horizontal, FawxSpacing.paddingSM)
        .padding(.vertical, FawxSpacing.paddingSM)
        .background(Color.fawxSurface)
    }

    private func sidebarButton(
        title: String,
        systemImage: String,
        isSelected: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: FawxSpacing.paddingSM) {
                Image(systemName: systemImage)
                    .frame(width: 16, alignment: .center)

                Text(title)

                Spacer(minLength: 0)
            }
            .font(FawxTypography.sidebar)
            .foregroundStyle(Color.fawxText)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, FawxSpacing.paddingMD)
            .padding(.vertical, FawxSpacing.paddingSM)
            .background(
                RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                    .fill(isSelected ? Color.fawxAccentSubtle : Color.clear)
            )
            .contentShape(RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius))
        }
        .buttonStyle(.plain)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var pendingClearAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingClearSession != nil },
            set: { if !$0 { pendingClearSession = nil } }
        )
    }

    private var pendingDeleteAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingDeleteSessions.isEmpty == false },
            set: { if !$0 { pendingDeleteSessions = [] } }
        )
    }

    private var filteredGroupedSections: [SessionSection] {
        SessionViewModel.filterSessionSections(sessionViewModel.groupedSections, query: searchText)
    }

    private var trimmedSearchText: String {
        searchText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var selectedSessions: [Session] {
        sessionViewModel.sessions.filter { selectedSessionIDs.contains($0.id) }
    }

    private var visibleSessionIDs: [String] {
        filteredGroupedSections
            .flatMap(\.sessions)
            .map(\.id)
    }

    private var selectionSummary: String {
        let count = selectedSessionIDs.count
        if count == 1 {
            return "1 selected"
        }
        return "\(count) selected"
    }

    private var deleteAlertTitle: String {
        if pendingDeleteSessions.count > 1 {
            return "Delete \(pendingDeleteSessions.count) sessions?"
        }
        return "Delete this session?"
    }

    private var deleteButtonTitle: String {
        if pendingDeleteSessions.count > 1 {
            return "Delete \(pendingDeleteSessions.count)"
        }
        return "Delete"
    }

    private var deleteAlertMessage: String {
        if pendingDeleteSessions.count > 1 {
            return "This permanently deletes the selected sessions from the server."
        }
        return "This permanently deletes the session from the server."
    }

    private var deleteButtonTitleForSelection: String {
        if selectedSessionIDs.count > 1 {
            return "Delete \(selectedSessionIDs.count)"
        }
        return "Delete"
    }

    private func rowIsSelected(_ sessionID: String) -> Bool {
        if isSelectingSessions {
            return selectedSessionIDs.contains(sessionID)
        }
        return selection == .session(sessionID)
    }

    private func handleSessionRowTap(_ sessionID: String) {
        if isSelectingSessions {
            toggleSelection(for: sessionID)
        } else {
            selectSessionAction(sessionID)
        }
    }

    private func beginSelectionMode(selecting sessionID: String) {
        isSelectingSessions = true
        selectedSessionIDs = [sessionID]
    }

    private func cancelSelectionMode() {
        isSelectingSessions = false
        selectedSessionIDs.removeAll()
    }

    private func toggleSelection(for sessionID: String) {
        if selectedSessionIDs.contains(sessionID) {
            selectedSessionIDs.remove(sessionID)
            if selectedSessionIDs.isEmpty {
                cancelSelectionMode()
            }
        } else {
            selectedSessionIDs.insert(sessionID)
        }
    }

    private func selectAllVisibleSessions() {
        isSelectingSessions = true
        selectedSessionIDs = Set(visibleSessionIDs)
    }

    private func sessionListPlaceholder(
        title: String,
        message: String,
        actionTitle: String,
        action: @escaping () -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
            Text(title)
                .font(FawxTypography.sidebarTitle)
                .foregroundStyle(Color.fawxText)

            Text(message)
                .font(FawxTypography.chatBody)
                .foregroundStyle(Color.fawxTextSecondary)
                .fixedSize(horizontal: false, vertical: true)

            Button(actionTitle, action: action)
                .buttonStyle(.bordered)
        }
        .padding(.vertical, FawxSpacing.paddingSM)
    }
}
