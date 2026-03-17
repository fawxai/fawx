import Foundation
import Observation

@MainActor
@Observable
final class PermissionsViewModel {
    var permissions: [PermissionEntry] = []
    var activePreset: String = "power"
    var permissionMode: PermissionMode = .prompt
    var availablePresets: [String] = []
    var isShowingCustomEditor = false
    var isLoading = false
    var isApplyingPreset = false
    var isApplyingMode = false
    var pendingActions: Set<String> = []
    var errorMessage: String?

    private let appState: AppState

    init(appState: AppState) {
        self.appState = appState
    }

    var isBusy: Bool {
        isLoading || isApplyingPreset || isApplyingMode || !pendingActions.isEmpty
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
            permissionMode = .prompt
            availablePresets = []
            isShowingCustomEditor = false
            errorMessage = nil
            appState.permissionMode = .prompt
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
            permissionMode = response.mode
            availablePresets = response.availablePresets.isEmpty
                ? ["power", "cautious", "experimental", "custom"]
                : response.availablePresets.map { $0.lowercased() }
            isShowingCustomEditor = activePreset == "custom"
            errorMessage = nil
            appState.permissionPresetName = permissionPresetLabel(response.preset)
            appState.permissionMode = response.mode
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
                PermissionsPatchRequest(preset: name, mode: nil, changes: nil)
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

    func setMode(_ mode: PermissionMode) async {
        guard !isApplyingMode else {
            return
        }

        guard permissionMode != mode else {
            return
        }

        let previousMode = permissionMode
        permissionMode = mode
        appState.permissionMode = mode
        errorMessage = nil
        isApplyingMode = true
        defer { isApplyingMode = false }

        do {
            _ = try await appState.client.patchPermissions(
                PermissionsPatchRequest(preset: nil, mode: mode, changes: nil)
            )
            await refresh()
        } catch {
            permissionMode = previousMode
            appState.permissionMode = previousMode
            errorMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func setActionLevel(action: String, level: String) async {
        guard let index = permissions.firstIndex(where: { $0.action == action }) else {
            return
        }
        let normalizedLevel = level.lowercased()
        guard editablePermissionLevel(for: permissions[index].level) != normalizedLevel else {
            return
        }

        let previousPermissions = permissions
        let previousPreset = activePreset

        permissions[index] = PermissionEntry(
            action: permissions[index].action,
            level: displayLevel(forRequestedLevel: normalizedLevel),
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
                    mode: nil,
                    changes: [PermissionChange(action: action, level: normalizedLevel)]
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

    private func editablePermissionLevel(for level: String) -> String {
        switch level.lowercased() {
        case "allow":
            "allow"
        case "deny":
            "deny"
        case "denied", "ask", "propose":
            "ask"
        default:
            "ask"
        }
    }

    private func displayLevel(forRequestedLevel level: String) -> String {
        switch level {
        case "ask" where permissionMode == .capability:
            "denied"
        default:
            level
        }
    }
}
