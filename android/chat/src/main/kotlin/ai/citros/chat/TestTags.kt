package ai.citros.chat

import ai.citros.core.OverlayLineType

/** Stable test tags used by Compose tests across chat/overlay surfaces. */
internal const val TEST_TAG_QUICK_SWITCHER_SHEET = "quick_switcher_sheet"
internal const val TEST_TAG_QUICK_SWITCHER_CHIP = "quick_switcher_chip"
internal const val TEST_TAG_QUICK_SWITCHER_HEADER = "quick_switcher_header"
internal const val TEST_TAG_API_KEY_REQUIRED_MODAL = "api_key_required_modal"
internal const val TEST_TAG_MESSAGE_INPUT_FIELD = "message_input_field"
internal const val TEST_TAG_MESSAGE_SEND_BUTTON = "message_send_button"
internal const val TEST_TAG_MESSAGE_STEER_QUEUED_BUTTON = "message_steer_queued_button"
internal const val TEST_TAG_OVERLAY_SYSTEM_LINE = "overlay_system_line"
internal const val TEST_TAG_OVERLAY_USER_LINE = "overlay_user_line"
internal const val TEST_TAG_OVERLAY_QUEUED_LINE = "overlay_queued_line"
internal const val TEST_TAG_ONBOARDING_CONTINUE_WELCOME = "onboarding_continue_welcome"
internal const val TEST_TAG_ONBOARDING_CONTINUE_FLAVOR = "onboarding_continue_flavor"
internal const val TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY = "onboarding_continue_personality"
internal const val TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED = "onboarding_continue_acquainted"
internal const val TEST_TAG_ONBOARDING_CONTINUE_TRUST = "onboarding_continue_trust"
internal const val TEST_TAG_ONBOARDING_BACK_FLAVOR = "onboarding_back_flavor"
internal const val TEST_TAG_ONBOARDING_BACK_PERSONALITY = "onboarding_back_personality"
internal const val TEST_TAG_ONBOARDING_BACK_ACQUAINTED = "onboarding_back_acquainted"
internal const val TEST_TAG_ONBOARDING_BACK_PAYWALL = "onboarding_back_paywall"
internal const val TEST_TAG_ONBOARDING_BACK_API_KEY = "onboarding_back_api_key"
internal const val TEST_TAG_ONBOARDING_BACK_PERMISSIONS = "onboarding_back_permissions"
internal const val TEST_TAG_ONBOARDING_BACK_TRUST = "onboarding_back_trust"

internal fun overlayLineTypeTestTag(type: OverlayLineType): String = when (type) {
    OverlayLineType.SYSTEM -> TEST_TAG_OVERLAY_SYSTEM_LINE
    OverlayLineType.USER -> TEST_TAG_OVERLAY_USER_LINE
    OverlayLineType.QUEUED -> TEST_TAG_OVERLAY_QUEUED_LINE
}

internal fun overlayLineTestTag(type: OverlayLineType): String = overlayLineTypeTestTag(type)

internal fun overlayLineTestTag(type: OverlayLineType, lineId: Number): String =
    "${overlayLineTypeTestTag(type)}_$lineId"
