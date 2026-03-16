import Foundation
import Observation

@MainActor
@Observable
final class PermissionsViewModel {
    var permissions: [PermissionEntry] = []
    var activePreset: String = "power"
    var availablePresets: [String] = []
    var isShowingCustomEditor = false
    var isLoading = false
    var isApplyingPreset = false
    var pendingActions: Set<String> = []
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var isBusy: Bool {
        isLoading || isApplyingPreset || !pendingActions.isEmpty
    }

    var selectedPreset: String {
        isShowingCustomEditor ? "custom" : activePreset
    }

    var showsCustomPermissionsEditor: Bool {
        isShowingCustomEditor || activePreset == "custom"
    }

    func refresh() async {
        guard appState.isConfigured else {
            permissions = []
            activePreset = "power"
            availablePresets = []
            isShowingCustomEditor = false
            errorMessage = nil
            return
        }

        guard !isLoading else {
            return
        }

        isLoading = true
        defer { isLoading = false }

        do {
            let response = try await appState.client.getPermissions()
            permissions = response.permissions.sorted {
                $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
            }
            activePreset = response.preset.lowercased()
            availablePresets = response.availablePresets.isEmpty
                ? ["power", "cautious", "experimental", "custom"]
                : response.availablePresets.map { $0.lowercased() }
            isShowingCustomEditor = activePreset == "custom"
            errorMessage = nil
            appState.permissionPresetName = permissionPresetLabel(response.preset)
        } catch {
            if permissions.isEmpty {
                permissions = []
            }
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func applyPreset(_ name: String) async {
        guard !isApplyingPreset else {
            return
        }

        isApplyingPreset = true
        defer { isApplyingPreset = false }

        do {
            let response = try await appState.client.patchPermissions(
                PermissionsPatchRequest(preset: name, changes: nil)
            )
            activePreset = response.preset.lowercased()
            isShowingCustomEditor = activePreset == "custom"
            appState.permissionPresetName = permissionPresetLabel(response.preset)
            errorMessage = nil
            await refresh()
        } catch {
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func setActionLevel(action: String, level: String) async {
        guard let index = permissions.firstIndex(where: { $0.action == action }) else {
            return
        }
        guard permissions[index].level != level else {
            return
        }

        let previousPermissions = permissions
        let previousPreset = activePreset

        permissions[index] = PermissionEntry(
            action: permissions[index].action,
            level: level,
            title: permissions[index].title
        )
        activePreset = "custom"
        isShowingCustomEditor = true
        appState.permissionPresetName = permissionPresetLabel("custom")
        pendingActions.insert(action)
        errorMessage = nil

        defer { pendingActions.remove(action) }

        do {
            let response = try await appState.client.patchPermissions(
                PermissionsPatchRequest(
                    preset: nil,
                    changes: [PermissionChange(action: action, level: level)]
                )
            )
            activePreset = response.preset.lowercased()
            appState.permissionPresetName = permissionPresetLabel(response.preset)
        } catch {
            permissions = previousPermissions
            activePreset = previousPreset
            isShowingCustomEditor = previousPreset == "custom"
            appState.permissionPresetName = permissionPresetLabel(previousPreset)
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func showCustomEditor() {
        isShowingCustomEditor = true
        errorMessage = nil
    }
}
