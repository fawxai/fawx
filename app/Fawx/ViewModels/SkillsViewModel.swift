import Foundation
import Observation

@MainActor
@Observable
final class SkillsViewModel {
    var skills: [SkillSummary] = []
    var isLoading = false
    var errorMessage: String?
    var marketplaceSkills: [MarketplaceSkillSummary] = []
    var marketplaceQuery = ""
    var marketplaceAvailable = true
    var marketplaceMessage: String?
    var marketplaceErrorMessage: String?
    var isSearchingMarketplace = false
    var installingSkillNames: Set<String> = []
    var removingSkillNames: Set<String> = []
    var editingSkillName: String?
    var skillPermissionsDraft: Set<String> = []
    var skillPermissionsErrorMessage: String?
    var savingSkillPermissionsName: String?
    var editingSkillSettings: SkillSettingsResponse?
    var loadingSkillSettingsName: String?
    var skillSettingsDraft: [String: String] = [:]
    var clearedSkillSecretKeys: Set<String> = []
    var skillSettingsErrorMessage: String?
    var savingSkillSettingsName: String?

    private let appState: AppState
    @ObservationIgnored private var marketplaceSearchGeneration = 0

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

    func searchMarketplace(query: String) async {
        guard appState.isConfigured else {
            marketplaceQuery = query
            marketplaceSkills = []
            marketplaceAvailable = false
            marketplaceMessage = "Marketplace not yet connected"
            marketplaceErrorMessage = nil
            return
        }

        marketplaceQuery = query
        marketplaceSearchGeneration += 1
        let generation = marketplaceSearchGeneration
        isSearchingMarketplace = true

        do {
            let response = try await appState.client.searchSkills(query: query)
            guard generation == marketplaceSearchGeneration else {
                return
            }

            marketplaceSkills = response.skills.sorted {
                $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
            }
            marketplaceAvailable = response.marketplaceAvailable
            marketplaceMessage = response.message.nonEmpty
                ?? (response.marketplaceAvailable ? nil : "Marketplace not yet connected")
            marketplaceErrorMessage = nil
        } catch {
            guard generation == marketplaceSearchGeneration else {
                return
            }

            marketplaceSkills = []
            if let apiError = error as? APIError, apiError.statusCode == 503 {
                marketplaceAvailable = false
                marketplaceMessage = error.localizedDescription.nonEmpty ?? "Marketplace not yet connected"
                marketplaceErrorMessage = nil
            } else {
                marketplaceAvailable = true
                marketplaceMessage = nil
                marketplaceErrorMessage = error.localizedDescription
                await appState.noteRecoverableRequestFailure(error)
            }
        }

        if generation == marketplaceSearchGeneration {
            isSearchingMarketplace = false
        }
    }

    func installMarketplaceSkill(named name: String) async {
        guard !installingSkillNames.contains(name) else {
            return
        }

        installingSkillNames.insert(name)
        defer { installingSkillNames.remove(name) }

        do {
            try await appState.client.installSkill(name: name)
            await refresh()
            await searchMarketplace(query: marketplaceQuery)
            appState.showToast(message: "Installed \(name).", style: .success)
            marketplaceErrorMessage = nil
        } catch {
            if let apiError = error as? APIError, apiError.statusCode == 503 {
                marketplaceErrorMessage = error.localizedDescription.nonEmpty ?? "Marketplace not yet connected"
                appState.showToast(message: marketplaceErrorMessage ?? "Marketplace not yet connected", style: .warning)
                return
            }

            marketplaceErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func removeInstalledSkill(named name: String) async {
        guard !removingSkillNames.contains(name) else {
            return
        }

        removingSkillNames.insert(name)
        defer { removingSkillNames.remove(name) }

        do {
            try await appState.client.removeSkill(name: name)
            await refresh()
            await searchMarketplace(query: marketplaceQuery)
            appState.showToast(message: "Removed \(name).", style: .info)
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    var editingSkill: SkillSummary? {
        guard let editingSkillName else {
            return nil
        }

        return skills.first { $0.name == editingSkillName }
    }

    func beginEditingPermissions(for skill: SkillSummary) {
        editingSkillName = skill.name
        skillPermissionsDraft = Set(skill.capabilities)
        skillPermissionsErrorMessage = nil
    }

    func cancelEditingPermissions() {
        editingSkillName = nil
        skillPermissionsDraft = []
        skillPermissionsErrorMessage = nil
        savingSkillPermissionsName = nil
    }

    func setCapability(_ capability: String, enabled: Bool) {
        if enabled {
            skillPermissionsDraft.insert(capability)
        } else {
            skillPermissionsDraft.remove(capability)
        }
    }

    func saveEditingPermissions() async {
        guard let skill = editingSkill else {
            return
        }

        savingSkillPermissionsName = skill.name
        skillPermissionsErrorMessage = nil
        defer { savingSkillPermissionsName = nil }

        let orderedCapabilities = SkillSummary.editableCapabilities.filter { capability in
            skillPermissionsDraft.contains(capability)
        }

        do {
            let response = try await appState.client.updateSkillPermissions(
                name: skill.name,
                capabilities: orderedCapabilities
            )

            if let index = skills.firstIndex(where: { $0.name == response.name }) {
                let current = skills[index]
                skills[index] = SkillSummary(
                    name: current.name,
                    description: current.description,
                    tools: current.tools,
                    capabilities: response.capabilities,
                    version: current.version,
                    source: current.source,
                    revisionHash: current.revisionHash,
                    activatedAtMs: current.activatedAtMs,
                    signatureStatus: current.signatureStatus,
                    staleSource: current.staleSource
                )
            }

            appState.showToast(
                message: response.restartRequired
                    ? "Updated \(skill.name) permissions. Restart the server to apply them."
                    : "Updated \(skill.name) permissions.",
                style: response.restartRequired ? .info : .success
            )
            cancelEditingPermissions()
            await refresh()
        } catch {
            skillPermissionsErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func beginEditingSettings(for skill: SkillSummary) async {
        guard loadingSkillSettingsName != skill.name else {
            return
        }

        loadingSkillSettingsName = skill.name
        skillSettingsErrorMessage = nil
        defer { loadingSkillSettingsName = nil }

        do {
            let response = try await appState.client.skillSettings(name: skill.name)
            guard !response.schema.fields.isEmpty else {
                appState.showToast(
                    message: "\(skill.name) does not expose any settings yet.",
                    style: .info
                )
                return
            }

            editingSkillSettings = response
            skillSettingsDraft = response.schema.fields.reduce(into: [:]) { draft, field in
                if field.fieldType == .secret {
                    draft[field.key] = ""
                } else if let stored = response.values.first(where: { $0.key == field.key })?.value {
                    draft[field.key] = stored
                } else if field.fieldType == .boolean {
                    draft[field.key] = "false"
                } else {
                    draft[field.key] = ""
                }
            }
            clearedSkillSecretKeys = []
            skillSettingsErrorMessage = nil
        } catch {
            if let apiError = error as? APIError, apiError.statusCode == 404 {
                appState.showToast(
                    message: "\(skill.name) does not expose any settings yet.",
                    style: .info
                )
                return
            }

            skillSettingsErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func cancelEditingSettings() {
        editingSkillSettings = nil
        skillSettingsDraft = [:]
        clearedSkillSecretKeys = []
        skillSettingsErrorMessage = nil
        savingSkillSettingsName = nil
    }

    func skillSettingDraftValue(for key: String) -> String {
        skillSettingsDraft[key] ?? ""
    }

    func setSkillSettingValue(_ key: String, value: String) {
        skillSettingsDraft[key] = value
        if !value.isEmpty {
            clearedSkillSecretKeys.remove(key)
        }
    }

    func clearSecretSetting(_ key: String) {
        skillSettingsDraft[key] = ""
        clearedSkillSecretKeys.insert(key)
    }

    func saveEditingSettings() async {
        guard let settings = editingSkillSettings else {
            return
        }

        if let localValidationError = validateDraftSettings(settings) {
            skillSettingsErrorMessage = localValidationError
            appState.showToast(message: localValidationError, style: .error)
            return
        }

        savingSkillSettingsName = settings.skillName
        skillSettingsErrorMessage = nil
        defer { savingSkillSettingsName = nil }

        let values = settings.schema.fields.compactMap { field -> SkillSettingInput? in
            switch field.fieldType {
            case .secret:
                if clearedSkillSecretKeys.contains(field.key) {
                    return SkillSettingInput(key: field.key, value: nil)
                }

                let entered = trimmedDraftValue(for: field.key)
                guard let entered, !entered.isEmpty else {
                    return nil
                }
                return SkillSettingInput(key: field.key, value: entered)

            case .boolean:
                let normalized = (trimmedDraftValue(for: field.key) ?? "false").lowercased()
                return SkillSettingInput(key: field.key, value: normalized)

            case .text:
                return SkillSettingInput(
                    key: field.key,
                    value: trimmedDraftValue(for: field.key)
                )

            case .unknown:
                return nil
            }
        }

        do {
            let response = try await appState.client.updateSkillSettings(
                name: settings.skillName,
                values: values
            )
            editingSkillSettings = response.settings
            appState.showToast(message: "Saved \(settings.skillName) settings.", style: .success)
            cancelEditingSettings()
        } catch {
            skillSettingsErrorMessage = error.localizedDescription
            appState.showToast(message: error.localizedDescription, style: .error)
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    func isLoadedOnServer(_ marketplaceSkill: MarketplaceSkillSummary) -> Bool {
        skills.contains(where: { $0.name == marketplaceSkill.name })
    }

    private func validateDraftSettings(_ settings: SkillSettingsResponse) -> String? {
        for field in settings.schema.fields {
            switch field.fieldType {
            case .secret:
                if let error = validateSecretDraftField(field, in: settings) {
                    return error
                }

            case .unknown:
                if let error = validateUnsupportedDraftField(field, in: settings) {
                    return error
                }

            case .text, .boolean:
                if let error = field.validate(skillSettingsDraft[field.key]) {
                    return error
                }
            }
        }

        return nil
    }

    private func trimmedDraftValue(for key: String) -> String? {
        skillSettingsDraft[key]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty
    }

    private func validateSecretDraftField(
        _ field: SkillSettingsField,
        in settings: SkillSettingsResponse
    ) -> String? {
        let entered = trimmedDraftValue(for: field.key)
        if let entered {
            return field.validate(entered)
        }

        if shouldValidateEmptySecretDraft(for: field, in: settings) {
            return field.validate(nil)
        }

        return nil
    }

    private func shouldValidateEmptySecretDraft(
        for field: SkillSettingsField,
        in settings: SkillSettingsResponse
    ) -> Bool {
        let existingSecret = settings.values.first(where: { $0.key == field.key })
        return clearedSkillSecretKeys.contains(field.key) || existingSecret?.isConfigured != true
    }

    private func validateUnsupportedDraftField(
        _ field: SkillSettingsField,
        in settings: SkillSettingsResponse
    ) -> String? {
        guard case .unknown(let rawType) = field.fieldType else {
            return nil
        }

        let existingValue = settings.values.first(where: { $0.key == field.key })
        if field.required && existingValue?.isConfigured != true {
            return "Update Fawx to edit \(field.label) (\(rawType))."
        }

        return nil
    }
}
