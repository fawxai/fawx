package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals

class VoiceStartRetryStateTest {

    @Test
    fun `loading gate keeps token pending and retries when loading clears`() {
        val blocked = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 5,
            isLoading = true,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.NONE, blocked.action)
        assertEquals(5, blocked.state.pendingToken)
        assertEquals(0, blocked.state.lastConsumedToken)

        val retried = resolveVoiceStartRetry(
            state = blocked.state,
            incomingToken = 5,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(5, retried.state.lastConsumedToken)
        assertEquals(0, retried.state.pendingToken)
    }

    @Test
    fun `listening gate keeps token pending and retries when listening stops`() {
        val blocked = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 7,
            isLoading = false,
            isListening = true,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.NONE, blocked.action)
        assertEquals(7, blocked.state.pendingToken)

        val retried = resolveVoiceStartRetry(
            state = blocked.state,
            incomingToken = 7,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(7, retried.state.lastConsumedToken)
    }

    @Test
    fun `permission request is emitted once and retried after grant`() {
        val first = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 9,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = false
        )
        assertEquals(VoiceStartRetryAction.REQUEST_PERMISSION, first.action)
        assertEquals(9, first.state.permissionRequestedToken)
        assertEquals(9, first.state.pendingToken)

        val duplicate = resolveVoiceStartRetry(
            state = first.state,
            incomingToken = 9,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = false
        )
        assertEquals(VoiceStartRetryAction.NONE, duplicate.action)

        val afterGrant = applyVoiceStartPermissionResult(duplicate.state, granted = true)
        val retried = resolveVoiceStartRetry(
            state = afterGrant,
            incomingToken = 9,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(9, retried.state.lastConsumedToken)
        assertEquals(0, retried.state.pendingToken)
    }

    @Test
    fun `voice manager readiness gate retries when manager becomes available`() {
        val blocked = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 11,
            isLoading = false,
            isListening = false,
            hasVoiceManager = false,
            hasActiveStt = false,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.NONE, blocked.action)
        assertEquals(11, blocked.state.pendingToken)

        val retried = resolveVoiceStartRetry(
            state = blocked.state,
            incomingToken = 11,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(11, retried.state.lastConsumedToken)
    }

    @Test
    fun `active stt readiness gate keeps token pending and retries when provider becomes available`() {
        val blocked = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 13,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = false,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.NONE, blocked.action)
        assertEquals(13, blocked.state.pendingToken)

        val retried = resolveVoiceStartRetry(
            state = blocked.state,
            incomingToken = 13,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(13, retried.state.lastConsumedToken)
    }

    @Test
    fun `consumed token is ignored`() {
        val resolution = resolveVoiceStartRetry(
            state = VoiceStartRetryState(lastConsumedToken = 4),
            incomingToken = 4,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.NONE, resolution.action)
        assertEquals(4, resolution.state.lastConsumedToken)
        assertEquals(0, resolution.state.pendingToken)
    }
}
