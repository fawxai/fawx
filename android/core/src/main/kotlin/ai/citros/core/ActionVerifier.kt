package ai.citros.core

/**
 * Verification mode for post-action screenshot verification.
 *
 * Controls when the agent captures a screenshot after performing an action
 * and asks the vision model whether the action succeeded.
 *
 * Usage guidelines:
 * - [ALWAYS]: Best for production when you need maximum reliability.
 *   Cost: ~$0.01-0.02 per action (depends on vision model pricing).
 * - [ON_FAILURE]: Recommended for most use cases. Only pays for verification
 *   when the action result text suggests failure (keywords: "failed", "error", etc.).
 *   Cost: ~$0.001-0.005 per session on average.
 * - [NEVER]: Use for development/testing when you don't want verification overhead.
 *   Cost: $0.
 */
enum class VerificationMode {
    /** Always verify after every UI action. */
    ALWAYS,

    /** Only verify when the action result text suggests failure. */
    ON_FAILURE,

    /** Never verify — skip screenshot verification entirely. */
    NEVER
}

/**
 * Result of a post-action verification check.
 *
 * @property verified Whether the vision model confirmed the action succeeded
 * @property description The vision model's description of what it sees
 * @property error Non-null if verification itself failed (e.g., screenshot capture error).
 *   When error is non-null, [verified] is false — callers should treat this as
 *   "verification skipped" rather than "action failed".
 */
data class VerificationResult(
    val verified: Boolean,
    val description: String,
    val error: String? = null
) {
    companion object {
        /** Verification was skipped (mode=NEVER or non-UI action). */
        val SKIPPED = VerificationResult(verified = true, description = "Verification skipped")
    }
}

/**
 * Post-action screenshot verifier for the OPAV (Observe-Plan-Act-Verify) loop.
 *
 * After a UI action (tap, swipe, type, etc.), captures a screenshot and asks
 * a vision-capable model whether the expected outcome occurred. This is more
 * reliable than accessibility tree diffs alone, especially for visual changes
 * like animations, color changes, or image content.
 *
 * Uses the action model (cheap, e.g. Haiku) for verification to keep costs low.
 *
 * @param actionClient Vision-capable client for verification (typically the cheap action model)
 * @param mode When to verify: ALWAYS, ON_FAILURE, or NEVER
 * @param screenshotDelayMs Delay in ms before capturing screenshot, to let UI animations settle.
 *   Default 300ms matches typical Android animation duration.
 */
class ActionVerifier(
    private val actionClient: ProviderClient,
    private val mode: VerificationMode = VerificationMode.NEVER,
    private val screenshotDelayMs: Long = DEFAULT_SCREENSHOT_DELAY_MS,
    /** ScreenReader attachment check, injectable for unit tests. */
    private val isScreenReaderAttached: () -> Boolean = { ScreenReader.isAttached() },
    /** Screenshot provider, injectable for unit tests. */
    private val takeScreenshot: suspend () -> ScreenshotResult = { ScreenReader.takeScreenshot() }
) {
    companion object {
        /** Default delay before screenshot capture (typical Android animation duration). */
        internal const val DEFAULT_SCREENSHOT_DELAY_MS = 300L

        /**
         * Tool names that represent UI actions worth verifying.
         * Non-UI tools (think, read_screen, read_file, etc.) are never verified.
         *
         * Count: 14 tools. Update [ActionVerifierTest.VERIFIABLE_ACTIONS count] test if changed.
         */
        internal val VERIFIABLE_ACTIONS = setOf(
            "tap", "tap_text", "type_text", "swipe", "scroll",
            "press_back", "press_home", "open_app", "open_notifications",
            "long_press", "paste", "tap_notification", "dismiss_notification",
            "reply_notification"
        )

        /** Failure keywords that trigger ON_FAILURE verification. */
        internal val FAILURE_KEYWORDS = listOf(
            "failed", "error", "could not", "couldn't", "not found",
            "not attached", "not available", "not configured",
            "unable", "unsuccessful", "denied", "rejected",
            "invalid", "missing", "timeout"
        )
    }

    /**
     * Check whether a given tool action should be verified based on current mode.
     *
     * @param toolName The name of the tool that was executed (must not be blank)
     * @param actionResult The text result from executing the tool (must not be blank)
     * @return true if verification should proceed
     * @throws IllegalArgumentException if toolName or actionResult is blank
     */
    fun shouldVerify(toolName: String, actionResult: String): Boolean {
        require(toolName.isNotBlank()) { "toolName must not be blank" }
        require(actionResult.isNotBlank()) { "actionResult must not be blank" }

        if (mode == VerificationMode.NEVER) return false
        if (toolName !in VERIFIABLE_ACTIONS) return false

        return when (mode) {
            VerificationMode.ALWAYS -> true
            VerificationMode.ON_FAILURE -> looksLikeFailure(actionResult)
            VerificationMode.NEVER -> false
        }
    }

    /**
     * Verify that an action succeeded by capturing a screenshot and asking the
     * vision model.
     *
     * Waits [screenshotDelayMs] before capturing to let UI animations settle.
     *
     * If verification itself fails (ScreenReader detached, screenshot capture fails,
     * or vision API errors), returns `verified = false` with a non-null [VerificationResult.error].
     * The caller should treat error results as "verification skipped" (don't block the loop).
     *
     * @param toolName The tool that was executed (e.g., "tap", "open_app")
     * @param actionResult The text result from the tool execution
     * @return VerificationResult with the vision model's assessment
     */
    suspend fun verify(toolName: String, actionResult: String): VerificationResult {
        // Capture screenshot
        if (!isScreenReaderAttached()) {
            return VerificationResult(
                verified = false,
                description = "Verification skipped: accessibility service not attached",
                error = "Accessibility service not attached"
            )
        }

        // Wait for UI to settle (animations, transitions)
        if (screenshotDelayMs > 0) {
            kotlinx.coroutines.delay(screenshotDelayMs)
        }

        val base64 = when (val screenshot = takeScreenshot()) {
            is ScreenshotResult.Success -> screenshot.base64
            is ScreenshotResult.PrivacyBlocked -> {
                return VerificationResult(
                    verified = false,
                    description = "Verification skipped: screenshot blocked by privacy mode for ${PrivacyRedaction.APP_PLACEHOLDER}",
                    error = "Screenshot blocked by privacy mode"
                )
            }
            is ScreenshotResult.Failed -> {
                return VerificationResult(
                    verified = false,
                    description = "Verification skipped: screenshot capture failed (${screenshot.reason ?: "unknown"})",
                    error = screenshot.reason ?: "Screenshot capture failed"
                )
            }
        }

        // Build verification prompt
        val prompt = buildVerificationPrompt(toolName, actionResult)

        // Ask the action model (cheap) to verify
        val result = actionClient.describeImage(base64, prompt)

        return result.fold(
            onSuccess = { response -> parseVerificationResponse(response) },
            onFailure = { error ->
                // Vision call failed — don't block the action loop
                VerificationResult(
                    verified = false,
                    description = "Verification skipped: vision API error - ${error.message}",
                    error = error.message
                )
            }
        )
    }

    /**
     * Build the verification prompt sent to the vision model.
     * Uses Kotlin string templates for safety and readability.
     */
    internal fun buildVerificationPrompt(toolName: String, actionResult: String): String {
        return "You just performed this action: $toolName\n" +
            "The action returned: $actionResult\n\n" +
            "Look at the screenshot and determine:\n" +
            "1. Did the action succeed? (YES or NO)\n" +
            "2. What do you see on screen now?\n\n" +
            "Reply with exactly one line starting with YES or NO, followed by a brief description.\n" +
            "Example: YES — Settings screen is now open\n" +
            "Example: NO — Still on the home screen, nothing changed"
    }

    /**
     * Parse the vision model's response into a [VerificationResult].
     * Expects the response to start with "YES" or "NO".
     */
    internal fun parseVerificationResponse(response: String): VerificationResult {
        val trimmed = response.trim()
        val verified = trimmed.uppercase().startsWith("YES")
        return VerificationResult(verified = verified, description = trimmed)
    }

    /**
     * Check if the action result text looks like a failure.
     * Uses word boundary matching to reduce false positives.
     * Used by ON_FAILURE mode to decide whether to verify.
     */
    internal fun looksLikeFailure(actionResult: String): Boolean {
        val lower = actionResult.lowercase()
        return FAILURE_KEYWORDS.any { keyword ->
            Regex("\\b${Regex.escape(keyword)}\\b").containsMatchIn(lower)
        }
    }
}
