package ai.citros.core

/**
 * Contextual action pill rendered while the runtime agent is waiting for user input.
 *
 * Shared in :core so service, chat, and overlay can use one deterministic action model.
 */
data class ActionPill(
    val id: String,
    val label: String,
    val icon: Int? = null,
    val style: PillStyle = PillStyle.DEFAULT,
    val action: PillAction
)

enum class PillStyle {
    DEFAULT,
    PRIMARY,
    DANGER,
    SUBTLE
}

sealed class PillAction {
    data class Approve(val requestId: String) : PillAction()
    data class Deny(val requestId: String) : PillAction()
    data class Steer(val message: String) : PillAction()
    data class Authenticate(val requestId: String) : PillAction()
    data object Cancel : PillAction()
    data object Dismiss : PillAction()
}
