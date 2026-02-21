package ai.citros.chat

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayStep
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithText
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Tests for the live overlay composables in OverlayContent.kt.
 *
 * Covers PR #624 (Send/Queue button state + placeholder text) and
 * PR #614 (overlay rendering with markdown lines, auto-scroll).
 *
 * These test the LIVE overlay composables ([OverlayMiniChatContent]),
 * not the ported-screen copies in OverlayPortedScreen.kt.
 */
@RunWith(RobolectricTestRunner::class)
class OverlayContentTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val idleStep = OverlayStep(step = 0, total = 0, label = "")
    private val executingStep = OverlayStep(step = 2, total = 5, label = "Running tool")

    // ========== PR #624: Send/Queue button text ==========

    @Test
    fun `mini chat shows Send button when idle`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send").assertIsDisplayed()
    }

    @Test
    fun `mini chat shows Stop button when executing with empty draft`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // Empty draft + executing = stop button
        composeRule.onNodeWithContentDescription("Stop").assertIsDisplayed()
    }

    @Test
    fun `mini chat shows Send button when completed`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.COMPLETED,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send").assertIsDisplayed()
    }

    @Test
    fun `mini chat shows Send button when failed`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.FAILED,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send").assertIsDisplayed()
    }

    @Test
    fun `mini chat shows Send button when stopped`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send").assertIsDisplayed()
    }

    // ========== PR #624: Placeholder text ==========

    @Test
    fun `mini chat shows idle placeholder when not executing`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Message", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat shows queue placeholder when executing`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Steer or queue...", useUnmergedTree = true).assertExists()
    }

    // ========== PR #614: Header status text ==========

    @Test
    fun `mini chat header shows Ready when idle`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Ready").assertIsDisplayed()
    }

    @Test
    fun `mini chat header shows step label when executing`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Running tool").assertIsDisplayed()
    }

    @Test
    fun `mini chat header shows Completed when done`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.COMPLETED,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Completed").assertIsDisplayed()
    }

    // ========== PR #614: Markdown rendering in transcript lines ==========

    @Test
    fun `mini chat renders transcript lines with markdown`() {
        val lines = listOf(
            OverlayLine(id = 1, type = OverlayLineType.USER, text = "Do **something**"),
            OverlayLine(id = 2, type = OverlayLineType.SYSTEM, text = "Opening app")
        )

        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = lines,
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // MarkdownText renders the text content (bold formatting applied via AnnotatedString)
        composeRule.onNodeWithText("Do something", substring = true).assertExists()
        composeRule.onNodeWithText("Opening app").assertExists()
    }

    @Test
    fun `mini chat renders queued line with badge`() {
        val lines = listOf(
            OverlayLine(id = 1, type = OverlayLineType.QUEUED, text = "Check bluetooth too")
        )

        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = lines,
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // QUEUED lines are style-only now; assert via stable semantic tag + text
        composeRule.onNodeWithTag(TEST_TAG_OVERLAY_QUEUED_LINE).assertExists()
        composeRule.onNode(
            hasTestTag(TEST_TAG_OVERLAY_QUEUED_LINE) and hasAnyDescendant(hasText("Check bluetooth too", substring = true))
        ).assertExists()
    }

    // ========== PR #614: Stop button visibility ==========

    @Test
    fun `mini chat shows Stop button when executing`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Stop").assertIsDisplayed()
    }

    @Test
    fun `mini chat hides Stop button when idle`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Stop").assertDoesNotExist()
    }

    // ========== PR #614: Mode control buttons ==========

    @Test
    fun `mini chat has Full and Island mode buttons`() {
        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = idleStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Full").assertIsDisplayed()
        composeRule.onNodeWithText("Island").assertIsDisplayed()
    }

    // ========== PR #614: Auto-scroll ==========
    // TODO: Auto-scroll uses a two-pass LaunchedEffect with delays (yield + 100ms + 300ms).
    // Testing this requires advanceTimeBy() with a TestDispatcher injected into the
    // composable's coroutine scope. The current OverlayMiniChatContent hardcodes
    // kotlinx.coroutines.delay(), making it difficult to test without refactoring
    // the delay injection. Specific scenarios to test when infra supports it:
    //   - scrollState.value reaches scrollState.maxValue after lines.size changes
    //   - scrollState updates again after second 300ms pass (for late-measuring markdown)
    //   - scroll triggers on lastLineText change (streaming content updates)
}
