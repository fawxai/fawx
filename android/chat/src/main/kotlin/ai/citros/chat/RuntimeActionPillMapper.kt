package ai.citros.chat

import ai.citros.core.ActionPill
import ai.citros.core.PillAction
import ai.citros.core.PillStyle
import ai.citros.core.PolicyReasonCode

/**
 * Runtime pill mapping from agent state/context to deterministic pill actions.
 *
 * Mapping source: docs/specs/h2-action-policy-engine.md §8a.
 */
internal object RuntimeActionPillMapper {

    internal enum class PolicyConfirmKind {
        STANDARD,
        SENSITIVE_APP,
        FINANCIAL_CONTEXT,
        FIRST_USE_APP
    }

    fun classifyPolicyConfirmation(reason: String, reasonCode: String? = null): PolicyConfirmKind {
        val normalizedReasonCode = reasonCode?.trim()?.lowercase()
        if (normalizedReasonCode != null) {
            return when (normalizedReasonCode) {
                PolicyReasonCode.CONFIRM_FIRST_USE_APP -> PolicyConfirmKind.FIRST_USE_APP
                PolicyReasonCode.CONFIRM_SENSITIVE_APP,
                PolicyReasonCode.CONFIRM_DEGRADED_SENSITIVE -> PolicyConfirmKind.SENSITIVE_APP
                else -> {
                    // Fall through to reason text classification for compatibility
                    // with legacy reason strings and custom policy implementations.
                    classifyPolicyConfirmation(reason = reason, reasonCode = null)
                }
            }
        }

        val normalized = reason.trim().lowercase()
        return when {
            "confirm.first_use_app" in normalized || "first time acting" in normalized -> PolicyConfirmKind.FIRST_USE_APP
            "financial" in normalized || "biometric" in normalized || "authenticate" in normalized ->
                PolicyConfirmKind.FINANCIAL_CONTEXT
            "confirm.sensitive_app_interaction" in normalized ||
                "confirm.degraded_context_sensitive" in normalized ||
                "sensitive app" in normalized ||
                "sensitive context" in normalized -> PolicyConfirmKind.SENSITIVE_APP
            else -> PolicyConfirmKind.STANDARD
        }
    }

    fun policyConfirmationPills(
        requestId: String,
        reason: String,
        reasonCode: String? = null
    ): List<ActionPill> {
        return when (classifyPolicyConfirmation(reason = reason, reasonCode = reasonCode)) {
            PolicyConfirmKind.STANDARD -> listOf(
                ActionPill(
                    id = "confirm_yes",
                    label = "Yes",
                    style = PillStyle.PRIMARY,
                    action = PillAction.Approve(requestId)
                ),
                ActionPill(
                    id = "confirm_no",
                    label = "No",
                    style = PillStyle.DANGER,
                    action = PillAction.Deny(requestId)
                ),
                ActionPill(
                    id = "confirm_other",
                    label = "Do something else",
                    style = PillStyle.SUBTLE,
                    action = PillAction.Steer("Try a different approach.")
                )
            )

            PolicyConfirmKind.SENSITIVE_APP -> listOf(
                ActionPill(
                    id = "sensitive_allow_once",
                    label = "Allow once",
                    style = PillStyle.PRIMARY,
                    action = PillAction.Approve(requestId)
                ),
                ActionPill(
                    id = "sensitive_deny",
                    label = "Deny",
                    style = PillStyle.DANGER,
                    action = PillAction.Deny(requestId)
                ),
                ActionPill(
                    id = "sensitive_always_deny",
                    label = "Always deny for this app",
                    style = PillStyle.SUBTLE,
                    action = PillAction.Deny(requestId)
                )
            )

            PolicyConfirmKind.FINANCIAL_CONTEXT -> listOf(
                ActionPill(
                    id = "financial_authenticate_allow",
                    label = "Authenticate & allow",
                    style = PillStyle.PRIMARY,
                    action = PillAction.Authenticate(requestId)
                ),
                ActionPill(
                    id = "financial_deny",
                    label = "Deny",
                    style = PillStyle.DANGER,
                    action = PillAction.Deny(requestId)
                )
            )

            PolicyConfirmKind.FIRST_USE_APP -> listOf(
                ActionPill(
                    id = "first_use_continue",
                    label = "Continue",
                    style = PillStyle.PRIMARY,
                    action = PillAction.Approve(requestId)
                ),
                ActionPill(
                    id = "first_use_not_now",
                    label = "Not now",
                    style = PillStyle.DEFAULT,
                    action = PillAction.Deny(requestId)
                ),
                ActionPill(
                    id = "first_use_never",
                    label = "Never for this app",
                    style = PillStyle.SUBTLE,
                    action = PillAction.Deny(requestId)
                )
            )
        }
    }

    fun offerChoicePills(choices: List<String>): List<ActionPill> {
        return choices.mapIndexed { index, choice ->
            ActionPill(
                id = "choice_$index",
                label = choice,
                style = PillStyle.DEFAULT,
                action = PillAction.Steer(choice)
            )
        }
    }

    fun errorRecoveryPills(): List<ActionPill> = listOf(
        ActionPill(
            id = "error_retry",
            label = "Try again",
            style = PillStyle.DEFAULT,
            action = PillAction.Steer("Try again.")
        ),
        ActionPill(
            id = "error_other",
            label = "Do something else",
            style = PillStyle.DEFAULT,
            action = PillAction.Steer("Try a different approach.")
        ),
        ActionPill(
            id = "error_cancel",
            label = "Cancel",
            style = PillStyle.DANGER,
            action = PillAction.Cancel
        )
    )
}
