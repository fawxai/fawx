package ai.citros.preview.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ai.citros.preview.theme.CitrosFlavor
import ai.citros.preview.theme.CitrosDimensions

/**
 * Primary button with pill shape, flavor color, and glow shadow.
 * Full-width button with hover feedback and disabled state.
 */
@Composable
fun CitrusPrimaryButton(
    text: String,
    onClick: () -> Unit,
    enabled: Boolean = true,
    modifier: Modifier = Modifier,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE
) {
    val interactionSource = remember { MutableInteractionSource() }
    val backgroundColor = if (enabled) flavor.primary else flavor.primary.copy(alpha = 0.4f)
    val textColor = flavor.tint

    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(56.dp)
            .shadow(
                elevation = if (enabled) 12.dp else 0.dp,
                shape = RoundedCornerShape(999.dp),
                ambientColor = flavor.primary.copy(alpha = 0.2f),
                spotColor = flavor.primary.copy(alpha = 0.2f)
            )
            .background(
                color = backgroundColor,
                shape = RoundedCornerShape(999.dp)
            )
            .clickable(
                interactionSource = interactionSource,
                indication = ripple(),
                enabled = enabled,
                onClick = onClick
            ),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.bodyLarge.copy(
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold
            ),
            color = textColor
        )
    }
}
