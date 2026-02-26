package ai.citros.core

import android.graphics.Rect

data class ScreenContent(
    val elements: List<ScreenElement>,
    val packageName: String?,
    val privacyMode: Boolean = false
) {
    companion object {
        /** Default maximum elements included in prompt text. */
        const val DEFAULT_ELEMENT_CAP = 40
        const val PRIVACY_APP_PLACEHOLDER = PrivacyRedaction.APP_PLACEHOLDER
    }

    fun toToolResult(): String {
        if (privacyMode) {
            return "SCREEN: ${privacyHiddenMessage()}"
        }
        return "SCREEN:\n${toPromptText()}"
    }

    /**
     * Format screen content as prompt text for the LLM.
     *
     * @param elementCap Maximum elements to include (default [DEFAULT_ELEMENT_CAP]).
     *   Complex apps may have 100+ elements; higher caps give the model more context
     *   at the cost of more tokens. Use model-tier-aware values for optimization.
     *   Clamped to 1..200 to prevent degenerate cases.
     */
    fun toPromptText(elementCap: Int = DEFAULT_ELEMENT_CAP): String {
        if (privacyMode) {
            return privacyHiddenMessage()
        }
        val cap = elementCap.coerceIn(1, 200)
        if (elements.isEmpty() && packageName == null) {
            return "No target app is visible. The Citros overlay is in the foreground. Use open_app to launch the app you need."
        }
        val sb = StringBuilder()
        sb.appendLine("App: ${packageName ?: "unknown"}")

        val prioritized = elements
            .sortedByDescending { e ->
                var score = 0
                if (e.isClickable) score += 3
                if (e.isEditable) score += 4
                if (e.text != null) score += 2
                if (e.contentDescription != null) score += 1
                score
            }
            .take(cap)
            .sortedBy { it.id }

        prioritized.forEach { element ->
            val desc = buildString {
                val indent = "  ".repeat(element.depth.coerceAtMost(4))
                append("$indent[${element.id}]")
                element.text?.let { append(" \"${it.take(50)}\"") }
                element.contentDescription?.let { append(" (${it.take(30)})") }
                if (element.isClickable) append(" [click]")
                if (element.isEditable) append(" [edit]")
            }
            sb.appendLine(desc)
        }

        if (elements.size > cap) {
            sb.appendLine("(${elements.size - cap} more elements hidden)")
        }

        return sb.toString()
    }

    private fun privacyHiddenMessage(): String =
        "[Privacy mode — screen content hidden for $PRIVACY_APP_PLACEHOLDER. Ask the user for guidance if needed.]"
}

data class ScreenElement(
    val id: Int,
    val text: String?,
    val contentDescription: String?,
    val className: String?,
    val isClickable: Boolean,
    val isEditable: Boolean,
    val bounds: Rect,
    /** Nesting depth in the accessibility tree (0 = top-level). */
    val depth: Int = 0
)
