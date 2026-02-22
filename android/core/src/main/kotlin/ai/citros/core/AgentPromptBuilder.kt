package ai.citros.core

import android.util.Log

/**
 * Builds composed system prompts by weaving agent identity files into
 * the phone agent's modular prompt sections.
 *
 * Identity files (SOUL.md, IDENTITY.md, USER.md, MEMORY.md) SUPPLEMENT
 * the phone prompt — they replace the generic identity section but never
 * displace tools, strategy, recovery, communication, or rules.
 */
class AgentPromptBuilder(
    private val fileManager: AgentFileManager
) {
    /**
     * Build the full composed system prompt.
     *
     * Reads identity files and passes them to [PhoneAgentPrompts.buildSystemPrompt]
     * which weaves them into the correct positions among phone-specific sections.
     */
    fun full(phoneControlAvailable: Boolean = true, modelName: String? = null, sensorContext: SensorContext? = null): String {
        val soulContent = readFileOrNull(AgentFileManager.SOUL_FILE)
        val identityContent = readFileOrNull(AgentFileManager.IDENTITY_FILE)
        val userContent = readFileOrNull(AgentFileManager.USER_FILE)
        val agentsContent = readFileOrNull(AgentFileManager.AGENTS_FILE)
        val securityContent = readFileOrNull(AgentFileManager.SECURITY_FILE)
        val memoryContent = fileManager.readMemoryForPrompt()

        // Compose identity section from SOUL.md + IDENTITY.md
        val composedIdentity = buildIdentitySection(soulContent, identityContent, memoryContent != null)

        return PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            identityContent = composedIdentity,
            userContent = userContent,
            agentsContent = agentsContent,
            memoryContent = memoryContent,
            securityContent = securityContent,
            sensorContext = sensorContext
        )
    }

    /**
     * Build the trimmed action-loop prompt.
     * Uses the action prompt with identity reminder.
     */
    fun trimmed(
        phoneControlAvailable: Boolean = true,
        modelName: String? = null,
        sensorContext: SensorContext? = null
    ): String {
        val securityContent = readFileOrNull(AgentFileManager.SECURITY_FILE)
        return PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            securityContent = securityContent,
            sensorContext = sensorContext
        )
    }

    /**
     * Compose the identity section from SOUL.md and IDENTITY.md.
     * Returns null if neither file has content (falls back to hardcoded identity).
     */
    private fun buildIdentitySection(soulContent: String?, identityContent: String?, hasMemory: Boolean = false): String? {
        if (soulContent == null && identityContent == null) return null

        val parts = mutableListOf<String>()

        if (identityContent != null) {
            parts.add(identityContent)
        }

        if (soulContent != null) {
            parts.add(soulContent)
        }

        // Core phone agent role reminder (skip if identity files already contain it)
        val joined = parts.joinToString(" ")
        if ("AI agent that controls" !in joined) {
            parts.add("You are an AI agent that controls the user's Android phone. You see the screen, tap elements, type text, and navigate apps to complete tasks. When the user asks you to do something on their phone, you do it — efficiently and reliably. When they're just chatting, respond naturally without using tools.")
        }

        // Prime the model to attend to memory context at the end of the prompt
        if (hasMemory) {
            parts.add("You have memories from past interactions — always check the Memory Context section before responding.")
        }

        return parts.joinToString("\n\n")
    }

    private fun readFileOrNull(path: String): String? {
        val content = runCatching { fileManager.readFile(path) }
            .onFailure { e ->
                Log.d(TAG, "Skipping $path: not readable (${e.message})")
            }
            .getOrNull()

        if (content != null && content.isBlank()) {
            Log.d(TAG, "Skipping $path: blank or whitespace-only")
            return null
        }

        return content
    }

    companion object {
        private const val TAG = "AgentPromptBuilder"
    }
}
