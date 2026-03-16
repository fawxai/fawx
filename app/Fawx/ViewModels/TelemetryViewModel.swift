import Foundation
import Observation

@MainActor
@Observable
final class TelemetryViewModel {
    var isEnabled = false
    var categories: [TelemetryCategory] = []
    var isLoading = false
    var isUpdatingMaster = false
    var pendingCategories: Set<String> = []
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var canManageTelemetry: Bool {
        appState.isConfigured
    }

    func refresh() async {
        guard canManageTelemetry else {
            isEnabled = false
            categories = []
            isUpdatingMaster = false
            pendingCategories = []
            errorMessage = nil
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            apply(response: try await appState.client.getTelemetryConsent())
            errorMessage = nil
        } catch {
            if categories.isEmpty {
                isEnabled = false
                categories = []
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func setEnabled(_ enabled: Bool) async {
        guard canManageTelemetry else {
            return
        }
        guard isEnabled != enabled else {
            return
        }

        let previousEnabled = isEnabled
        let previousCategories = categories

        isEnabled = enabled
        if !enabled {
            categories = categories.map { category in
                var category = category
                category.enabled = false
                return category
            }
        }
        isUpdatingMaster = true
        errorMessage = nil

        defer { isUpdatingMaster = false }

        do {
            let response = try await appState.client.patchTelemetryConsent(
                TelemetryConsentPatchRequest(enabled: enabled, categories: nil)
            )
            apply(response: response)
            errorMessage = nil
        } catch {
            isEnabled = previousEnabled
            categories = previousCategories
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func setCategoryEnabled(_ name: String, enabled: Bool) async {
        guard canManageTelemetry, isEnabled else {
            return
        }
        guard let index = categories.firstIndex(where: { $0.name == name }) else {
            return
        }
        guard categories[index].enabled != enabled else {
            return
        }

        let previousCategories = categories

        categories[index].enabled = enabled
        pendingCategories.insert(name)
        errorMessage = nil

        defer { pendingCategories.remove(name) }

        do {
            let response = try await appState.client.patchTelemetryConsent(
                TelemetryConsentPatchRequest(enabled: nil, categories: [name: enabled])
            )
            apply(response: response)
            errorMessage = nil
        } catch {
            categories = previousCategories
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func apply(response: TelemetryConsentResponse) {
        isEnabled = response.enabled
        categories = response.categories
            .map { name, info in
                TelemetryCategory(
                    name: name,
                    enabled: info.enabled,
                    description: info.description
                )
            }
            .sorted { lhs, rhs in
                if lhs.sortOrder != rhs.sortOrder {
                    return lhs.sortOrder < rhs.sortOrder
                }
                return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
            }
    }
}
