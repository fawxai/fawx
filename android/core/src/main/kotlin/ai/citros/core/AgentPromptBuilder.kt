package ai.citros.core

import android.util.Log

/** Builds system prompts from agent markdown files. */
class AgentPromptBuilder(
    private val fileManager: AgentFileManager
) {
    fun full(): String {
        val sections = listOf(
            "SOUL.md",
            "USER.md",
            "AGENTS.md",
            "SECURITY.md",
            "TOOLS.md",
            "MEMORY.md"
        ).mapNotNull { namedSection(it) }

        return sections.joinToString("\n\n").ifBlank { PhoneAgentPrompts.buildSystemPrompt() }
    }

    fun trimmed(): String {
        val sections = listOf("SOUL.md", "SECURITY.md").mapNotNull { namedSection(it) }
        return sections.joinToString("\n\n").ifBlank { PhoneAgentPrompts.buildActionPrompt() }
    }

    private fun namedSection(path: String): String? {
        val content = runCatching { fileManager.readFile(path) }
            .onFailure { e ->
                Log.d(TAG, "Skipping section $path: not readable (${e.message})")
            }
            .getOrNull()

        if (content != null && content.isBlank()) {
            Log.d(TAG, "Skipping section $path: blank or whitespace-only")
            return null
        }

        return content?.let { "## $path\n$it" }
    }

    companion object {
        private const val TAG = "AgentPromptBuilder"
    }
}
