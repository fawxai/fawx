package ai.citros.core

/**
 * Classification for how a tool result should be displayed to the user.
 *
 * See docs/agentic-loop-v2.md §7 (User-Facing Output)
 */
enum class OutputVisibility {
    /** Show prominently — high-level actions, results, errors. */
    SHOW,
    /** Show but dimmed/italic — agent reasoning, minor status updates. */
    SHOW_DIMMED,
    /** Hide from UI — mechanical actions (taps, swipes, scrolls). */
    HIDE
}

/**
 * User preference for how much tool output to display.
 */
enum class OutputVerbosity {
    /** Show everything including mechanical actions. */
    VERBOSE,
    /** Default: hide mechanical, show/dim the rest. */
    NORMAL,
    /** Show only results and errors. */
    MINIMAL
}

/**
 * Tool categories for classification.
 *
 * Groups tools by their UX behavior rather than individual name checks.
 * New tools are added to the appropriate category rather than scattering
 * set membership across the classifier.
 */
enum class ToolCategory {
    /** Mechanical UI actions — hidden by default. */
    MECHANICAL,
    /** High-level actions and results — always shown. */
    PROMINENT,
    /** Research/information retrieval — always shown. */
    RESEARCH,
    /** Agent reasoning — shown dimmed. */
    REASONING,
    /** Everything else — shown dimmed. */
    OTHER
}

/**
 * Severity classification for tool execution errors.
 *
 * Determines how prominently an error is surfaced to the user.
 * See docs/specs/error-visibility-design.md for full taxonomy.
 */
enum class ErrorSeverity {
    /** Agent retries silently. User sees nothing.
     *  Examples: wrong tap target, element not found, empty grep */
    EXPLORATORY,

    /** Brief issue, may self-resolve. Show only as status indicator.
     *  Examples: accessibility briefly disconnected, transient 500 */
    TRANSIENT,

    /** User needs to know. Show prominently in chat.
     *  Examples: accessibility gone permanently, API key invalid,
     *  repeated failures on same action */
    PERSISTENT,

    /** Useful context the user should see, even though it's "error"-flagged.
     *  Examples: "App not installed", permission denied (actionable) */
    INFORMATIONAL
}

/**
 * Context for error escalation decisions.
 *
 * Tracks consecutive failures per tool to enable severity escalation:
 * EXPLORATORY → TRANSIENT (at [escalateToTransientAt]) → PERSISTENT (at [escalateToPersistentAt]).
 */
data class RetryContext(
    /** Current count of consecutive failures (inclusive: >= thresholds trigger escalation). */
    val consecutiveFailures: Int = 0,
    /** Escalate EXPLORATORY → TRANSIENT when consecutiveFailures >= this value. */
    val escalateToTransientAt: Int = 2,
    /** Escalate TRANSIENT → PERSISTENT when consecutiveFailures >= this value. */
    val escalateToPersistentAt: Int = 3
) {
    init {
        require(escalateToTransientAt <= escalateToPersistentAt) {
            "escalateToTransientAt ($escalateToTransientAt) must be <= escalateToPersistentAt ($escalateToPersistentAt)"
        }
    }
}

/**
 * Classifies tool output for user display.
 *
 * Determines how prominently a tool result should appear in the chat UI
 * and whether it should be spoken in audio/voice mode.
 *
 * Classification uses [ToolCategory] to group tools:
 * - **MECHANICAL/HIDE:** tap, swipe, scroll, press_back, etc.
 * - **REASONING/SHOW_DIMMED:** think
 * - **PROMINENT/SHOW:** open_app, screenshot, subtask
 * - **RESEARCH/SHOW:** web_search, web_fetch
 * - **OTHER/SHOW_DIMMED:** file ops, memory, clipboard, wait, read_screen, etc.
 *
 * Errors (via [ToolResult.isError]) are classified by [ErrorSeverity] — see [classifyError].
 *
 * TODO: Audio classification (spec §7.2) — when voice/audio mode is implemented,
 * add an AudioVisibility enum (ANNOUNCE, OPTIONAL, SILENT) mapped from OutputVisibility.
 * SHOW → ANNOUNCE, SHOW_DIMMED → OPTIONAL, HIDE → SILENT.
 */
object OutputClassifier {

    /**
     * Tool name → category mapping.
     *
     * Tools not in this map default to [ToolCategory.OTHER] (SHOW_DIMMED).
     * Add new tools here rather than creating separate sets.
     */
    internal val TOOL_CATEGORIES: Map<String, ToolCategory> = mapOf(
        // Mechanical — hidden by default
        "tap" to ToolCategory.MECHANICAL,
        "tap_text" to ToolCategory.MECHANICAL,
        "long_press" to ToolCategory.MECHANICAL,
        "swipe" to ToolCategory.MECHANICAL,
        "scroll" to ToolCategory.MECHANICAL,
        "press_back" to ToolCategory.MECHANICAL,
        "press_home" to ToolCategory.MECHANICAL,
        "type_text" to ToolCategory.MECHANICAL,

        // Prominent — always shown
        "open_app" to ToolCategory.PROMINENT,
        "open_notifications" to ToolCategory.PROMINENT,
        "screenshot" to ToolCategory.PROMINENT,
        "subtask" to ToolCategory.PROMINENT,

        // Research — always shown
        "web_search" to ToolCategory.RESEARCH,
        "web_fetch" to ToolCategory.RESEARCH,
        "web_browse" to ToolCategory.RESEARCH,
        "recall" to ToolCategory.RESEARCH,

        // Reasoning — shown dimmed
        // Status
        "wait" to ToolCategory.MECHANICAL,
        "read_screen" to ToolCategory.MECHANICAL,
        "read_notifications" to ToolCategory.PROMINENT,
        "learn" to ToolCategory.OTHER,
        "remember" to ToolCategory.OTHER,
        "list_files" to ToolCategory.OTHER,
        "read_file" to ToolCategory.OTHER,
        "write_file" to ToolCategory.OTHER,
        "copy" to ToolCategory.OTHER,
        "set_clipboard" to ToolCategory.OTHER,
        "list_memories" to ToolCategory.OTHER,
        "tap_notification" to ToolCategory.OTHER,
        "dismiss_notification" to ToolCategory.OTHER,
        "reply_notification" to ToolCategory.OTHER,
        "paste" to ToolCategory.OTHER,
        "clipboard" to ToolCategory.OTHER,
        "think" to ToolCategory.REASONING
    )

    /**
     * Look up the category for a tool.
     *
     * @param toolName The tool name
     * @return The tool's category, or [ToolCategory.OTHER] if not mapped
     */
    fun categoryOf(toolName: String): ToolCategory {
        return TOOL_CATEGORIES[toolName] ?: ToolCategory.OTHER
    }

    /**
     * Classify an error by severity.
     *
     * Uses tool category, error message content, and optional retry context
     * for escalation. When [ToolResult.severity] is set, that takes precedence.
     *
     * Classification rules (evaluated in order):
     * 1. Accessibility lost keywords → PERSISTENT
     * 2. API auth failure (401/403 keywords) → PERSISTENT
     * 3. Rate limit (429) after escalation threshold → PERSISTENT
     * 4. Server error (5xx keywords), first occurrence → TRANSIENT
     * 5. "element not found" / "could not tap" → EXPLORATORY
     * 6. "no results" from search/fetch → INFORMATIONAL
     * 7. "app not installed" / "permission denied" → INFORMATIONAL
     * 8. Network timeout, check retry context for escalation
     * 9. Unknown error on MECHANICAL tool → EXPLORATORY
     * 10. Unknown error on PROMINENT/RESEARCH tool → INFORMATIONAL
     * 11. Default fallback → EXPLORATORY
     */
    fun classifyError(
        toolName: String,
        errorText: String,
        retryContext: RetryContext? = null
    ): ErrorSeverity {
        val lower = errorText.lowercase()

        // 1. Accessibility lost
        if ("accessibility" in lower &&
            ("lost" in lower || "disconnected" in lower || "unavailable" in lower)
        ) {
            return ErrorSeverity.PERSISTENT
        }

        // 2. API auth failure
        if ("401" in lower || "403" in lower || "unauthorized" in lower ||
            "api key" in lower || Regex("invalid.*key").containsMatchIn(lower)
        ) {
            return ErrorSeverity.PERSISTENT
        }

        // 3. Rate limit (429) — base is TRANSIENT, escalates to PERSISTENT via retryContext
        if ("429" in lower) {
            return escalate(ErrorSeverity.TRANSIENT, retryContext)
        }

        // 4. Server errors (5xx)
        if ("500" in lower || "502" in lower || "503" in lower || "529" in lower ||
            "server error" in lower
        ) {
            val base = ErrorSeverity.TRANSIENT
            return escalate(base, retryContext)
        }

        // 5. Exploratory UI failures
        if ("element not found" in lower || "could not tap" in lower ||
            "could not find" in lower || "no matching" in lower ||
            "failed to tap" in lower || "failed to click" in lower
        ) {
            val base = ErrorSeverity.EXPLORATORY
            return escalate(base, retryContext)
        }

        // 6. No results
        if ("no results" in lower) {
            return ErrorSeverity.INFORMATIONAL
        }

        // 7. App not installed / permission denied
        if ("not installed" in lower || "permission denied" in lower ||
            "access denied" in lower
        ) {
            return ErrorSeverity.INFORMATIONAL
        }

        // 8. Timeout with escalation
        if ("timeout" in lower) {
            val base = ErrorSeverity.TRANSIENT
            return escalate(base, retryContext)
        }

        // 9-10. Unknown error — classify by tool category
        return when (categoryOf(toolName)) {
            ToolCategory.MECHANICAL -> escalate(ErrorSeverity.EXPLORATORY, retryContext)
            ToolCategory.PROMINENT, ToolCategory.RESEARCH -> ErrorSeverity.INFORMATIONAL
            else -> escalate(ErrorSeverity.EXPLORATORY, retryContext)
        }
    }

    /**
     * Apply retry-context escalation to a base severity.
     *
     * - EXPLORATORY escalates to TRANSIENT at [RetryContext.escalateToTransientAt]
     * - TRANSIENT escalates to PERSISTENT at [RetryContext.escalateToPersistentAt]
     * - PERSISTENT and INFORMATIONAL are not escalated
     */
    private fun escalate(base: ErrorSeverity, retryContext: RetryContext?): ErrorSeverity {
        if (retryContext == null) return base
        val failures = retryContext.consecutiveFailures
        return when (base) {
            ErrorSeverity.EXPLORATORY -> when {
                failures >= retryContext.escalateToPersistentAt -> ErrorSeverity.PERSISTENT
                failures >= retryContext.escalateToTransientAt -> ErrorSeverity.TRANSIENT
                else -> base
            }
            ErrorSeverity.TRANSIENT -> when {
                failures >= retryContext.escalateToPersistentAt -> ErrorSeverity.PERSISTENT
                else -> base
            }
            else -> base
        }
    }

    /**
     * Default visibility for a tool based on its category (non-error path).
     */
    private fun categoryVisibility(toolName: String): OutputVisibility {
        return when (categoryOf(toolName)) {
            ToolCategory.MECHANICAL -> OutputVisibility.HIDE
            ToolCategory.PROMINENT -> OutputVisibility.SHOW
            ToolCategory.RESEARCH -> OutputVisibility.SHOW
            ToolCategory.REASONING -> OutputVisibility.SHOW_DIMMED
            ToolCategory.OTHER -> OutputVisibility.SHOW_DIMMED
        }
    }

    /**
     * Map an [ErrorSeverity] to [OutputVisibility].
     */
    private fun severityToVisibility(severity: ErrorSeverity): OutputVisibility {
        return when (severity) {
            ErrorSeverity.EXPLORATORY -> OutputVisibility.HIDE
            ErrorSeverity.TRANSIENT -> OutputVisibility.HIDE
            ErrorSeverity.PERSISTENT -> OutputVisibility.SHOW
            ErrorSeverity.INFORMATIONAL -> OutputVisibility.SHOW_DIMMED
        }
    }

    /**
     * Classify a tool result for display.
     *
     * @param toolName The name of the tool that produced the result
     * @param result The tool result text
     * @param isError Whether the tool result is an error
     * @return The visibility classification
     */
    fun classify(toolName: String, result: String, isError: Boolean = false): OutputVisibility {
        if (isError) {
            return severityToVisibility(classifyError(toolName, result))
        }
        return categoryVisibility(toolName)
    }

    /**
     * Classify a tool result for display with optional error severity and retry context.
     *
     * @param toolName The name of the tool that produced the result
     * @param result The tool result text
     * @param isError Whether the tool result is an error
     * @param severity Pre-classified error severity (takes precedence over auto-classification)
     * @param retryContext Retry context for escalation decisions
     * @return The visibility classification
     */
    fun classify(
        toolName: String,
        result: String,
        isError: Boolean = false,
        severity: ErrorSeverity? = null,
        retryContext: RetryContext? = null
    ): OutputVisibility {
        if (isError) {
            val effectiveSeverity = severity ?: classifyError(toolName, result, retryContext)
            return severityToVisibility(effectiveSeverity)
        }
        return categoryVisibility(toolName)
    }

    /**
     * Apply user verbosity preference to override default classification.
     *
     * @param visibility The default classification from [classify]
     * @param verbosity The user's display preference
     * @return The effective visibility after applying user preference
     */
    fun applyVerbosity(
        visibility: OutputVisibility,
        verbosity: OutputVerbosity
    ): OutputVisibility {
        return applyVerbosity(visibility, verbosity, severity = null)
    }

    /**
     * Apply user verbosity preference with error severity awareness.
     *
     * PERSISTENT errors always break through regardless of verbosity.
     * See docs/specs/error-visibility-design.md §6 for the full mapping.
     *
     * | Severity      | MINIMAL | NORMAL     | VERBOSE    |
     * |---------------|---------|------------|------------|
     * | EXPLORATORY   | HIDE    | HIDE       | SHOW_DIMMED|
     * | TRANSIENT     | HIDE    | HIDE       | SHOW_DIMMED|
     * | PERSISTENT    | SHOW    | SHOW       | SHOW       |
     * | INFORMATIONAL | HIDE    | SHOW_DIMMED| SHOW       |
     *
     * @param visibility The default classification from [classify]
     * @param verbosity The user's display preference
     * @param severity Error severity, or null for non-error results
     * @return The effective visibility after applying user preference
     */
    fun applyVerbosity(
        visibility: OutputVisibility,
        verbosity: OutputVerbosity,
        severity: ErrorSeverity? = null
    ): OutputVisibility {
        // PERSISTENT errors always break through
        if (severity == ErrorSeverity.PERSISTENT) return OutputVisibility.SHOW

        if (severity != null) {
            // Error-specific verbosity mapping from design doc §6
            return when (verbosity) {
                OutputVerbosity.VERBOSE -> when (severity) {
                    ErrorSeverity.EXPLORATORY -> OutputVisibility.SHOW_DIMMED
                    ErrorSeverity.TRANSIENT -> OutputVisibility.SHOW_DIMMED
                    ErrorSeverity.INFORMATIONAL -> OutputVisibility.SHOW
                    else -> visibility // PERSISTENT already handled above
                }
                OutputVerbosity.MINIMAL -> when (severity) {
                    ErrorSeverity.EXPLORATORY -> OutputVisibility.HIDE
                    ErrorSeverity.TRANSIENT -> OutputVisibility.HIDE
                    ErrorSeverity.INFORMATIONAL -> OutputVisibility.HIDE
                    else -> visibility // PERSISTENT already handled above
                }
                OutputVerbosity.NORMAL -> visibility
            }
        }

        // Non-error: original behavior
        return when (verbosity) {
            OutputVerbosity.VERBOSE -> OutputVisibility.SHOW
            OutputVerbosity.MINIMAL -> when (visibility) {
                OutputVisibility.SHOW -> OutputVisibility.SHOW
                OutputVisibility.SHOW_DIMMED -> OutputVisibility.HIDE
                OutputVisibility.HIDE -> OutputVisibility.HIDE
            }
            OutputVerbosity.NORMAL -> visibility
        }
    }

    /**
     * Maximum characters of tool result text shown in the chat UI.
     *
     * Full results stay in the LLM conversation context; this only affects
     * the user-facing message bubbles and overlay lines.
     */
    internal const val DISPLAY_MAX_CHARS = 200

    /**
     * Format a tool result for the user-facing chat UI.
     *
     * Summarizes verbose output (screen dumps, JSON payloads, long text) into
     * a concise display string. Full results remain in the LLM conversation
     * context — this only controls what the user sees in chat bubbles and
     * overlay lines.
     *
     * @param toolName The tool that produced this result
     * @param result Raw result text from tool execution
     * @param visibility Classification from [classify]
     * @return Formatted display string, or null if the result should be hidden
     */
    fun formatForDisplay(
        toolName: String,
        result: String,
        visibility: OutputVisibility
    ): String? {
        return when (visibility) {
            OutputVisibility.HIDE -> null
            OutputVisibility.SHOW_DIMMED -> when (categoryOf(toolName)) {
                ToolCategory.REASONING -> "💭 ${summarize(result)}"
                else -> "⚙️ ${summarize(result)}"
            }
            OutputVisibility.SHOW -> "🤖 ${summarize(result)}"
        }
    }

    /**
     * Summarize tool result text for display.
     *
     * Rules:
     * 1. Strip `SCREEN:` blocks (full accessibility dumps are for the LLM, not the user)
     * 2. Strip `[Verified: ...]` / `[Verification ...]` suffixes
     * 3. Take only the first meaningful line (action summary)
     * 4. Truncate at [DISPLAY_MAX_CHARS] on a word boundary
     */
    internal fun summarize(result: String): String {
        parseJsonSummary(result)?.let { return it }

        // Handle "Screenshot description:\n..." — extract the actual description
        if (result.startsWith("Screenshot description:")) {
            val desc = result.removePrefix("Screenshot description:").trim()
            return if (desc.isNotEmpty()) {
                truncateAtWord(desc.lineSequence().first { it.isNotBlank() })
            } else {
                "Analyzed screen"
            }
        }

        // Strip "Waited Xs. Screen:\n..." down to just "Waited Xs"
        val withoutWaitDump = result.replace(
            Regex("""(Waited \d+(?:\.\d+)?(?:s|ms))\.\s*Screen:\n[\s\S]*"""), "$1"
        ).trim()

        // Strip SCREEN: blocks — everything from "SCREEN:" onward
        val withoutScreen = withoutWaitDump.replace(Regex("""(^|\n+)SCREEN:\n[\s\S]*"""), "").trim()

        // Strip verification suffixes (newline-prefixed; inline suffixes like
        // "Tapped Send [Verified: ok]" are kept as they're part of the action text)
        val withoutVerification = withoutScreen
            .replace(Regex("""\n\[Verif(ied|ication)[^\]]*]"""), "")
            .trim()

        // Take first non-empty line as the summary
        val firstLine = withoutVerification.lineSequence()
            .map { it.trim() }
            .firstOrNull { it.isNotEmpty() }
            ?: return if (withoutVerification.length <= DISPLAY_MAX_CHARS) {
                withoutVerification
            } else {
                withoutVerification.take(DISPLAY_MAX_CHARS) + "…"
            }

        return truncateAtWord(firstLine)
    }

    /**
     * Parses compact JSON tool results into a user-facing summary.
     *
     * Intentionally handles only known lightweight shapes emitted by tool wrappers,
     * not arbitrary JSON parsing: `{ok, tool, ...}` with specialized handling for
     * remember/learn/recall/list_files/read_file and a best-effort string fallback
     * for unknown tools. If the payload is not recognized (missing required fields,
     * malformed, or no extractable summary), returns null so summarize() falls back
     * to the existing non-JSON line/regex pipeline.
     */
    private fun parseJsonSummary(result: String): String? {
        val trimmed = result.trim()
        if (!trimmed.startsWith("{") || !trimmed.contains("\"ok\"") || !trimmed.contains("\"tool\"")) {
            return null
        }

        val tool = matchStringField(trimmed, "tool")
        return when (tool) {
            "remember", "learn" -> {
                val content = matchStringField(trimmed, "content")
                if (!content.isNullOrBlank()) "Saved: ${content.trim()}" else "Saved"
            }
            "recall" -> {
                val recalled = Regex(""""results"\s*:\s*\[\s*\{[^}]*"content"\s*:\s*"([^"]+)"""")
                    .find(trimmed)
                    ?.groupValues
                    ?.getOrNull(1)
                    ?.trim()
                if (!recalled.isNullOrEmpty()) "Recalled: $recalled" else "No results found"
            }
            "list_files" -> {
                val block = Regex(""""files"\s*:\s*\[([^\]]*)]""")
                    .find(trimmed)
                    ?.groupValues
                    ?.getOrNull(1)
                    ?: return "Files: none"
                val files = Regex(""""([^"]+)"""")
                    .findAll(block)
                    .map { it.groupValues[1] }
                    .toList()
                if (files.isEmpty()) {
                    "Files: none"
                } else {
                    val shown = files.take(3).joinToString(", ")
                    val suffix = if (files.size > 3) ", ..." else ""
                    "Files: $shown$suffix"
                }
            }
            "read_file" -> {
                val path = matchStringField(trimmed, "path")
                if (!path.isNullOrBlank()) "Read: ${path.trim()}" else "Read file"
            }
            else -> {
                Regex(""":\s*"([^"]{2,})"""")
                    .find(trimmed)
                    ?.groupValues
                    ?.getOrNull(1)
            }
        }
    }

    // Regex helper for flat string fields in compact tool JSON.
    // Limitation: does not handle escaped quotes inside values (e.g. "), which is
    // acceptable for current summaries and falls back safely when no match is found.
    private fun matchStringField(json: String, key: String): String? {
        return Regex(""""$key"\s*:\s*"([^"]*)"""")
            .find(json)
            ?.groupValues
            ?.getOrNull(1)
    }

    /**
     * Truncate a string at a word boundary within [DISPLAY_MAX_CHARS].
     */
    private fun truncateAtWord(text: String): String {
        return if (text.length <= DISPLAY_MAX_CHARS) {
            text
        } else {
            val truncated = text.substring(0, DISPLAY_MAX_CHARS)
            val lastSpace = truncated.lastIndexOf(' ')
            if (lastSpace > DISPLAY_MAX_CHARS / 2) {
                truncated.substring(0, lastSpace) + "…"
            } else {
                truncated + "…"
            }
        }
    }
}
