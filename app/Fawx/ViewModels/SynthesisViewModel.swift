import Foundation
import Observation

@MainActor
@Observable
final class SynthesisViewModel {
    var text = ""
    var maxLength = 4000
    var updatedAt: Int?
    var source = "settings"
    var version: Int?
    var isLoading = false
    var isSaving = false
    var statusKind: ConnectionTestKind = .idle
    var statusMessage: String?

    private let appState: AppState
    private var savedText = ""

    init(appState: AppState) {
        self.appState = appState
    }

    var currentLength: Int {
        text.count
    }

    var isOverLimit: Bool {
        currentLength > maxLength
    }

    var hasChanges: Bool {
        text != savedText
    }

    var canSave: Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return !isLoading && !isSaving && !trimmed.isEmpty && !isOverLimit && hasChanges
    }

    var canClear: Bool {
        !isLoading && !isSaving && (!savedText.isEmpty || !text.isEmpty)
    }

    func refresh() async {
        await loadSynthesis(clearStatus: false)
    }

    func save() async {
        guard canSave else {
            return
        }

        isSaving = true
        defer { isSaving = false }

        do {
            let response = try await appState.client.setSynthesis(text, version: version)
            savedText = response.synthesis
            text = response.synthesis
            updatedAt = response.updatedAt
            version = response.version
            source = "settings"
            statusKind = .success
            statusMessage = "Custom instructions saved."
        } catch {
            if let apiError = error as? APIError, apiError.statusCode == 409 {
                statusKind = .warning
                statusMessage = "Instructions were updated elsewhere. Refreshing..."
                await loadSynthesis(clearStatus: false)
                return
            }

            statusKind = .failure
            statusMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func clear() async {
        guard canClear else {
            return
        }

        isSaving = true
        defer { isSaving = false }

        do {
            let response = try await appState.client.clearSynthesis()
            text = ""
            savedText = ""
            updatedAt = nil
            version = response.version
            source = "settings"
            statusKind = .success
            statusMessage = "Custom instructions cleared."
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func loadSynthesis(clearStatus: Bool) async {
        guard appState.isConfigured else {
            text = ""
            savedText = ""
            maxLength = 4000
            updatedAt = nil
            version = nil
            if clearStatus {
                statusKind = .idle
                statusMessage = nil
            }
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            let response = try await appState.client.getSynthesis()
            text = response.synthesis ?? ""
            savedText = response.synthesis ?? ""
            maxLength = response.maxLength
            updatedAt = response.updatedAt
            source = response.source
            version = response.version
            if clearStatus {
                statusKind = .idle
                statusMessage = nil
            }
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }
}
