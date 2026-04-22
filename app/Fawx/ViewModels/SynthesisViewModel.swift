import Foundation
import Observation

@MainActor
@Observable
final class SynthesisViewModel {
    static let customInstructionsMaxLength = 4000
    static let defaultPersonalityID = "casual"

    var text = ""
    var personalityID = SynthesisViewModel.defaultPersonalityID
    var maxLength = SynthesisViewModel.customInstructionsMaxLength
    var updatedAt: Int?
    var source = "settings"
    var version: Int?
    var isLoading = false
    var isSaving = false
    var statusKind: ConnectionTestKind = .idle
    var statusMessage: String?

    private let appState: AppState
    private var savedText = ""
    private var savedPersonalityID = SynthesisViewModel.defaultPersonalityID

    init(appState: AppState) {
        self.appState = appState
    }

    var currentLength: Int {
        text.count
    }

    var remainingLength: Int {
        maxLength - currentLength
    }

    var isOverLimit: Bool {
        currentLength > maxLength
    }

    var hasChanges: Bool {
        text != savedText || personalityID != savedPersonalityID
    }

    var canSave: Bool {
        !isLoading && !isSaving && !isOverLimit && hasChanges
    }

    var canClear: Bool {
        !isLoading && !isSaving && (!savedText.isEmpty || !text.isEmpty)
    }

    var personalityChoices: [AgentPersonalityChoice] {
        var choices = Self.standardPersonalityChoices
        if choices.contains(where: { $0.id == personalityID }) == false {
            choices.append(
                AgentPersonalityChoice(
                    id: personalityID,
                    title: Self.displayName(forPersonalityID: personalityID),
                    description: "Current personality from your config."
                )
            )
        }
        return choices
    }

    var selectedPersonalityDescription: String {
        personalityChoices.first(where: { $0.id == personalityID })?.description
            ?? "Choose how Fawx should shape its default tone."
    }

    var appAccentColor: AppAccentColor {
        appState.accentColor
    }

    func updateText(_ value: String) {
        text = String(value.prefix(maxLength))
    }

    func refresh() async {
        await loadPreferences(clearStatus: false)
    }

    func save() async {
        guard canSave else {
            return
        }

        isSaving = true
        defer { isSaving = false }

        do {
            let response = try await appState.client.patchConfig(changes: preferencePatch(instructions: text))
            savedText = text
            savedPersonalityID = personalityID
            // Config PATCH is intentionally last-write-wins and does not return the legacy
            // synthesis version/updatedAt metadata; this timestamp is UI-only feedback.
            updatedAt = unixTimestamp()
            version = nil
            source = "settings"
            statusKind = .success
            statusMessage = response.restartRequired
                ? "Custom preferences saved. Restart may be required for every surface to pick them up."
                : "Custom preferences saved."
        } catch {
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
            _ = try await appState.client.patchConfig(changes: preferencePatch(instructions: ""))
            text = ""
            savedText = ""
            savedPersonalityID = personalityID
            // Config PATCH is intentionally last-write-wins and does not return the legacy
            // synthesis version/updatedAt metadata; this timestamp is UI-only feedback.
            updatedAt = unixTimestamp()
            version = nil
            source = "settings"
            statusKind = .success
            statusMessage = "Custom instructions cleared."
        } catch {
            statusKind = .failure
            statusMessage = error.localizedDescription
            await appState.noteRecoverableRequestFailure(error)
        }
    }

    private func loadPreferences(clearStatus: Bool) async {
        guard appState.isConfigured else {
            text = ""
            savedText = ""
            personalityID = Self.defaultPersonalityID
            savedPersonalityID = Self.defaultPersonalityID
            maxLength = Self.customInstructionsMaxLength
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
            let config = try await appState.client.serverConfig()
            let loadedPersonality = Self.normalizedPersonalityID(
                config.value(at: ["agent", "personality"])?.stringValue
            )
            let behaviorInstructions = config
                .value(at: ["agent", "behavior", "custom_instructions"])?.stringValue
            let customPersonality = config.value(at: ["agent", "custom_personality"])?.stringValue
            let legacySynthesisInstruction = config.value(at: ["model", "synthesis_instruction"])?.stringValue
            let loadedText = if loadedPersonality == "custom" {
                customPersonality ?? behaviorInstructions ?? legacySynthesisInstruction ?? ""
            } else {
                behaviorInstructions ?? legacySynthesisInstruction ?? ""
            }

            text = loadedText
            savedText = loadedText
            personalityID = loadedPersonality
            savedPersonalityID = loadedPersonality
            maxLength = Self.customInstructionsMaxLength
            updatedAt = nil
            source = "config"
            version = nil
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

    private func preferencePatch(instructions: String) -> JSONValue {
        let isCustomPersonality = personalityID == "custom"
        return .object([
            "agent": .object([
                "custom_personality": .string(isCustomPersonality ? instructions : ""),
                "personality": .string(personalityID),
                "behavior": .object([
                    "custom_instructions": .string(isCustomPersonality ? "" : instructions)
                ])
            ])
        ])
    }

    private func unixTimestamp() -> Int {
        Int(Date().timeIntervalSince1970)
    }

    private static let standardPersonalityChoices = [
        AgentPersonalityChoice(
            id: "casual",
            title: "Casual",
            description: "Friendly and natural, while still concise."
        ),
        AgentPersonalityChoice(
            id: "direct",
            title: "Direct",
            description: "Lead with the answer. Be terse, concrete, and avoid hedging or tool narration."
        ),
        AgentPersonalityChoice(
            id: "professional",
            title: "Professional",
            description: "Polished, structured, and workplace-ready."
        ),
        AgentPersonalityChoice(
            id: "technical",
            title: "Technical",
            description: "Precise, implementation-focused, and comfortable with engineering detail."
        ),
        AgentPersonalityChoice(
            id: "caveman",
            title: "Caveman",
            description: "Raw output mode: minimize human polish, social wrapping, and softened phrasing."
        ),
        AgentPersonalityChoice(
            id: "custom",
            title: "Custom",
            description: "Define the interaction style in the custom instructions field below."
        ),
    ]

    private static func normalizedPersonalityID(_ value: String?) -> String {
        let normalized = value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        guard let normalized, normalized.isEmpty == false else {
            return defaultPersonalityID
        }
        if normalized == "minimal" {
            return "caveman"
        }
        return normalized
    }

    private static func displayName(forPersonalityID value: String) -> String {
        value
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "_", with: " ")
            .capitalized
    }
}

struct AgentPersonalityChoice: Identifiable, Hashable, Sendable {
    let id: String
    let title: String
    let description: String
}
