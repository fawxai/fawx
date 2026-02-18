package ai.citros.chat

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.ui.unit.dp

internal object OverlayUiConstants {
    const val STEP_TICKER_DELAY_MS = 1400L
    const val DISMISS_ANIMATION_DELAY_MS = 200L

    val BubbleSize = 58.dp
    val BubbleProgressSize = 48.dp
    val BubbleBadgeSize = 18.dp
    val HeaderStatusDotSize = 8.dp

    val StandardChipPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp)
    val CompactChipPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp)
    val ActionChipPadding = PaddingValues(horizontal = 9.dp, vertical = 5.dp)
    val CompactActionPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp)
    val PrimaryActionPadding = PaddingValues(horizontal = 12.dp, vertical = 10.dp)

    val PreviewCornerRadius = 22.dp
    val ControlPanelCornerRadius = 16.dp
    val MiniChatCornerRadius = 22.dp
    val StandardCardCornerRadius = 14.dp
    val MiniChatMaxHeight = 340.dp
    val BubbleQuickActionsWidth = 150.dp

    val PillCornerRadius = 999.dp
    val ModeChipCornerRadius = 10.dp
    val PhoneItemCornerRadius = 6.dp
    val ErrorCardCornerRadius = 12.dp
}
