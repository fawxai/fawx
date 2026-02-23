package ai.citros.chat

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayState
import kotlin.test.Test
import kotlin.test.assertEquals

class OverlayInteractionDemandTest {

    @Test
    fun `stopped run does not pin interaction demand from stale question line`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.STOPPED,
                systemLine = "Should I tap continue?"
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.NONE, demand)
    }

    @Test
    fun `failed run always requires error action`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.FAILED,
                systemLine = "Execution failed"
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.ERROR_ACTION_REQUIRED, demand)
    }

    @Test
    fun `idle run does not pin interaction demand from stale question line`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.IDLE,
                systemLine = "Should I tap continue?"
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.NONE, demand)
    }

    @Test
    fun `completed run does not force permission demand from stale status`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.COMPLETED,
                systemLine = "Please enable notification access"
            ),
            toolStatus = "notification access required"
        )

        assertEquals(OverlayInteractionDemand.NONE, demand)
    }

    @Test
    fun `executing run with permission keywords requires permission action`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.EXECUTING,
                systemLine = "Need accessibility permission"
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.PERMISSION_REQUIRED, demand)
    }

    @Test
    fun `executing run can require permission action from tool status only`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.EXECUTING,
                systemLine = "Working through the next step"
            ),
            toolStatus = "Please grant access to continue"
        )

        assertEquals(OverlayInteractionDemand.PERMISSION_REQUIRED, demand)
    }

    @Test
    fun `executing run with no system lines and no tool status returns none`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.EXECUTING,
                lines = emptyList()
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.NONE, demand)
    }

    @Test
    fun `executing run only inspects latest system line for demand`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.EXECUTING,
                lines = listOf(
                    OverlayLine(id = 1, type = OverlayLineType.SYSTEM, text = "Need accessibility permission"),
                    OverlayLine(id = 2, type = OverlayLineType.SYSTEM, text = "Continuing execution")
                )
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.NONE, demand)
    }

    @Test
    fun `executing run with question requires input action`() {
        val demand = deriveOverlayInteractionDemand(
            overlayState = state(
                runState = OverlayRunState.EXECUTING,
                systemLine = "Which option should I pick?"
            ),
            toolStatus = null
        )

        assertEquals(OverlayInteractionDemand.INPUT_REQUIRED, demand)
    }

    private fun state(
        runState: OverlayRunState,
        systemLine: String = "system"
    ): OverlayState = state(
        runState = runState,
        lines = listOf(OverlayLine(id = 1, type = OverlayLineType.SYSTEM, text = systemLine))
    )

    private fun state(
        runState: OverlayRunState,
        lines: List<OverlayLine>
    ): OverlayState = OverlayState(
        runState = runState,
        steps = emptyList(),
        lines = lines,
        currentStepIndex = 0,
        totalSteps = 0
    )
}
