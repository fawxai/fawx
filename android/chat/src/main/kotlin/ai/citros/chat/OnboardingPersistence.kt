package ai.citros.chat

import ai.citros.core.AgentFileManager
import ai.citros.core.AgentPromptBuilder
import ai.citros.core.PhoneAgentPrompts

/**
 * Persists onboarding identity into agent markdown files and resolves startup prompts.
 *
 * Markdown format is intentionally simple bullet sections so prompt assembly remains stable.
 */
internal object OnboardingPersistence {
    fun persistIdentityProfile(
        fileManager: AgentFileManager,
        profile: OnboardingIdentityProfile
    ) {
        fileManager.writeFile(AgentFileManager.SOUL_FILE, buildSoulMarkdown(profile))
        fileManager.writeFile(AgentFileManager.USER_FILE, buildUserMarkdown(profile))
    }

    fun systemPromptForStartup(fileManager: AgentFileManager): String {
        val hasSoul = runCatching { fileManager.readFile(AgentFileManager.SOUL_FILE) }
            .getOrNull()
            ?.isNotBlank() == true
        val hasUser = runCatching { fileManager.readFile(AgentFileManager.USER_FILE) }
            .getOrNull()
            ?.isNotBlank() == true

        if (!hasSoul || !hasUser) {
            return PhoneAgentPrompts.buildSystemPrompt()
        }

        return AgentPromptBuilder(fileManager).full()
    }

    internal fun buildSoulMarkdown(profile: OnboardingIdentityProfile): String {
        return """
# SOUL

## Identity
- Name: ${profile.agentName}
- Nature: ${profile.agentNature}
- Vibe: ${profile.agentVibe}
- Emoji: ${profile.agentEmoji}

## Relationship
- Style: ${profile.relationshipStyle}
""".trimIndent()
    }

    internal fun buildUserMarkdown(profile: OnboardingIdentityProfile): String {
        return """
# USER

## Core
- Name: ${profile.userName}
- Address: ${profile.userAddress}

## Preferences
- Relationship style: ${profile.relationshipStyle}
- Boundaries: ${profile.boundaries}
- Context: ${profile.userContext}
""".trimIndent()
    }
}
