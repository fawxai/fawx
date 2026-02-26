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
    fun `permission grant consumes pending token and starts listening when manual request is in flight`() {
        val result = handleVoiceStartPermissionResult(
            state = VoiceStartRetryState(
                lastConsumedToken = 2,
                pendingToken = 6,
                permissionRequestedToken = 6
            ),
            granted = true,
            manualPermissionRequestInFlight = true,
            hasActiveStt = true
        )

        assertEquals(true, result.shouldBeginListeningNow)
        assertEquals(6, result.state.lastConsumedToken)
        assertEquals(0, result.state.pendingToken)
        assertEquals(0, result.state.permissionRequestedToken)
    }

    @Test
    fun `permission grant keeps pending token when provider is not ready`() {
        val result = handleVoiceStartPermissionResult(
            state = VoiceStartRetryState(
                lastConsumedToken = 2,
                pendingToken = 6,
                permissionRequestedToken = 6
            ),
            granted = true,
            manualPermissionRequestInFlight = true,
            hasActiveStt = false
        )

        assertEquals(false, result.shouldBeginListeningNow)
        assertEquals(2, result.state.lastConsumedToken)
        assertEquals(6, result.state.pendingToken)
        assertEquals(0, result.state.permissionRequestedToken)
    }

    @Test
    fun `permission denial keeps pending token and does not start listening`() {
        val result = handleVoiceStartPermissionResult(
            state = VoiceStartRetryState(
                lastConsumedToken = 2,
                pendingToken = 6,
                permissionRequestedToken = 6
            ),
            granted = false,
            manualPermissionRequestInFlight = true,
            hasActiveStt = true
        )

        assertEquals(false, result.shouldBeginListeningNow)
        assertEquals(2, result.state.lastConsumedToken)
        assertEquals(6, result.state.pendingToken)
        assertEquals(6, result.state.permissionRequestedToken)
    }

    @Test
    fun `permission grant without manual request in flight clears request and waits for retry path`() {
        val result = handleVoiceStartPermissionResult(
            state = VoiceStartRetryState(
                lastConsumedToken = 2,
                pendingToken = 6,
                permissionRequestedToken = 6
            ),
            granted = true,
            manualPermissionRequestInFlight = false,
            hasActiveStt = true
        )

        assertEquals(false, result.shouldBeginListeningNow)
        assertEquals(2, result.state.lastConsumedToken)
        assertEquals(6, result.state.pendingToken)
        assertEquals(0, result.state.permissionRequestedToken)
    }

    @Test
    fun `dispatch request permission action signals permission prompt only`() {
        val result = dispatchVoiceStartRetryAction(
            state = VoiceStartRetryState(lastConsumedToken = 2, pendingToken = 6, permissionRequestedToken = 6),
            action = VoiceStartRetryAction.REQUEST_PERMISSION,
            hasActiveSttAtLaunch = true
        )

        assertEquals(false, result.shouldBeginListening)
        assertEquals(true, result.shouldRequestPermission)
        assertEquals(2, result.state.lastConsumedToken)
        assertEquals(6, result.state.pendingToken)
        assertEquals(6, result.state.permissionRequestedToken)
    }

    @Test
    fun `dispatch none action produces no side effects`() {
        val state = VoiceStartRetryState(lastConsumedToken = 2, pendingToken = 6, permissionRequestedToken = 6)
        val result = dispatchVoiceStartRetryAction(
            state = state,
            action = VoiceStartRetryAction.NONE,
            hasActiveSttAtLaunch = true
        )

        assertEquals(false, result.shouldBeginListening)
        assertEquals(false, result.shouldRequestPermission)
        assertEquals(state, result.state)
    }

    @Test
    fun `launch race requeues token so next retry tick can execute`() {
        val decision = resolveVoiceStartRetry(
            state = VoiceStartRetryState(),
            incomingToken = 21,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, decision.action)

        val launchRace = dispatchVoiceStartRetryAction(
            state = decision.state,
            action = decision.action,
            hasActiveSttAtLaunch = false
        )
        assertEquals(false, launchRace.shouldBeginListening)
        assertEquals(21, launchRace.state.pendingToken)
        assertEquals(20, launchRace.state.lastConsumedToken)

        val retried = resolveVoiceStartRetry(
            state = launchRace.state,
            incomingToken = 21,
            isLoading = false,
            isListening = false,
            hasVoiceManager = true,
            hasActiveStt = true,
            hasMicPermission = true
        )
        assertEquals(VoiceStartRetryAction.START_LISTENING, retried.action)
        assertEquals(21, retried.state.lastConsumedToken)
        assertEquals(0, retried.state.pendingToken)
    }

    @Test
    fun `launch race with zero-state tokens remains bounded`() {
        val launchRace = dispatchVoiceStartRetryAction(
            state = VoiceStartRetryState(),
            action = VoiceStartRetryAction.START_LISTENING,
            hasActiveSttAtLaunch = false
        )

        assertEquals(false, launchRace.shouldBeginListening)
        assertEquals(false, launchRace.shouldRequestPermission)
        assertEquals(0, launchRace.state.pendingToken)
        assertEquals(0, launchRace.state.lastConsumedToken)
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
