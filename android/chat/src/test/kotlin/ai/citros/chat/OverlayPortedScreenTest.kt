package ai.citros.chat

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextInput
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayStep
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Overlay composable tests (#300, #301).
 *
 * Tests cover rendering, click interactions, IME actions, draft clearing,
 * and whitespace trimming.
 */
@RunWith(RobolectricTestRunner::class)
class OverlayPortedScreenTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val defaultStep = OverlayStep(step = 1, total = 5, label = "Testing")

    // ========== MiniChat Rendering ==========

    @Test
    fun `mini chat renders message input with executing placeholder`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Message input").assertExists()
        composeRule.onNodeWithText("Steer or queue...", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat renders idle placeholder when not executing`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.IDLE,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Message...", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat renders idle placeholder when completed`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.COMPLETED,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Message...", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat send button disabled when draft is blank`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").assertIsNotEnabled()
    }

    @Test
    fun `mini chat send button enabled when draft has text`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "hello",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").assertIsEnabled()
    }

    @Test
    fun `mini chat send button disabled for whitespace-only draft`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "   ",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").assertIsNotEnabled()
    }

    @Test
    fun `mini chat displays current step`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = OverlayStep(step = 1, total = 5, label = "Opening Settings"),
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Opening Settings", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat shows resume banner and input when stopped`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Stopped"),
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = true,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // Banner shows with Resume button
        composeRule.onAllNodesWithText("Stopped").assertCountEquals(2)
        composeRule.onNodeWithText("Resume").assertExists()
        // Input is still visible (never hidden)
        composeRule.onNodeWithContentDescription("Message input").assertExists()
        composeRule.onNodeWithText("Message...", useUnmergedTree = true).assertExists()
    }

    @Test
    fun `mini chat shows failed banner when run failed`() {
        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.FAILED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Failed"),
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Failed").assertExists()
        composeRule.onNodeWithText("Retry").assertExists()
        composeRule.onNodeWithContentDescription("Message input").assertExists()
    }

    @Test
    fun `mini chat resume banner fires undo callback`() {
        var resumed = false

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Stopped"),
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = {},
                isUndoStopVisible = true,
                onResumeOrRetry = { resumed = true },
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Resume").performClick()
        assertTrue(resumed, "Resume should fire onResumeOrRetry callback")
    }

    @Test
    fun `mini chat resume banner dismissed when message sent`() {
        var undoVisible by mutableStateOf(true)

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Stopped"),
                lines = emptyList(),
                queuedMessageDraft = "new message",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = { undoVisible = false },
                isUndoStopVisible = undoVisible,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // Banner visible initially
        composeRule.onAllNodesWithText("Stopped").assertCountEquals(2)
        composeRule.onNodeWithText("Resume").assertExists()

        // Send a message
        composeRule.onNodeWithText("Send").performClick()
        composeRule.waitForIdle()

        // Banner should be gone
        assertTrue(!undoVisible, "isUndoStopVisible should be false after send")
        composeRule.onAllNodesWithText("Stopped").assertCountEquals(1)
        composeRule.onNodeWithText("Resume").assertDoesNotExist()

        // Input still visible
        composeRule.onNodeWithContentDescription("Message input").assertExists()
    }

    // ========== MiniChat Click Interactions ==========

    @Test
    fun `mini chat send button fires callback on click`() {
        var submitted = false

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "hello",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = { submitted = true },
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").performClick()
        assertTrue(submitted, "onSubmitQueuedMessage should fire on Send click")
    }

    @Test
    fun `mini chat draft change callback fires on text input`() {
        var capturedDraft = ""

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "",
                onQueuedDraftChange = { capturedDraft = it },
                onSubmitQueuedMessage = {},
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithContentDescription("Message input")
            .performTextInput("test message")
        assertEquals("test message", capturedDraft)
    }

    // ========== FullApp Rendering ==========

    @Test
    fun `full app renders message input and send button`() {
        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Message input").assertExists()
        composeRule.onNodeWithContentDescription("Send message").assertExists()
    }

    @Test
    fun `full app send button disabled when draft is empty`() {
        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send message").assertIsNotEnabled()
    }

    @Test
    fun `full app send button enabled with text`() {
        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "test message",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send message").assertIsEnabled()
    }

    @Test
    fun `full app send button disabled for whitespace-only draft`() {
        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "   ",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send message").assertIsNotEnabled()
    }

    @Test
    fun `full app displays Citros header and return button`() {
        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithText("Citros").assertExists()
        composeRule.onNodeWithText("Return").assertExists()
    }

    // ========== FullApp Click Interactions ==========

    @Test
    fun `full app send button fires callback on click`() {
        var sent = false

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "test",
                onMessageDraftChange = {},
                onSendMessage = { sent = true },
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send message").performClick()
        assertTrue(sent, "onSendMessage should fire on Send click")
    }

    @Test
    fun `full app return button fires callback`() {
        var returned = false

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "",
                onMessageDraftChange = {},
                onSendMessage = {},
                onReturnToOverlay = { returned = true },
                onStopAction = {}
            )
        }

        composeRule.onNodeWithText("Return").performClick()
        assertTrue(returned, "onReturnToOverlay should fire on Return click")
    }

    // ========== IME Keyboard Action ==========

    @Test
    fun `full app IME send action fires callback`() {
        var sent = false

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = "test",
                onMessageDraftChange = {},
                onSendMessage = { sent = true },
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Message input").performImeAction()
        assertTrue(sent, "IME Send should fire onSendMessage")
    }

    @Test
    fun `mini chat IME send action fires callback`() {
        var submitted = false

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "follow-up",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = { submitted = true },
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        // Find the TextField by its placeholder text (unmerged tree to reach it)
        composeRule.onNodeWithContentDescription("Message input")
            .performImeAction()
        assertTrue(submitted, "IME Send should fire onSubmitQueuedMessage")
    }

    // ========== Draft Clearing After Send (Integration with stateful wrapper) ==========

    @Test
    fun `full app draft clears after send via stateful wrapper`() {
        var currentDraft by mutableStateOf("hello world")
        var sentMessage = ""

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = currentDraft,
                onMessageDraftChange = { currentDraft = it },
                onSendMessage = {
                    sentMessage = currentDraft.trim()
                    currentDraft = ""
                },
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        composeRule.onNodeWithContentDescription("Send message").performClick()
        composeRule.waitForIdle()

        assertEquals("hello world", sentMessage)
        assertEquals("", currentDraft)
        // Send button should now be disabled since draft is empty
        composeRule.onNodeWithContentDescription("Send message").assertIsNotEnabled()
    }

    @Test
    fun `mini chat draft clears after send via stateful wrapper`() {
        var currentDraft by mutableStateOf("follow-up message")
        var sentMessage = ""

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = currentDraft,
                onQueuedDraftChange = { currentDraft = it },
                onSubmitQueuedMessage = {
                    sentMessage = currentDraft.trim()
                    currentDraft = ""
                },
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").performClick()
        composeRule.waitForIdle()

        assertEquals("follow-up message", sentMessage)
        assertEquals("", currentDraft)
        composeRule.onNodeWithText("Send").assertIsNotEnabled()
    }

    // ========== Whitespace Trimming ==========

    @Test
    fun `full app whitespace-only draft keeps send disabled`() {
        var currentDraft by mutableStateOf("   ")
        var sendCalled = false

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = currentDraft,
                onMessageDraftChange = { currentDraft = it },
                onSendMessage = { sendCalled = true },
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        // Button should be disabled for whitespace-only
        composeRule.onNodeWithContentDescription("Send message").assertIsNotEnabled()
        // sendCalled should remain false — can't click disabled button
        assertEquals(false, sendCalled)
    }

    @Test
    fun `mini chat whitespace-only draft keeps send disabled`() {
        var sendCalled = false

        composeRule.setContent {
            MiniChatOverlayCard(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.EXECUTING,
                currentStep = defaultStep,
                lines = emptyList(),
                queuedMessageDraft = "   ",
                onQueuedDraftChange = {},
                onSubmitQueuedMessage = { sendCalled = true },
                isUndoStopVisible = false,
                onResumeOrRetry = {},
                onStopAction = {},
                onOpenFull = {},
                onOpenIsland = {}
            )
        }

        composeRule.onNodeWithText("Send").assertIsNotEnabled()
        assertEquals(false, sendCalled)
    }

    @Test
    fun `full app padded draft sends trimmed content`() {
        var currentDraft by mutableStateOf("  hello  ")
        var sentMessage = ""

        composeRule.setContent {
            FullAppOverlayContent(
                flavor = CitrosFlavor.LIME,
                runState = OverlayRunState.STOPPED,
                currentStep = OverlayStep(step = 0, total = 0, label = "Ready"),
                lines = emptyList(),
                messageDraft = currentDraft,
                onMessageDraftChange = { currentDraft = it },
                onSendMessage = {
                    sentMessage = currentDraft.trim()
                    currentDraft = ""
                },
                onReturnToOverlay = {},
                onStopAction = {}
            )
        }

        // Button should be enabled (non-blank after trim)
        composeRule.onNodeWithContentDescription("Send message").assertIsEnabled()
        composeRule.onNodeWithContentDescription("Send message").performClick()
        composeRule.waitForIdle()

        assertEquals("hello", sentMessage, "Message should be trimmed")
        assertEquals("", currentDraft, "Draft should be cleared")
    }
}
