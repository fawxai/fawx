package ai.citros.core

import java.text.Normalizer

/**
 * Lightweight keyword/intent heuristic for context-based category selection.
 * Stateless v1 — uses only current-turn message text.
 * Conservative: when uncertain, include additional safe categories.
 * See docs/specs/h2-3-tool-grouping-spec.md Section 5.5.
 */
object ContextCategoryResolver {

    data class ResolverResult(
        val candidates: Set<ToolCategory>,
        val actionIntent: Boolean
    )

    /** Extracted keyword groups for testability. */
    internal object Keywords {
        // Device-action verbs → NAVIGATION, INTERACTION, OBSERVATION
        val DEVICE_ACTION_KEYWORDS = setOf(
            "open", "tap", "click", "type", "swipe", "scroll", "press",
            "launch", "go to", "go back", "navigate", "switch",
            "settings", "app", "camera", "wifi", "bluetooth", "brightness"
        )

        // Notification intents
        val NOTIFICATION_KEYWORDS = setOf(
            "notification", "notifications", "notify", "alert", "alerts",
            "read notifications", "check notifications", "dismiss notification",
            "reply to notification"
        )

        // Clipboard intents
        val CLIPBOARD_KEYWORDS = setOf(
            "copy", "paste", "clipboard", "cut"
        )

        // Memory intents
        val MEMORY_KEYWORDS = setOf(
            "remember", "recall", "memorize", "note", "save for later",
            "what did i", "do you remember", "my notes", "store this",
            "save this", "write down", "jot down"
        )

        // Research intents
        val RESEARCH_KEYWORDS = setOf(
            "search", "look up", "find out", "google", "browse",
            "website", "web", "online", "news", "article",
            "what is", "who is", "how to", "wikipedia", "price",
            "weather forecast", "stock", "recipe", "review"
        )

        // Planning intents
        val PLANNING_KEYWORDS = setOf(
            "plan", "planning", "strategy", "organize", "schedule",
            "step by step", "break down", "decompose", "think through",
            "figure out how"
        )
    }

    /**
     * Resolve candidate categories from message text.
     * Returns only resolver-derived categories.
     * CORE is force-added later by [ToolGroupingPolicy].
     */
    fun resolve(messageText: String): ResolverResult {
        val normalized = normalizeText(messageText)
        val candidates = mutableSetOf<ToolCategory>()
        var actionIntent = false

        // Check device-action keywords → NAVIGATION, INTERACTION, OBSERVATION
        if (matchesAny(normalized, Keywords.DEVICE_ACTION_KEYWORDS)) {
            candidates += ToolCategory.NAVIGATION
            candidates += ToolCategory.INTERACTION
            candidates += ToolCategory.OBSERVATION
            actionIntent = true
        }

        // Check notification intents
        if (matchesAny(normalized, Keywords.NOTIFICATION_KEYWORDS)) {
            candidates += ToolCategory.NOTIFICATION
        }

        // Check clipboard intents
        if (matchesAny(normalized, Keywords.CLIPBOARD_KEYWORDS)) {
            candidates += ToolCategory.CLIPBOARD
        }

        // Check memory intents
        if (matchesAny(normalized, Keywords.MEMORY_KEYWORDS)) {
            candidates += ToolCategory.MEMORY
        }

        // Check research intents
        if (matchesAny(normalized, Keywords.RESEARCH_KEYWORDS)) {
            candidates += ToolCategory.RESEARCH
        }

        // Check planning intents
        if (matchesAny(normalized, Keywords.PLANNING_KEYWORDS)) {
            candidates += ToolCategory.PLANNING
        }

        return ResolverResult(candidates = candidates, actionIntent = actionIntent)
    }

    /**
     * Normalize text per Section 5.2.1:
     * Unicode NFKC, then lowercase using Locale.ROOT.
     */
    internal fun normalizeText(text: String): String {
        val nfkc = Normalizer.normalize(text, Normalizer.Form.NFKC)
        return nfkc.lowercase(java.util.Locale.ROOT).replace(Regex("\\s+"), " ").trim()
    }

    private fun matchesAny(normalized: String, keywords: Set<String>): Boolean {
        return keywords.any { keyword ->
            if (keyword.contains(' ')) {
                // Multi-word: substring match after whitespace collapsing
                normalized.contains(keyword)
            } else {
                // Single-word: whole-word boundary match
                Regex("\\b${Regex.escape(keyword)}\\b").containsMatchIn(normalized)
            }
        }
    }
}
