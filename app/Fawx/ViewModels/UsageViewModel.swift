import Foundation
import Observation

@MainActor
@Observable
final class UsageViewModel {
    var usage: UsageResponse?
    var isLoading = false
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    func refresh() async {
        guard appState.isConfigured else {
            usage = nil
            errorMessage = nil
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            usage = try await appState.client.getUsage()
            errorMessage = nil
        } catch {
            if usage == nil {
                usage = nil
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }
}
