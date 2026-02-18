package ai.citros.chat

/**
 * Actions dispatched from the overlay UI (OverlayService) to be handled
 * by the mediator (ChatActivity).
 *
 * This enforces unidirectional data flow:
 * - **Actions flow up:** Overlay UI → [OverlayController.dispatch] → ChatActivity mediator
 * - **State flows down:** ChatViewModel → ChatActivity → [OverlayController] state flows → OverlayService
 *
 * The mediator decides whether an action affects app state (routed to ChatViewModel)
 * or presentation state (applied directly to OverlayController).
 */
sealed class OverlayAction {
    /** User tapped Queue in overlay input while agent is executing. */
    data class QueueMessage(val text: String) : OverlayAction()

    /** User tapped Stop button during tool execution. */
    data object StopExecution : OverlayAction()

    /** User tapped Undo/Resume in overlay after a stop. */
    data object ResumeExecution : OverlayAction()

    /** User tapped Full/Bubble mode button to change overlay surface mode. */
    data class SetSurfaceMode(val mode: OverlaySurfaceMode) : OverlayAction()

    /** User dismissed overlay via bubble long-press or notification Stop button. */
    data object Deactivate : OverlayAction()

    /** User tapped bubble to expand — switches to MINI_CHAT and resets unread count. */
    data object ExpandFromBubble : OverlayAction()
}
