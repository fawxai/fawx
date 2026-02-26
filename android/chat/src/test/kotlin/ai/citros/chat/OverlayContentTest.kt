package ai.citros.chat

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayStep
import ai.citros.core.ActionPill
import ai.citros.core.PillAction
import ai.citros.core.PillStyle
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.text.TextLayoutResult
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertTrue
import kotlin.test.assertEquals
import kotlin.test.assertNotNull

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
    fun `mini chat renders runtime pills and dispatches tap action`() {
        var tappedAction: PillAction? = null
        val pills = listOf(
            ActionPill(
                id = "approve",
                label = "Allow once",
                style = PillStyle.PRIMARY,
                action = PillAction.Approve("req-1")
            )
        )

        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = emptyList(),
                actionPills = pills,
                onActionPillTap = { tappedAction = it.action },
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithTag("runtime_pill_approve").assertIsDisplayed()
            .performSemanticsAction(SemanticsActions.OnClick)
        assertNotNull(tappedAction)
        assertTrue(tappedAction is PillAction.Approve)
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
        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.QUEUED, 1)).assertExists()
        composeRule.onNode(
            hasTestTag(overlayLineTestTag(OverlayLineType.QUEUED, 1)) and hasAnyDescendant(hasText("Check bluetooth too", substring = true))
        ).assertExists()
    }

    @Test
    fun `mini chat assigns line-type tags for system user and queued bubbles`() {
        val lines = listOf(
            OverlayLine(id = 1, type = OverlayLineType.SYSTEM, text = "system line"),
            OverlayLine(id = 2, type = OverlayLineType.USER, text = "user line"),
            OverlayLine(id = 3, type = OverlayLineType.QUEUED, text = "queued line")
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

        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.SYSTEM, 1)).assertExists()
        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.USER, 2)).assertExists()
        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.QUEUED, 3)).assertExists()
    }

    @Test
    fun `mini chat assigns unique tags when multiple user lines exist`() {
        val lines = listOf(
            OverlayLine(id = 10, type = OverlayLineType.USER, text = "first"),
            OverlayLine(id = 11, type = OverlayLineType.USER, text = "second")
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

        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.USER, 10)).assertExists()
        composeRule.onNodeWithTag(overlayLineTestTag(OverlayLineType.USER, 11)).assertExists()
    }

    @Test
    fun `mini chat user bubble aligns toward end`() {
        val flavor = CitrosFlavor.LIME
        val lines = listOf(
            OverlayLine(id = 1, type = OverlayLineType.USER, text = "user line")
        )

        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = flavor,
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

        val userBubbleBounds = composeRule
            .onNodeWithTag(overlayLineTestTag(OverlayLineType.USER, 1))
            .fetchSemanticsNode()
            .boundsInRoot
        val rootBounds = composeRule.onRoot().fetchSemanticsNode().boundsInRoot

        assertTrue(
            userBubbleBounds.center.x > rootBounds.center.x,
            "Expected user bubble to render on the end/right side of the row"
        )
    }

    @Test
    fun `mini chat system and queued bubbles align toward start`() {
        val flavor = CitrosFlavor.LIME

        composeRule.setContent {
            OverlayMiniChatContent(
                flavor = flavor,
                runState = OverlayRunState.EXECUTING,
                currentStep = executingStep,
                lines = listOf(
                    OverlayLine(id = 1, type = OverlayLineType.SYSTEM, text = "system line"),
                    OverlayLine(id = 2, type = OverlayLineType.QUEUED, text = "queued line")
                ),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                onStopAction = {},
                onResumeOrRetry = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        val systemBubbleBounds = composeRule
            .onNodeWithTag(overlayLineTestTag(OverlayLineType.SYSTEM, 1))
            .fetchSemanticsNode()
            .boundsInRoot
        val queuedBubbleBounds = composeRule
            .onNodeWithTag(overlayLineTestTag(OverlayLineType.QUEUED, 2))
            .fetchSemanticsNode()
            .boundsInRoot
        val rootBounds = composeRule.onRoot().fetchSemanticsNode().boundsInRoot

        assertTrue(
            systemBubbleBounds.center.x < rootBounds.center.x,
            "Expected system bubble to render on the start/left side of the row"
        )
        assertTrue(
            queuedBubbleBounds.center.x < rootBounds.center.x,
            "Expected queued bubble to render on the start/left side of the row"
        )
    }



    @Test
    fun `dynamic island long executing status remains single line and ellipsized`() {
        val longStatus = "Executing very long action name with additional progress detail that should truncate in dynamic island ".repeat(8)
        var layoutResult: TextLayoutResult? = null

        composeRule.setContent {
            OverlayDynamicIslandContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStepLabel = longStatus,
                unreadCount = 0,
                onExpand = {},
                onStopAction = {},
                onDismiss = {}
            )
        }

        composeRule.onNodeWithTag("dynamic_island_status_text", useUnmergedTree = true)
            .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { action ->
                val results = mutableListOf<TextLayoutResult>()
                action(results)
                if (results.isNotEmpty()) {
                    layoutResult = results.first()
                }
            }

        composeRule.runOnIdle {
            val result = layoutResult
            assertNotNull(result, "Expected text layout result for dynamic island status text")
            assertEquals(1, result.lineCount, "Dynamic island status text should stay single line")
            assertTrue(result.getLineEnd(0, visibleEnd = true) < longStatus.length, "Dynamic island status text should truncate long content")
        }
    }

    @Test
    fun `dynamic island renders expected run state visuals for executing stopped and failed`() {
        val runState = androidx.compose.runtime.mutableStateOf(OverlayRunState.EXECUTING)
        val currentLabel = androidx.compose.runtime.mutableStateOf("Running tool")

        composeRule.setContent {
            OverlayDynamicIslandContent(
                flavor = CitrosFlavor.LIME,
                runState = runState.value,
                currentStepLabel = currentLabel.value,
                unreadCount = 0,
                onExpand = {},
                onStopAction = {},
                onDismiss = {}
            )
        }
        composeRule.onNodeWithText("Running tool", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithContentDescription("Dynamic island overlay").assertExists()

        composeRule.runOnIdle {
            runState.value = OverlayRunState.STOPPED
            currentLabel.value = ""
        }
        composeRule.onNodeWithText("Tap to resume", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithContentDescription("Dynamic island overlay").assertExists()

        composeRule.runOnIdle {
            runState.value = OverlayRunState.FAILED
        }
        composeRule.onNodeWithText("Tap to open settings", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithContentDescription("Dynamic island overlay").assertExists()
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
