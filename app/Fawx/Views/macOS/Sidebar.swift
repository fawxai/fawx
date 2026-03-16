import Observation
import SwiftUI

struct Sidebar: View {
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

    @State private var pendingClearSession: Session?
    @State private var pendingDeleteSession: Session?

    var body: some View {
        VStack(spacing: 0) {
            List {
                Button(action: newSessionAction) {
                    Label("New Session", systemImage: "plus")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(.fawxAccent)
                .accessibilityIdentifier("newSessionButton")
                .listRowBackground(Color.clear)

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
                } else {
                    ForEach(sessionViewModel.groupedSections) { section in
                        Section(section.title.uppercased()) {
                            ForEach(section.sessions) { session in
                                Button {
                                    selectSessionAction(session.id)
                                } label: {
                                    SessionRowView(
                                        session: session,
                                        isSelected: selection == .session(session.id),
                                        isStreaming: session.id == streamingSessionID
                                    )
                                }
                                .buttonStyle(.plain)
                                .contextMenu {
                                    Button("Clear History") {
                                        pendingClearSession = session
                                    }

                                    Button("Delete Session", role: .destructive) {
                                        pendingDeleteSession = session
                                    }
                                }
                                .listRowBackground(rowBackground(for: session))
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .scrollContentBackground(.hidden)
            .background(Color.fawxSurface)

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
            .padding(.horizontal, FawxSpacing.paddingSM)
            .padding(.vertical, FawxSpacing.paddingSM)
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
        .alert("Delete this session?", isPresented: pendingDeleteAlertBinding) {
            Button("Cancel", role: .cancel) {
                pendingDeleteSession = nil
            }

            Button("Delete", role: .destructive) {
                if let session = pendingDeleteSession {
                    deleteSessionAction(session.id)
                }
                pendingDeleteSession = nil
            }
        } message: {
            Text("This permanently deletes the session from the server.")
        }
    }

    private func rowBackground(for session: Session) -> some View {
        RoundedRectangle(cornerRadius: FawxSpacing.cornerRadiusSM)
            .fill(selection == .session(session.id) ? Color.fawxAccentSubtle : Color.clear)
    }

    private func sidebarButton(
        title: String,
        systemImage: String,
        isSelected: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Label(title, systemImage: systemImage)
                .font(FawxTypography.sidebar)
                .foregroundStyle(Color.fawxText)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, FawxSpacing.paddingMD)
                .padding(.vertical, FawxSpacing.paddingSM)
                .background(
                    RoundedRectangle(cornerRadius: FawxSpacing.cornerRadius)
                        .fill(isSelected ? Color.fawxAccentSubtle : Color.clear)
                )
        }
        .buttonStyle(.plain)
    }

    private var pendingClearAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingClearSession != nil },
            set: { if !$0 { pendingClearSession = nil } }
        )
    }

    private var pendingDeleteAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingDeleteSession != nil },
            set: { if !$0 { pendingDeleteSession = nil } }
        )
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
