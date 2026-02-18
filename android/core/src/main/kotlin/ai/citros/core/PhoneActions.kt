package ai.citros.core

/** Response from the phone agent — either text or an action to execute. */
data class AgentResponse(
    val text: String?,
    val action: PhoneAction?
)

/** Actions the phone agent can execute via the Accessibility Service. */
sealed class PhoneAction {
    data class Click(val elementId: Int) : PhoneAction()
    data class ClickText(val text: String) : PhoneAction()
    data class Type(val text: String) : PhoneAction()
    data class Swipe(val direction: String) : PhoneAction()
    object Back : PhoneAction()
    object Home : PhoneAction()
    data class OpenApp(val name: String) : PhoneAction()
    object OpenNotifications : PhoneAction()
}
