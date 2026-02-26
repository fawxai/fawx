package ai.citros.core

import org.junit.After
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertSame

class ActionPolicyFactoryTest {

    @After
    fun cleanup() {
        FeatureFlags.resetToDefaults()
    }

    @Test
    fun `createConfiguredActionPolicy returns default policy when action policy flag enabled`() {
        FeatureFlags.actionPolicyEnabled = true

        val policy = createConfiguredActionPolicy()
        val decision = policy.evaluate(ToolCall("1", "root_shell", emptyMap()), PolicyContext()).decision

        val denyDecision = assertIs<PolicyDecision.Deny>(decision)
        assertEquals(PolicyReasonCode.DENY_PHASE1_TOOL, denyDecision.reasonCode)
    }

    @Test
    fun `createConfiguredActionPolicy returns permissive policy when action policy flag disabled`() {
        FeatureFlags.actionPolicyEnabled = false

        val policy = createConfiguredActionPolicy()
        val decision = policy.evaluate(ToolCall("1", "root_shell", emptyMap()), PolicyContext()).decision

        assertSame(PermissiveActionPolicy, policy)
        assertIs<PolicyDecision.Allow>(decision)
    }
}
