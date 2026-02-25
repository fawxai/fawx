package ai.citros.chat

import ai.citros.core.PillAction
import ai.citros.core.PillStyle
import ai.citros.core.PolicyReasonCode
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertIs

@RunWith(RobolectricTestRunner::class)
class RuntimeActionPillMapperTest {

    @Test
    fun `standard confirmation maps to yes no and steer pills`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-1",
            reason = "Send a notification reply"
        )

        assertEquals(listOf("Yes", "No", "Do something else"), pills.map { it.label })
        assertEquals(PillStyle.PRIMARY, pills[0].style)
        assertEquals(PillStyle.DANGER, pills[1].style)
        assertEquals(PillStyle.SUBTLE, pills[2].style)
        assertIs<PillAction.Approve>(pills[0].action)
        assertIs<PillAction.Deny>(pills[1].action)
        assertIs<PillAction.Steer>(pills[2].action)
    }

    @Test
    fun `sensitive confirmation reason maps to allow once set`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-2",
            reason = "Sensitive app interaction requires confirmation"
        )

        assertEquals(
            listOf("Allow once", "Deny", "Always deny for this app"),
            pills.map { it.label }
        )
        assertIs<PillAction.Approve>(pills[0].action)
        assertIs<PillAction.Deny>(pills[1].action)
        assertIs<PillAction.Deny>(pills[2].action)
    }

    @Test
    fun `sensitive reason code maps to sensitive set even when reason text is generic`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-sensitive-code",
            reason = "Need approval",
            reasonCode = PolicyReasonCode.CONFIRM_SENSITIVE_APP
        )

        assertEquals(
            listOf("Allow once", "Deny", "Always deny for this app"),
            pills.map { it.label }
        )
    }

    @Test
    fun `first use reason maps to continue set`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-3",
            reason = "First time acting in 'com.example.app' this session"
        )

        assertEquals(
            listOf("Continue", "Not now", "Never for this app"),
            pills.map { it.label }
        )
        assertIs<PillAction.Approve>(pills[0].action)
        assertIs<PillAction.Deny>(pills[1].action)
        assertIs<PillAction.Deny>(pills[2].action)
    }

    @Test
    fun `first use reason code maps to continue set`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-3b",
            reason = "Need approval",
            reasonCode = PolicyReasonCode.CONFIRM_FIRST_USE_APP
        )

        assertEquals(
            listOf("Continue", "Not now", "Never for this app"),
            pills.map { it.label }
        )
    }

    @Test
    fun `financial reason maps to authenticate and deny`() {
        val pills = RuntimeActionPillMapper.policyConfirmationPills(
            requestId = "req-4",
            reason = "Financial transfer requires authenticate before proceeding"
        )

        assertEquals(listOf("Authenticate & allow", "Deny"), pills.map { it.label })
        assertIs<PillAction.Authenticate>(pills[0].action)
        assertIs<PillAction.Deny>(pills[1].action)
    }

    @Test
    fun `offer choices maps each choice to steer pill`() {
        val pills = RuntimeActionPillMapper.offerChoicePills(listOf("Maps", "Chrome"))

        assertEquals(listOf("Maps", "Chrome"), pills.map { it.label })
        assertEquals(PillStyle.DEFAULT, pills[0].style)
        assertIs<PillAction.Steer>(pills[0].action)
        assertEquals("Maps", (pills[0].action as PillAction.Steer).message)
    }

    @Test
    fun `error recovery pills include cancel`() {
        val pills = RuntimeActionPillMapper.errorRecoveryPills()

        assertEquals(listOf("Try again", "Do something else", "Cancel"), pills.map { it.label })
        assertIs<PillAction.Steer>(pills[0].action)
        assertIs<PillAction.Steer>(pills[1].action)
        assertIs<PillAction.Cancel>(pills[2].action)
        assertEquals(PillStyle.DANGER, pills[2].style)
    }
}
