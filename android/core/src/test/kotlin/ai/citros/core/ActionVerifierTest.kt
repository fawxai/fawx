package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * Tests for [ActionVerifier] — post-action screenshot verification.
 *
 * Note: Full end-to-end screenshot verification (capture → vision → parse)
 * requires a real AccessibilityService which cannot be unit-tested.
 * The screenshot flow is covered by manual/instrumented tests on-device.
 * These tests cover the decision logic, response parsing, and error handling.
 */
class ActionVerifierTest {

    @Before
    fun setUp() {
        ScreenReader.detach()
    }

    // ========== shouldVerify Tests ==========

    @Test
    fun `NEVER mode never verifies any action`() {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.NEVER
        )
        assertFalse(verifier.shouldVerify("tap", "Tapped element 5"))
        assertFalse(verifier.shouldVerify("open_app", "Failed to open app"))
        assertFalse(verifier.shouldVerify("swipe", "Swiped up"))
    }

    @Test
    fun `ALWAYS mode verifies all UI actions`() {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ALWAYS
        )
        assertTrue(verifier.shouldVerify("tap", "Tapped element 5"))
        assertTrue(verifier.shouldVerify("open_app", "Opened Chrome"))
        assertTrue(verifier.shouldVerify("swipe", "Swiped up"))
        assertTrue(verifier.shouldVerify("type_text", "Typed \"hello\""))
        assertTrue(verifier.shouldVerify("press_back", "Pressed back"))
        assertTrue(verifier.shouldVerify("long_press", "Long-pressed element 3"))
        assertTrue(verifier.shouldVerify("paste", "Pasted (5 chars): \"hello\""))
    }

    @Test
    fun `ALWAYS mode does not verify non-UI actions`() {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ALWAYS
        )
        assertFalse(verifier.shouldVerify("think", "Thought: planning next step"))
        assertFalse(verifier.shouldVerify("read_screen", "Screen refreshed"))
        assertFalse(verifier.shouldVerify("screenshot", "Screenshot description"))
        assertFalse(verifier.shouldVerify("read_file", "{\"ok\":true}"))
        assertFalse(verifier.shouldVerify("write_file", "{\"ok\":true}"))
        assertFalse(verifier.shouldVerify("copy", "Clipboard content: hello"))
        assertFalse(verifier.shouldVerify("set_clipboard", "Copied to clipboard"))
        assertFalse(verifier.shouldVerify("remember", "{\"ok\":true}"))
        assertFalse(verifier.shouldVerify("recall", "{\"ok\":true}"))
        assertFalse(verifier.shouldVerify("wait", "Waited 2s"))
        assertFalse(verifier.shouldVerify("read_notifications", "No notifications"))
    }

    @Test
    fun `ON_FAILURE mode only verifies when action result looks like failure`() {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ON_FAILURE
        )

        // Successes → no verification
        assertFalse(verifier.shouldVerify("tap", "Tapped element 5"))
        assertFalse(verifier.shouldVerify("open_app", "Opened Chrome"))
        assertFalse(verifier.shouldVerify("swipe", "Swiped up"))
        assertFalse(verifier.shouldVerify("press_back", "Pressed back"))

        // Failures → verify
        assertTrue(verifier.shouldVerify("tap", "Failed to tap element 5"))
        assertTrue(verifier.shouldVerify("open_app", "Couldn't find MyApp — went home instead"))
        assertTrue(verifier.shouldVerify("tap_text", "Could not find element with text \"Submit\""))
    }

    @Test
    fun `ON_FAILURE mode does not verify non-UI actions even on failure`() {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ON_FAILURE
        )
        assertFalse(verifier.shouldVerify("think", "Error: thought is empty"))
        assertFalse(verifier.shouldVerify("read_screen", "Failed to read screen"))
        assertFalse(verifier.shouldVerify("read_file", "Error: file not found"))
    }

    @Test(expected = IllegalArgumentException::class)
    fun `shouldVerify rejects blank toolName`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)
        verifier.shouldVerify("", "Tapped element 5")
    }

    @Test(expected = IllegalArgumentException::class)
    fun `shouldVerify rejects blank actionResult`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)
        verifier.shouldVerify("tap", "")
    }

    // ========== looksLikeFailure Tests ==========

    @Test
    fun `looksLikeFailure detects failure keywords`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        assertTrue(verifier.looksLikeFailure("Failed to tap element 5"))
        assertTrue(verifier.looksLikeFailure("Error executing tap: null"))
        assertTrue(verifier.looksLikeFailure("Could not find element"))
        assertTrue(verifier.looksLikeFailure("Couldn't find MyApp"))
        assertTrue(verifier.looksLikeFailure("Element not found"))
        assertTrue(verifier.looksLikeFailure("Accessibility service not attached"))
        assertTrue(verifier.looksLikeFailure("Clipboard not available"))
        assertTrue(verifier.looksLikeFailure("Tool not configured"))
    }

    @Test
    fun `looksLikeFailure detects expanded keywords`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        assertTrue(verifier.looksLikeFailure("Unable to complete action"))
        assertTrue(verifier.looksLikeFailure("Action was unsuccessful"))
        assertTrue(verifier.looksLikeFailure("Permission denied"))
        assertTrue(verifier.looksLikeFailure("Request rejected"))
        assertTrue(verifier.looksLikeFailure("Invalid input"))
        assertTrue(verifier.looksLikeFailure("Missing parameter"))
        assertTrue(verifier.looksLikeFailure("Connection timeout"))
    }

    @Test
    fun `looksLikeFailure returns false for success messages`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        assertFalse(verifier.looksLikeFailure("Tapped element 5"))
        assertFalse(verifier.looksLikeFailure("Opened Chrome"))
        assertFalse(verifier.looksLikeFailure("Swiped up"))
        assertFalse(verifier.looksLikeFailure("Pressed back"))
        assertFalse(verifier.looksLikeFailure("Typed \"hello\""))
        assertFalse(verifier.looksLikeFailure("Long-pressed element 3"))
    }

    @Test
    fun `looksLikeFailure uses word boundaries to avoid false positives`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        // These contain failure keywords as substrings but NOT as whole words
        assertFalse(verifier.looksLikeFailure("Unfailed operation"))
        assertFalse(verifier.looksLikeFailure("Configured successfully"))
    }

    @Test
    fun `looksLikeFailure is case-insensitive`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        assertTrue(verifier.looksLikeFailure("FAILED TO TAP"))
        assertTrue(verifier.looksLikeFailure("Error: something"))
        assertTrue(verifier.looksLikeFailure("NOT FOUND"))
    }

    // ========== parseVerificationResponse Tests ==========

    @Test
    fun `parseVerificationResponse detects YES responses`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result1 = verifier.parseVerificationResponse("YES — Settings screen is now open")
        assertTrue(result1.verified)
        assertEquals("YES — Settings screen is now open", result1.description)
        assertNull(result1.error)

        val result2 = verifier.parseVerificationResponse("yes - it worked")
        assertTrue(result2.verified)

        val result3 = verifier.parseVerificationResponse("  YES  ")
        assertTrue(result3.verified)
    }

    @Test
    fun `parseVerificationResponse detects NO responses`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result1 = verifier.parseVerificationResponse("NO — Still on the home screen, nothing changed")
        assertFalse(result1.verified)
        assertEquals("NO — Still on the home screen, nothing changed", result1.description)
        assertNull(result1.error)

        val result2 = verifier.parseVerificationResponse("no - still same screen")
        assertFalse(result2.verified)
    }

    @Test
    fun `parseVerificationResponse treats ambiguous response as not verified`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result = verifier.parseVerificationResponse("I can see the home screen but nothing changed")
        assertFalse(result.verified)
    }

    @Test
    fun `parseVerificationResponse treats Maybe yes as not verified`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result = verifier.parseVerificationResponse("Maybe yes, maybe no")
        assertFalse(result.verified)
    }

    // ========== buildVerificationPrompt Tests ==========

    @Test
    fun `buildVerificationPrompt includes tool name and action result`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val prompt = verifier.buildVerificationPrompt("tap", "Tapped element 5")
        assertTrue(prompt.contains("tap"))
        assertTrue(prompt.contains("Tapped element 5"))
        assertTrue(prompt.contains("YES"))
        assertTrue(prompt.contains("NO"))
    }

    @Test
    fun `buildVerificationPrompt does not swap arguments`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val prompt = verifier.buildVerificationPrompt("open_app", "Opened Chrome")
        // Verify tool name appears after "action:" and result after "returned:"
        assertTrue(prompt.contains("this action: open_app"))
        assertTrue(prompt.contains("action returned: Opened Chrome"))
    }

    // ========== verify Tests ==========

    @Test
    fun `verify returns error when ScreenReader detached`() = runTest {
        ScreenReader.detach()
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result = verifier.verify("tap", "Tapped element 5")

        assertFalse(result.verified)
        assertNotNull(result.error)
        assertTrue(result.description.contains("Verification skipped"))
        assertTrue(result.error!!.contains("not attached"))
    }

    @Test
    fun `verify error result has consistent format`() = runTest {
        ScreenReader.detach()
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        val result = verifier.verify("tap", "Tapped element 5")

        // All error descriptions should start with "Verification skipped:"
        assertTrue(
            "Expected description to start with 'Verification skipped:', got: ${result.description}",
            result.description.startsWith("Verification skipped:")
        )
    }

    @Test
    fun `verify returns privacy-blocked error when screenshot is blocked`() = runTest {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ALWAYS,
            screenshotDelayMs = 0L,
            isScreenReaderAttached = { true },
            takeScreenshot = { ScreenshotResult.PrivacyBlocked }
        )

        val result = verifier.verify("tap", "Tapped element 5")

        assertFalse(result.verified)
        assertEquals("Screenshot blocked by privacy mode", result.error)
        assertTrue(result.description.contains("private_app"))
        assertFalse(result.description.contains("com.bank.app"))
    }

    @Test
    fun `verify returns screenshot failure reason when capture fails`() = runTest {
        val verifier = ActionVerifier(
            actionClient = dummyClient(),
            mode = VerificationMode.ALWAYS,
            screenshotDelayMs = 0L,
            isScreenReaderAttached = { true },
            takeScreenshot = { ScreenshotResult.Failed("Screenshot capture failed") }
        )

        val result = verifier.verify("tap", "Tapped element 5")

        assertFalse(result.verified)
        assertEquals("Screenshot capture failed", result.error)
        assertTrue(result.description.contains("screenshot capture failed", ignoreCase = true))
    }

    // ========== VerificationResult Tests ==========

    @Test
    fun `SKIPPED result is verified with skip description`() {
        val result = VerificationResult.SKIPPED
        assertTrue(result.verified)
        assertEquals("Verification skipped", result.description)
        assertNull(result.error)
    }

    @Test
    fun `VerificationResult preserves all fields`() {
        val result = VerificationResult(
            verified = false,
            description = "NO — nothing happened",
            error = null
        )
        assertFalse(result.verified)
        assertEquals("NO — nothing happened", result.description)
        assertNull(result.error)
    }

    @Test
    fun `VerificationResult with error has verified false`() {
        val result = VerificationResult(
            verified = false,
            description = "Verification skipped: vision API error - timeout",
            error = "Vision API timeout"
        )
        assertFalse(result.verified)
        assertNotNull(result.error)
        assertEquals("Vision API timeout", result.error)
    }

    // ========== VerificationMode Tests ==========

    @Test
    fun `all VerificationMode values exist`() {
        val modes = VerificationMode.values()
        assertEquals(3, modes.size)
        assertTrue(modes.contains(VerificationMode.ALWAYS))
        assertTrue(modes.contains(VerificationMode.ON_FAILURE))
        assertTrue(modes.contains(VerificationMode.NEVER))
    }

    // ========== Verifiable Actions Coverage ==========

    @Test
    fun `VERIFIABLE_ACTIONS contains exactly 14 UI action tools`() {
        assertEquals(
            "VERIFIABLE_ACTIONS count changed — update docs and PR if intentional",
            14,
            ActionVerifier.VERIFIABLE_ACTIONS.size
        )
    }

    @Test
    fun `VERIFIABLE_ACTIONS contains all expected UI tools`() {
        val expected = setOf(
            "tap", "tap_text", "type_text", "swipe", "scroll",
            "press_back", "press_home", "open_app", "open_notifications",
            "long_press", "paste", "tap_notification", "dismiss_notification",
            "reply_notification"
        )
        assertEquals(expected, ActionVerifier.VERIFIABLE_ACTIONS)
    }

    @Test
    fun `VERIFIABLE_ACTIONS does not contain non-UI tools`() {
        val nonUi = listOf(
            "think", "read_screen", "screenshot", "read_file", "write_file",
            "list_files", "copy", "set_clipboard", "remember", "recall",
            "list_memories", "wait", "read_notifications"
        )
        nonUi.forEach { tool ->
            assertFalse(
                "Expected $tool to NOT be in VERIFIABLE_ACTIONS",
                ActionVerifier.VERIFIABLE_ACTIONS.contains(tool)
            )
        }
    }

    // ========== Screenshot Delay Tests ==========

    @Test
    fun `default screenshot delay is 300ms`() {
        assertEquals(300L, ActionVerifier.DEFAULT_SCREENSHOT_DELAY_MS)
    }

    @Test
    fun `custom screenshot delay is accepted`() {
        // Just verify construction doesn't throw
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS, screenshotDelayMs = 500L)
        assertTrue(verifier.shouldVerify("tap", "Tapped element 5"))
    }

    @Test
    fun `zero delay is accepted`() {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS, screenshotDelayMs = 0L)
        assertTrue(verifier.shouldVerify("tap", "Tapped element 5"))
    }

    // ========== Integration-style Tests ==========

    @Test
    fun `full shouldVerify then verify flow with detached ScreenReader`() = runTest {
        ScreenReader.detach()
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ALWAYS)

        // shouldVerify says yes for UI action
        assertTrue(verifier.shouldVerify("tap", "Tapped element 5"))

        // verify returns error (verified=false, error non-null)
        val result = verifier.verify("tap", "Tapped element 5")
        assertFalse(result.verified)
        assertNotNull(result.error)
    }

    @Test
    fun `ON_FAILURE mode skips verification for successful tap then verifies failed tap`() = runTest {
        val verifier = ActionVerifier(dummyClient(), VerificationMode.ON_FAILURE)

        // Successful tap → no verification
        assertFalse(verifier.shouldVerify("tap", "Tapped element 5"))

        // Failed tap → verify
        assertTrue(verifier.shouldVerify("tap", "Failed to tap element 5"))
    }

    // ========== Helpers ==========

    private fun dummyClient(): ProviderClient = object : ProviderClient {
        override val provider = Provider.ANTHROPIC
        override suspend fun chat(conversation: Conversation) = Result.success("")
        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ) = Result.success(ChatResponse(text = "", toolCalls = emptyList(), stopReason = "end_turn"))
        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int) =
            Result.success("YES — action succeeded")
    }
}
