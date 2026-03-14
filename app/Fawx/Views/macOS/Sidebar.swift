import Observation
import SwiftUI

struct Sidebar: View {
    @Bindable var sessionViewModel: SessionViewModel

    let streamingSessionID: String?
    let newSessionAction: () -> Void
    let clearSessionAction: (String) -> Void
    let deleteSessionAction: (String) -> Void

    @State private var pendingClearSession: Session?
    @State private var pendingDeleteSession: Session?

    var body: some View {
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
            } else {
                ForEach(sessionViewModel.groupedSections) { section in
                    Section(section.title.uppercased()) {
                        ForEach(section.sessions) { session in
                            Button {
                                sessionViewModel.select(session.id)
                            } label: {
                                SessionRowView(
                                    session: session,
                                    isSelected: session.id == sessionViewModel.selectedSessionID,
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
        .scrollContentBackground(.hidden)
        .background(Color.fawxSurface)
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
            .fill(session.id == sessionViewModel.selectedSessionID ? Color.fawxAccentSubtle : Color.clear)
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
}
