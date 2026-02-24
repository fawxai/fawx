package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

class PermissiveActionPolicyTest {

    @Test
    fun `evaluate always allows with permissive bypass reason code`() {
        val evaluation = PermissiveActionPolicy.evaluate(
            toolCall = ToolCall(id = "1", name = "tap", input = emptyMap()),
            context = PolicyContext()
        )

        assertIs<PolicyDecision.Allow>(evaluation.decision)
        assertEquals(PolicyReasonCode.ALLOW_PERMISSIVE_BYPASS, evaluation.reasonCode)
    }
}
