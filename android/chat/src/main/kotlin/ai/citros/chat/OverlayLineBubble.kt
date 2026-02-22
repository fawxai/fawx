package ai.citros.chat

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

internal enum class OverlayBubbleHorizontalAnchor {
    START,
    END
}

internal data class OverlayLineBubbleStyle(
    val bubbleColor: Color,
    val bubbleBorder: Color,
    val bubbleTextColor: Color,
    val horizontalAnchor: OverlayBubbleHorizontalAnchor
)

internal fun overlayLineBubbleStyle(
    type: OverlayLineType,
    flavor: CitrosFlavor,
    surfaces: CitrosDirectiveSurfaces,
    flavorTokens: CitrosDirectiveFlavorTokens
): OverlayLineBubbleStyle {
    val bubbleColor = when (type) {
        OverlayLineType.SYSTEM -> surfaces.surface2
        OverlayLineType.USER -> flavorTokens.userBubbleBackground
        OverlayLineType.QUEUED -> flavor.primary.copy(alpha = 0.14f)
    }
    val bubbleBorder = when (type) {
        OverlayLineType.QUEUED -> flavor.primary.copy(alpha = 0.34f)
        else -> surfaces.separatorLight
    }
    val bubbleTextColor = when (type) {
        OverlayLineType.USER -> flavorTokens.userBubbleText
        OverlayLineType.QUEUED -> surfaces.labelSecondary
        OverlayLineType.SYSTEM -> surfaces.labelPrimary
    }
    val horizontalAnchor = if (type == OverlayLineType.USER) {
        OverlayBubbleHorizontalAnchor.END
    } else {
        OverlayBubbleHorizontalAnchor.START
    }
    return OverlayLineBubbleStyle(
        bubbleColor = bubbleColor,
        bubbleBorder = bubbleBorder,
        bubbleTextColor = bubbleTextColor,
        horizontalAnchor = horizontalAnchor
    )
}

@Composable
internal fun OverlayLineBubble(
    line: OverlayLine,
    flavor: CitrosFlavor,
    surfaces: CitrosDirectiveSurfaces,
    flavorTokens: CitrosDirectiveFlavorTokens
) {
    val style = overlayLineBubbleStyle(
        type = line.type,
        flavor = flavor,
        surfaces = surfaces,
        flavorTokens = flavorTokens
    )
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = when (style.horizontalAnchor) {
            OverlayBubbleHorizontalAnchor.END -> Arrangement.End
            OverlayBubbleHorizontalAnchor.START -> Arrangement.Start
        }
    ) {
        Surface(
            modifier = Modifier
                .testTag(overlayLineTestTag(line.type, line.id)),
            shape = RoundedCornerShape(
                topStart = 14.dp,
                topEnd = 14.dp,
                bottomStart = if (line.type == OverlayLineType.SYSTEM) 6.dp else 14.dp,
                bottomEnd = 14.dp
            ),
            color = style.bubbleColor,
            border = BorderStroke(1.dp, style.bubbleBorder)
        ) {
            MarkdownText(
                text = line.text,
                modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
                style = CitrosTypography.bodySmall,
                color = style.bubbleTextColor
            )
        }
    }
}
