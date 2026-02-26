package ai.citros.chat

import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class MessageInputSubmissionTest {

    @Test
    fun voiceAutoSendBlockedRetainsTranscript() {
        val state = submitMessageDraft(
            attemptedText = "voice transcript",
            isLoading = false,
            onSend = { false },
            onQueue = { true }
        )

        assertEquals("voice transcript", state.text)
        assertFalse(state.pendingStopVisual)
    }

    @Test
    fun voiceAutoSendAllowedClearsDraftAndShowsStopVisual() {
        val state = submitMessageDraft(
            attemptedText = "voice transcript",
            isLoading = false,
            onSend = { true },
            onQueue = { true }
        )

        assertEquals("", state.text)
        assertTrue(state.pendingStopVisual)
    }

    @Test
    fun queuedSendRejectionRetainsDraftAndDoesNotShowStopVisual() {
        var sendCalled = false
        val state = submitMessageDraft(
            attemptedText = "queued follow-up",
            isLoading = true,
            onSend = {
                sendCalled = true
                true
            },
            onQueue = { false }
        )

        assertFalse(sendCalled)
        assertEquals("queued follow-up", state.text)
        assertFalse(state.pendingStopVisual)
    }
}
