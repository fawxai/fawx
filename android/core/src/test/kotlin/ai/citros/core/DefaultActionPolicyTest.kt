package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class DefaultActionPolicyTest {

    private val policy = DefaultActionPolicy()

    @Test
    fun `known allow tool remains allow outside first-use`() {
        assertIs<PolicyDecision.Allow>(policy.evaluate(ToolCall("1", "think", emptyMap()), PolicyContext()).decision)
    }

    @Test
    fun `offer choices is allow by default`() {
        val eval = policy.evaluate(
            ToolCall("1", "offer_choices", mapOf("question" to "Pick one", "choices" to listOf("A", "B"))),
            PolicyContext()
        )
        assertIs<PolicyDecision.Allow>(eval.decision)
        assertEquals(PolicyReasonCode.ALLOW_DEFAULT, eval.reasonCode)
    }

    @Test
    fun `known confirm tool returns stable reason code`() {
        val decision = policy.evaluate(ToolCall("1", "reply_notification", mapOf("text" to "ok")), PolicyContext()).decision as PolicyDecision.Confirm
        assertEquals(PolicyReasonCode.defaultConfirmForTool("reply_notification"), decision.reasonCode)
    }

    @Test
    fun `unknown tool is confirm by default`() {
        val decision = policy.evaluate(ToolCall("1", "unknown_tool", emptyMap()), PolicyContext()).decision as PolicyDecision.Confirm
        assertEquals(PolicyReasonCode.CONFIRM_UNKNOWN_TOOL, decision.reasonCode)
    }

    @Test
    fun `phase1 deny tools are denied`() {
        val decision = policy.evaluate(ToolCall("1", "root_shell", emptyMap()), PolicyContext()).decision as PolicyDecision.Deny
        assertEquals(PolicyReasonCode.DENY_PHASE1_TOOL, decision.reasonCode)
    }

    @Test
    fun `first open_app action confirms and second allows`() {
        val first = policy.evaluate(ToolCall("1", "open_app", mapOf("app_package" to "com.whatsapp")), PolicyContext())
        val second = policy.evaluate(ToolCall("2", "open_app", mapOf("app_package" to "com.whatsapp.beta")), PolicyContext())
        assertEquals(PolicyReasonCode.CONFIRM_FIRST_USE_APP, (first.decision as PolicyDecision.Confirm).reasonCode)
        assertIs<PolicyDecision.Allow>(second.decision)
    }

    @Test
    fun `sensitive app interaction escalates allow to confirm`() {
        policy.evaluate(ToolCall("seed", "open_app", mapOf("app_package" to "com.whatsapp")), PolicyContext())
        val eval = policy.evaluate(
            ToolCall("1", "tap", mapOf("x" to 1, "y" to 1)),
            PolicyContext(foregroundApp = "com.whatsapp", appIdentifier = "com.whatsapp")
        )
        assertEquals(PolicyReasonCode.CONFIRM_SENSITIVE_APP, (eval.decision as PolicyDecision.Confirm).reasonCode)
    }

    @Test
    fun `financial submit in financial app is denied`() {
        val eval = policy.evaluate(
            ToolCall("1", "tap_text", mapOf("text" to "Send transfer")),
            PolicyContext(foregroundApp = "com.venmo", appIdentifier = "com.venmo")
        )
        val deny = assertIs<PolicyDecision.Deny>(eval.decision)
        assertEquals(PolicyReasonCode.DENY_FINANCIAL_SUBMIT, deny.reasonCode)
    }

    @Test
    fun `financial submit in degraded context is denied`() {
        val eval = policy.evaluate(
            ToolCall("1", "tap_text", mapOf("text" to "Confirm payment")),
            PolicyContext(
                foregroundApp = null,
                appIdentifier = null,
                screenContentSummary = "Wallet transfer confirmation page",
                targetNodeHints = listOf("primary button")
            )
        )
        val deny = assertIs<PolicyDecision.Deny>(eval.decision)
        assertEquals(PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT, deny.reasonCode)
    }

    @Test
    fun `app-targeted action without identity fails closed to confirm`() {
        val decision = policy.evaluate(ToolCall("1", "tap", mapOf("x" to 1, "y" to 1)), PolicyContext()).decision as PolicyDecision.Confirm
        assertEquals(PolicyReasonCode.CONFIRM_MISSING_APP_TARGET, decision.reasonCode)
    }

    @Test
    fun `egress denied when allowlist signature missing`() {
        val d = policy.evaluate(ToolCall("1", "web_fetch", mapOf("url" to "https://example.com/x")), PolicyContext()).decision
        assertEquals(PolicyReasonCode.DENY_EGRESS_UNSIGNED_ALLOWLIST, (d as PolicyDecision.Deny).reasonCode)
    }

    @Test
    fun `egress denied for malformed url`() {
        val d = policy.evaluate(ToolCall("1", "web_fetch", mapOf("url" to "https://")), PolicyContext()).decision
        assertEquals(PolicyReasonCode.DENY_EGRESS_MALFORMED_URL, (d as PolicyDecision.Deny).reasonCode)
    }

    @Test
    fun `egress denied for non https scheme`() {
        val d = policy.evaluate(ToolCall("1", "web_fetch", mapOf("url" to "http://example.com/x")), PolicyContext()).decision
        assertEquals(PolicyReasonCode.DENY_EGRESS_INSECURE_SCHEME, (d as PolicyDecision.Deny).reasonCode)
    }

    @Test
    fun `egress allowed for signed allowlisted endpoint`() {
        val p = DefaultActionPolicy(
            egressAllowlistProvider = object : EgressAllowlistProvider {
                override fun currentSnapshot(): EgressAllowlistSnapshot = EgressAllowlistSnapshot(
                    hosts = setOf("example.com"),
                    version = "v1",
                    signatureVerified = true,
                    appliedAtMs = 1L
                )
            }
        )
        val eval = p.evaluate(ToolCall("1", "web_fetch", mapOf("url" to "https://api.example.com/x")), PolicyContext())
        assertIs<PolicyDecision.Allow>(eval.decision)
        assertEquals(PolicyReasonCode.ALLOW_EGRESS_ALLOWLISTED, eval.reasonCode)
    }

    @Test
    fun `non app-targeted tool never sets first use observed`() {
        val eval = policy.evaluate(ToolCall("1", "wait", emptyMap()), PolicyContext(foregroundApp = "com.example.app"))
        assertFalse(eval.firstUseObserved)
    }

    @Test
    fun `egress denied when url missing`() {
        val d = policy.evaluate(ToolCall("1", "web_fetch", emptyMap()), PolicyContext()).decision
        assertEquals(PolicyReasonCode.DENY_EGRESS_MISSING_URL, (d as PolicyDecision.Deny).reasonCode)
    }

    @Test
    fun `web_search remains allow without model-supplied endpoint`() {
        val eval = policy.evaluate(ToolCall("1", "web_search", mapOf("query" to "openclaw docs")), PolicyContext())
        assertIs<PolicyDecision.Allow>(eval.decision)
        assertEquals(PolicyReasonCode.ALLOW_DEFAULT, eval.reasonCode)
    }

    @Test
    fun `global rate limit returns rate limited`() {
        val p = DefaultActionPolicy(clock = { 1000L })
        repeat(31) { p.evaluate(ToolCall("$it", "think", emptyMap()), PolicyContext()) }
        val eval = p.evaluate(ToolCall("x", "think", emptyMap()), PolicyContext())
        assertIs<PolicyDecision.RateLimited>(eval.decision)
        assertEquals(PolicyReasonCode.RATE_LIMIT_GLOBAL, (eval.decision as PolicyDecision.RateLimited).reasonCode)
    }

    @Test
    fun `message rate limit returns message specific reason`() {
        var now = 1_000L
        val p = DefaultActionPolicy(clock = { now })
        repeat(5) {
            p.evaluate(ToolCall("m$it", "reply_notification", mapOf("text" to "ok")), PolicyContext())
            now += 1_000L
        }
        val eval = p.evaluate(ToolCall("m6", "reply_notification", mapOf("text" to "ok")), PolicyContext())
        val limited = assertIs<PolicyDecision.RateLimited>(eval.decision)
        assertEquals(PolicyReasonCode.RATE_LIMIT_MESSAGES, limited.reasonCode)
    }

    @Test
    fun `user override can escalate allow to confirm`() {
        val p = DefaultActionPolicy(
            store = object : ActionPolicyStore {
                override fun getOverride(toolName: String): PolicyOverrideLevel? =
                    if (toolName == "think") PolicyOverrideLevel.CONFIRM else null

                override fun setOverride(toolName: String, decision: PolicyOverrideLevel?) = Unit
                override fun getAllOverrides(): Map<String, PolicyOverrideLevel> = emptyMap()
            }
        )
        val decision = p.evaluate(ToolCall("1", "think", emptyMap()), PolicyContext()).decision
        assertEquals(PolicyReasonCode.CONFIRM_USER_OVERRIDE, (decision as PolicyDecision.Confirm).reasonCode)
    }

    @Test
    fun `user override can force deny`() {
        val p = DefaultActionPolicy(
            store = object : ActionPolicyStore {
                override fun getOverride(toolName: String): PolicyOverrideLevel? =
                    if (toolName == "think") PolicyOverrideLevel.DENY else null

                override fun setOverride(toolName: String, decision: PolicyOverrideLevel?) = Unit
                override fun getAllOverrides(): Map<String, PolicyOverrideLevel> = emptyMap()
            }
        )
        val decision = p.evaluate(ToolCall("1", "think", emptyMap()), PolicyContext()).decision
        assertEquals(PolicyReasonCode.DENY_USER_OVERRIDE, (decision as PolicyDecision.Deny).reasonCode)
    }
}
