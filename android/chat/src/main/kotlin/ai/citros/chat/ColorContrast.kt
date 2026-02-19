package ai.citros.chat

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance

internal fun contrastOn(background: Color): Color {
    return if (background.luminance() > 0.5f) Color.Black else Color.White
}
