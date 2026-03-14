import Foundation
import Observation

struct SessionSection: Identifiable, Sendable {
    let title: String
    let sessions: [Session]

    var id: String { title }
}

@MainActor
@Observable
final class SessionViewModel {
    var sessions: [Session] = []
    var selectedSessionID: String?
    var isLoading = false
    var isMutatingSession = false
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var selectedSession: Session? {
        sessions.first(where: { $0.id == selectedSessionID })
    }

    var groupedSections: [SessionSection] {
        let calendar = Calendar.current
        let now = Date()
        let groups = Dictionary(grouping: sessions) { session in
            let updatedDate = Date(timeIntervalSince1970: TimeInterval(session.updatedAt))
            if calendar.isDateInToday(updatedDate) {
                return "Today"
            }
            if calendar.isDateInYesterday(updatedDate) {
                return "Yesterday"
            }

            let days = calendar.dateComponents(
                [.day],
                from: calendar.startOfDay(for: updatedDate),
                to: calendar.startOfDay(for: now)
            ).day ?? 0
            return days < 7 ? "Previous 7 Days" : "Older"
        }

        let orderedTitles = ["Today", "Yesterday", "Previous 7 Days", "Older"]
        return orderedTitles.compactMap { title in
            guard let sessions = groups[title], !sessions.isEmpty else {
                return nil
            }
            return SessionSection(title: title, sessions: sessions)
        }
    }

    func refresh() async {
        guard appState.isConfigured else {
            sessions = []
            selectedSessionID = nil
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            let response = try await appState.client.listSessions(limit: 50)
            sessions = response.sessions.sorted { lhs, rhs in
                if lhs.updatedAt == rhs.updatedAt {
                    return lhs.key < rhs.key
                }
                return lhs.updatedAt > rhs.updatedAt
            }
            if let selectedSessionID, !sessions.contains(where: { $0.id == selectedSessionID }) {
                self.selectedSessionID = nil
            }
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func select(_ sessionID: String?) {
        selectedSessionID = sessionID
    }

    func createNewSession() async -> String? {
        guard appState.isConfigured else {
            return nil
        }

        isMutatingSession = true
        defer { isMutatingSession = false }

        do {
            let created = try await appState.client.createSession(model: appState.activeModel?.modelID)
            upsert(created)
            selectedSessionID = created.id
            errorMessage = nil
            return created.id
        } catch {
            errorMessage = error.localizedDescription
            return nil
        }
    }

    func clearSession(id: String) async -> Bool {
        isMutatingSession = true
        defer { isMutatingSession = false }

        do {
            _ = try await appState.client.clearSession(id: id)
            if let index = sessions.firstIndex(where: { $0.id == id }) {
                sessions[index].preview = nil
                sessions[index].messageCount = 0
                sessions[index].updatedAt = Int(Date().timeIntervalSince1970)
            }
            errorMessage = nil
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func deleteSession(id: String) async -> Bool {
        isMutatingSession = true
        defer { isMutatingSession = false }

        do {
            _ = try await appState.client.deleteSession(id: id)
            removeSession(id)
            errorMessage = nil
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func upsert(_ session: Session) {
        if let index = sessions.firstIndex(where: { $0.id == session.id }) {
            sessions[index] = session
        } else {
            sessions.append(session)
        }
        sessions.sort { lhs, rhs in
            if lhs.updatedAt == rhs.updatedAt {
                return lhs.key < rhs.key
            }
            return lhs.updatedAt > rhs.updatedAt
        }
    }

    func removeSession(_ sessionID: String) {
        sessions.removeAll { $0.id == sessionID }
        if selectedSessionID == sessionID {
            selectedSessionID = sessions.first?.id
        }
    }

    func updatePreview(for sessionID: String, text: String, model: String?) {
        guard let index = sessions.firstIndex(where: { $0.id == sessionID }) else {
            return
        }

        sessions[index].applyPreview(text, model: model)
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false {
            sessions[index].messageCount += 1
        }
        sessions.sort { lhs, rhs in
            if lhs.updatedAt == rhs.updatedAt {
                return lhs.key < rhs.key
            }
            return lhs.updatedAt > rhs.updatedAt
        }
    }
}
