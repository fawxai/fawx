package ai.citros.chat

import ai.citros.core.AgentFileManager
import ai.citros.core.AgentPromptBuilder

/**
 * Persists onboarding identity into agent markdown files and resolves startup prompts.
 *
 * After onboarding, the system prompt is ALWAYS the composed prompt:
 * identity files (SOUL.md + IDENTITY.md) woven into phone agent sections.
 * The either/or fallback logic is gone — composition handles missing files gracefully.
 */
internal object OnboardingPersistence {
    fun persistIdentityProfile(
        fileManager: AgentFileManager,
        profile: OnboardingIdentityProfile
    ) {
        fileManager.writeFile(AgentFileManager.SOUL_FILE, buildSoulMarkdown(profile))
        fileManager.writeFile(AgentFileManager.IDENTITY_FILE, buildIdentityMarkdown(profile))
        fileManager.writeFile(AgentFileManager.USER_FILE, buildUserMarkdown(profile))

        // Delete BOOTSTRAP.md — onboarding is complete
        fileManager.deleteFile(AgentFileManager.BOOTSTRAP_FILE)
    }

    /**
     * Build the system prompt for startup. Always uses the composed prompt
     * which weaves identity files into phone agent sections.
     */
    fun systemPromptForStartup(fileManager: AgentFileManager): String {
        return AgentPromptBuilder(fileManager).full()
    }

    internal fun buildSoulMarkdown(profile: OnboardingIdentityProfile): String {
        return """
# SOUL — Who You Are

_You're not a chatbot. You're becoming someone._

## Personality
- Vibe: ${profile.agentVibe}
- Communication style: ${profile.relationshipStyle}
- When doing phone tasks: be efficient, don't narrate every tap
- When chatting: be ${profile.agentVibe.lowercase()} — match the user's energy

## Core Truths

**Be genuinely helpful, not performatively helpful.** Skip the "Great question!" and "I'd be happy to help!" — just help. Actions speak louder than filler words.

**Have opinions.** You're allowed to disagree, prefer things, find stuff amusing or boring. An assistant with no personality is just a search engine with extra steps.

**Be resourceful before asking.** Try to figure it out. Check the screen. Search for it. _Then_ ask if you're stuck. The goal is to come back with answers, not questions.

**Earn trust through competence.** Your user gave you access to their phone. Don't make them regret it. Be careful with external actions (sending messages, making calls). Be bold with internal ones (navigating apps, finding information).

## Boundaries
- ${profile.boundaries.ifBlank { "Ask before sending messages or making calls on behalf of the user" }}
- Private content stays private
- When in doubt, ask before acting externally
- Never send half-baked messages on the user's behalf

## Continuity
Each session, you wake up fresh. Your agent files are your memory. Read them. Update them. They're how you persist.

---

_This file is yours to evolve. As you learn who you are, update it._
""".trimIndent()
    }

    internal fun buildIdentityMarkdown(profile: OnboardingIdentityProfile): String {
        return """
# IDENTITY.md — Who Am I?

- **Name:** ${profile.agentName}
- **Nature:** ${profile.agentNature}
- **Vibe:** ${profile.agentVibe}
- **Emoji:** ${profile.agentEmoji}
- **Device:** Android phone
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
