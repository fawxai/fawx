package ai.citros.core

/** Default Phase-1 action policy: deny > rate-limit > confirm > allow. */
class DefaultActionPolicy(
    private val store: ActionPolicyStore? = null,
    private val egressAllowlistProvider: EgressAllowlistProvider = EmptyDenyEgressAllowlistProvider,
    private val clock: () -> Long = { System.currentTimeMillis() }
) : ActionPolicy {
    companion object {
        const val RATE_LIMIT_PER_MINUTE = 30
        const val MESSAGE_RATE_LIMIT = 5
        const val MESSAGE_RATE_WINDOW_MS = 120_000L

        val PHASE1_DENY_TOOLS = mapOf(
            "factory_reset" to "Factory reset is blocked in Phase 1",
            "disable_policy_engine" to "Policy engine cannot be disabled by agent actions",
            "modify_audit_log" to "Audit log modifications are blocked",
            "root_shell" to "Direct root-shell execution is blocked",
            "financial_transaction" to "Financial transactions are blocked in Phase 1"
        )

        val DEFAULT_POLICIES: Map<String, PolicyDecision> = mapOf(
            "tap" to PolicyDecision.Allow,
            "tap_text" to PolicyDecision.Allow,
            "type_text" to PolicyDecision.Allow,
            "swipe" to PolicyDecision.Allow,
            "scroll" to PolicyDecision.Allow,
            "long_press" to PolicyDecision.Allow,
            "press_back" to PolicyDecision.Allow,
            "press_home" to PolicyDecision.Allow,
            "open_app" to PolicyDecision.Allow,
            "open_notifications" to PolicyDecision.Allow,
            "read_screen" to PolicyDecision.Allow,
            "screenshot" to PolicyDecision.Allow,
            "think" to PolicyDecision.Allow,
            "wait" to PolicyDecision.Allow,
            "copy" to PolicyDecision.Allow,
            "paste" to PolicyDecision.Allow,
            "set_clipboard" to PolicyDecision.Allow,
            "read_notifications" to PolicyDecision.Allow,
            "web_search" to PolicyDecision.Allow,
            "web_fetch" to PolicyDecision.Allow,
            "web_browse" to PolicyDecision.Allow,
            "request_tools" to PolicyDecision.Allow,
            "list_files" to PolicyDecision.Allow,
            "read_file" to PolicyDecision.Allow,
            "recall" to PolicyDecision.Allow,
            "list_memories" to PolicyDecision.Allow,
            "reply_notification" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("reply_notification"), "Send a notification reply"),
            "tap_notification" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("tap_notification"), "Interact with a notification"),
            "dismiss_notification" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("dismiss_notification"), "Dismiss a notification"),
            "write_file" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("write_file"), "Write to device storage"),
            "remember" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("remember"), "Store information in memory"),
            "learn" to PolicyDecision.Confirm(PolicyReasonCode.defaultConfirmForTool("learn"), "Learn a new pattern")
        )

        val SENSITIVE_APP_PACKAGES = setOf(
            "com.google.android.apps.messaging", "com.whatsapp", "org.telegram.messenger", "com.discord", "com.slack",
            "com.google.android.gm", "com.microsoft.office.outlook", "com.google.android.dialer",
            "com.chase.sig.android", "com.venmo", "com.paypal.android.p2pmobile", "com.squareup.cash"
        )

        val APP_TARGETED_ACTION_TOOLS = setOf("open_app", "tap", "tap_text", "type_text", "long_press", "swipe", "scroll", "press_back")

        private val FINANCIAL_APP_PACKAGES = setOf(
            "com.chase.sig.android", "com.venmo", "com.paypal.android.p2pmobile", "com.squareup.cash"
        )
        private val FINANCIAL_SUBMIT_KEYWORDS = setOf(
            "send", "confirm", "transfer", "pay", "payment", "submit", "cash out", "withdraw", "approve"
        )
    }

    private val attemptTimestamps = mutableListOf<Long>()
    private val messageTimestamps = mutableListOf<Long>()
    private val seenApps = mutableSetOf<String>()

    @Synchronized
    override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
        val now = clock()
        val firstUseObserved = peekFirstUseSignal(toolCall, context)
        attemptTimestamps.add(now)

        checkHardDeny(toolCall)?.let { return PolicyEvaluation(it, firstUseObserved) }
        checkRateLimit(toolCall, context, now)?.let { return PolicyEvaluation(it, firstUseObserved) }

        val base = DEFAULT_POLICIES[toolCall.name] ?: PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_UNKNOWN_TOOL, "Unknown tool: ${toolCall.name}")
        val firstUse = checkFirstUseAppAction(toolCall, base, context)
        val escalated = escalateForContext(toolCall, firstUse, context)
        val final = applyUserOverrides(toolCall.name, escalated)

        if (isMessageAction(toolCall, context)) messageTimestamps.add(now)
        val reasonCode = when {
            final is PolicyDecision.Allow && isUrlEgressTool(toolCall) -> PolicyReasonCode.ALLOW_EGRESS_ALLOWLISTED
            final is PolicyDecision.Allow -> PolicyReasonCode.ALLOW_DEFAULT
            else -> null
        }
        return PolicyEvaluation(final, firstUseObserved, reasonCode)

    }

    private fun checkHardDeny(toolCall: ToolCall): PolicyDecision.Deny? {
        PHASE1_DENY_TOOLS[toolCall.name]?.let { return PolicyDecision.Deny(PolicyReasonCode.DENY_PHASE1_TOOL, it) }
        if (isUrlEgressTool(toolCall)) {
            return egressDenyDecision(toolCall)
        }
        return null
    }

    private fun checkRateLimit(toolCall: ToolCall, context: PolicyContext, now: Long): PolicyDecision? {
        attemptTimestamps.removeAll { now - it > 60_000 }
        messageTimestamps.removeAll { now - it > MESSAGE_RATE_WINDOW_MS }
        if (attemptTimestamps.size > RATE_LIMIT_PER_MINUTE) return PolicyDecision.RateLimited(PolicyReasonCode.RATE_LIMIT_GLOBAL, "Too many tool attempts", 5_000)
        if (isMessageAction(toolCall, context) && messageTimestamps.size >= MESSAGE_RATE_LIMIT) {
            return PolicyDecision.RateLimited(PolicyReasonCode.RATE_LIMIT_MESSAGES, "Too many messaging attempts", 10_000)
        }
        return null
    }

    private fun checkFirstUseAppAction(toolCall: ToolCall, current: PolicyDecision, context: PolicyContext): PolicyDecision {
        if (toolCall.name !in APP_TARGETED_ACTION_TOOLS) return current
        val appId = resolveAppIdentifier(toolCall, context)
            ?: return PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_MISSING_APP_TARGET, "App-targeted action missing app identifier/foreground context")

        val firstUse = if (appId.startsWith("app_name:")) {
            seenApps.add(appId)
        } else {
            val seenFamily = seenApps.any { ActionPolicyNormalizer.isSamePackageFamily(appId, it) }
            if (!seenFamily) seenApps.add(appId)
            !seenFamily
        }

        return if (firstUse && current is PolicyDecision.Allow) {
            PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_FIRST_USE_APP, "First time acting in '$appId' this session")
        } else current
    }

    private fun escalateForContext(toolCall: ToolCall, current: PolicyDecision, context: PolicyContext): PolicyDecision {
        val interaction = toolCall.name in setOf("tap", "tap_text", "type_text", "long_press")
        val pkg = resolveContextPackage(context)

        if (interaction && isFinancialSubmitAttempt(toolCall, context)) {
            return PolicyDecision.Deny(
                PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT,
                "Possible financial submit action in degraded context is blocked"
            )
        }
        if (interaction && pkg != null && ActionPolicyNormalizer.matchesAnyPackage(pkg, FINANCIAL_APP_PACKAGES) && isSubmitIntent(toolCall)) {
            return PolicyDecision.Deny(
                PolicyReasonCode.DENY_FINANCIAL_SUBMIT,
                "Financial submit actions are blocked in Phase 1"
            )
        }

        if (current !is PolicyDecision.Allow) return current
        if (interaction && pkg == null && looksSensitiveWithoutPackage(context)) {
            return PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_DEGRADED_SENSITIVE, "Foreground app is unknown in sensitive context; confirmation required")
        }
        if (interaction && pkg != null && ActionPolicyNormalizer.matchesAnyPackage(pkg, SENSITIVE_APP_PACKAGES)) {
            return PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_SENSITIVE_APP, "Sensitive app interaction requires confirmation")
        }
        return current
    }


    private fun isFinancialSubmitAttempt(toolCall: ToolCall, context: PolicyContext): Boolean {
        val pkg = resolveContextPackage(context)
        if (pkg != null) return false
        if (!isSubmitIntent(toolCall)) return false
        val blob = "${context.screenContentSummary.orEmpty()} ${context.targetNodeHints.joinToString(" ")}".lowercase()
        return setOf("bank", "wallet", "payment", "transfer", "zelle", "cash", "withdraw", "deposit").any { blob.contains(it) }
    }

    private fun isSubmitIntent(toolCall: ToolCall): Boolean {
        val textTokens = listOf("text", "content_desc", "resource_id", "hint", "label")
            .mapNotNull { toolCall.input[it] as? String }
            .joinToString(" ")
            .lowercase()
        return FINANCIAL_SUBMIT_KEYWORDS.any { textTokens.contains(it) }
    }

    private fun applyUserOverrides(toolName: String, current: PolicyDecision): PolicyDecision {
        return when (store?.getOverride(toolName)) {
            PolicyOverrideLevel.CONFIRM -> if (current is PolicyDecision.Allow) PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_USER_OVERRIDE, "User override: confirmation required") else current
            PolicyOverrideLevel.DENY -> if (current is PolicyDecision.Deny) current else PolicyDecision.Deny(PolicyReasonCode.DENY_USER_OVERRIDE, "User override: action denied")
            null -> current
        }
    }

    private fun isMessageAction(toolCall: ToolCall, context: PolicyContext): Boolean {
        if (toolCall.name == "reply_notification") return true
        if (toolCall.name !in setOf("tap", "tap_text")) return false
        val pkg = resolveContextPackage(context) ?: return false
        val isMessaging = ActionPolicyNormalizer.matchesAnyPackage(pkg, setOf("com.google.android.apps.messaging", "com.whatsapp", "org.telegram.messenger", "com.discord", "com.slack"))
        val text = (toolCall.input["text"] as? String)?.lowercase().orEmpty()
        return isMessaging && (text.contains("send") || text.contains("submit"))
    }

    // URL egress gate is intentionally limited to tools that accept caller-provided URLs.
    // web_search takes only {query,count} (see PhoneTools.WEB_SEARCH schema) and routes through
    // controlled provider clients; there is no model-controlled endpoint field to validate here.
    // Keep fail-closed behavior for arbitrary egress via web_fetch/web_browse URL enforcement.
    private fun isUrlEgressTool(toolCall: ToolCall): Boolean = toolCall.name in setOf("web_fetch", "web_browse")

    private fun egressDenyDecision(toolCall: ToolCall): PolicyDecision.Deny? {
        val url = when (toolCall.name) {
            "web_fetch", "web_browse" -> toolCall.input["url"] as? String
            else -> null
        } ?: return PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_MISSING_URL, "Egress request missing endpoint URL")

        val uri = kotlin.runCatching { java.net.URI(url) }.getOrNull()
            ?: return PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_MALFORMED_URL, "Egress endpoint URL is malformed")

        if (uri.scheme?.lowercase() != "https") {
            return PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_INSECURE_SCHEME, "Only HTTPS egress endpoints are allowed")
        }

        val host = canonicalizeHost(uri.host)
            ?: return PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_MALFORMED_URL, "Egress endpoint host is invalid")

        val snapshot = egressAllowlistProvider.currentSnapshot()
        if (!snapshot.signatureVerified) {
            return PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_UNSIGNED_ALLOWLIST, "Signed egress allowlist unavailable; blocking outbound request")
        }

        val allowed = snapshot.hosts.mapNotNull(::canonicalizeHost)
        val approved = allowed.any { host == it || host.endsWith(".$it") }
        return if (approved) null
        else PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_UNAPPROVED, "Sending data to unrecognized or unapproved endpoints is blocked in Phase 1")
    }

    private fun canonicalizeHost(raw: String?): String? {
        val trimmed = raw?.trim()?.trimEnd('.')?.lowercase()
        if (trimmed.isNullOrBlank()) return null
        return kotlin.runCatching { java.net.IDN.toASCII(trimmed) }.getOrNull()?.lowercase()
    }

    private fun resolveContextPackage(context: PolicyContext): String? {
        val fg = context.foregroundApp?.trim()?.lowercase()
        if (!fg.isNullOrBlank()) return fg
        val app = context.appIdentifier?.trim()?.lowercase() ?: return null
        return if (app.startsWith("app_name:")) null else app
    }

    private fun looksSensitiveWithoutPackage(context: PolicyContext): Boolean {
        val blob = "${context.screenContentSummary.orEmpty()} ${context.targetNodeHints.joinToString(" ")}".lowercase()
        return setOf("send", "message", "reply", "email", "dial", "call", "bank", "wallet", "payment", "transfer").any { blob.contains(it) }
    }

    private fun resolveAppIdentifier(toolCall: ToolCall, context: PolicyContext): String? {
        if (toolCall.name == "open_app") {
            return context.appIdentifier ?: ActionPolicyNormalizer.normalizeAppIdentifier(
                contextAppIdentifier = toolCall.input["app_package"] as? String,
                fallbackDisplayName = toolCall.input["app_name"] as? String
            )
        }
        return resolveContextPackage(context) ?: context.appIdentifier
    }

    private fun peekFirstUseSignal(toolCall: ToolCall, context: PolicyContext): Boolean {
        if (toolCall.name !in APP_TARGETED_ACTION_TOOLS) return false
        val appId = resolveAppIdentifier(toolCall, context) ?: return false
        return if (appId.startsWith("app_name:")) appId !in seenApps
        else seenApps.none { ActionPolicyNormalizer.isSamePackageFamily(appId, it) }
    }
}
