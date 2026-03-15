import Foundation
import Observation

@MainActor
@Observable
final class SkillsViewModel {
    var skills: [SkillSummary] = []
    var isLoading = false
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    func refresh() async {
        guard appState.isConfigured else {
            skills = []
            errorMessage = nil
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            let response = try await appState.client.skills()
            skills = response.skills.sorted { lhs, rhs in
                lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
            errorMessage = nil
        } catch {
            if skills.isEmpty {
                skills = []
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }
}
