import Observation
import SwiftUI

struct SessionListView: View {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel

    @State private var navigationPath: [String] = []
    @State private var pendingClearSession: Session?

    var body: some View {
        NavigationStack(path: $navigationPath) {
            List {
                if sessionViewModel.isLoading {
                    ProgressView("Loading sessions...")
                        .foregroundStyle(Color.fawxTextSecondary)
                } else {
                    ForEach(sessionViewModel.groupedSections) { section in
                        Section(section.title) {
                            ForEach(section.sessions) { session in
                                Button {
                                    openSession(session.id)
                                } label: {
                                    SessionRowView(
                                        session: session,
                                        isSelected: session.id == sessionViewModel.selectedSessionID,
                                        isStreaming: session.id == chatViewModel.activeStreamSessionID
                                    )
                                }
                                .buttonStyle(.plain)
                                .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                    Button("Delete", role: .destructive) {
                                        deleteSession(session.id)
                                    }

                                    Button("Clear") {
                                        pendingClearSession = session
                                    }
                                    .tint(.fawxWarning)
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Sessions")
            .accessibilityIdentifier("sessionList")
            .toolbar {
                ToolbarItem(placement: newSessionToolbarPlacement) {
                    Button {
                        createNewSession()
                    } label: {
                        Label("New", systemImage: "square.and.pencil")
                    }
                    .accessibilityIdentifier("newSessionButton")
                }
            }
            .refreshable {
                await sessionViewModel.refresh()
            }
            .navigationDestination(for: String.self) { sessionID in
                ChatDetailView(
                    appState: appState,
                    sessionViewModel: sessionViewModel,
                    chatViewModel: chatViewModel,
                    emptyStateTitle: "Conversation",
                    emptyStateMessage: "Start typing to ask Fawx for help."
                )
                .navigationTitle(sessionTitle(for: sessionID))
#if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
#endif
                .task(id: sessionID) {
                    sessionViewModel.select(sessionID)
                    await chatViewModel.loadMessages(for: sessionID)
                }
            }
        }
        .alert("Clear this session?", isPresented: pendingClearAlertBinding) {
            Button("Cancel", role: .cancel) {
                pendingClearSession = nil
            }

            Button("Clear", role: .destructive) {
                if let session = pendingClearSession {
                    clearSession(session.id)
                }
                pendingClearSession = nil
            }
        } message: {
            Text("This removes the conversation history but keeps the session.")
        }
    }

    private func sessionTitle(for sessionID: String) -> String {
        sessionViewModel.sessions.first(where: { $0.id == sessionID })?.displayTitle ?? "Conversation"
    }

    private func openSession(_ sessionID: String) {
        sessionViewModel.select(sessionID)
        if navigationPath.last != sessionID {
            navigationPath.append(sessionID)
        }
    }

    private func createNewSession() {
        Task { @MainActor in
            if let sessionID = await sessionViewModel.createNewSession() {
                openSession(sessionID)
            }
        }
    }

    private func clearSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
            }

            let didClear = await sessionViewModel.clearSession(id: sessionID)
            if didClear, sessionViewModel.selectedSessionID == sessionID {
                await chatViewModel.loadMessages(for: sessionID, force: true)
            }
        }
    }

    private func deleteSession(_ sessionID: String) {
        Task { @MainActor in
            if chatViewModel.activeStreamSessionID == sessionID {
                chatViewModel.stopStreaming()
            }

            let didDelete = await sessionViewModel.deleteSession(id: sessionID)
            if didDelete, navigationPath.last == sessionID {
                navigationPath.removeLast()
            }
        }
    }

    private var pendingClearAlertBinding: Binding<Bool> {
        Binding(
            get: { pendingClearSession != nil },
            set: { if !$0 { pendingClearSession = nil } }
        )
    }

    private var newSessionToolbarPlacement: ToolbarItemPlacement {
#if os(iOS)
        .topBarTrailing
#else
        .automatic
#endif
    }
}
