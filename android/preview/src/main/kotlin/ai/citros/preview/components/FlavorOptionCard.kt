package ai.citros.preview.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ai.citros.preview.theme.CitrosFlavor
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.extendedColors
import ai.citros.preview.components.CitrusHeroBadge

/**
 * Card for selecting a flavor option.
 * Full-width card with badge, flavor name, selection state indicator.
 */
@Composable
fun FlavorOptionCard(
    flavor: CitrosFlavor,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val interactionSource = remember { MutableInteractionSource() }

    val borderColor = if (selected) {
        flavor.primary.copy(alpha = 0.6f)
    } else {
        MaterialTheme.extendedColors.border
    }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .border(
                width = 1.dp,
                color = borderColor,
                shape = RoundedCornerShape(CitrosDimensions.cardRadius)
            )
            .background(
                color = MaterialTheme.extendedColors.cardSurface,
                shape = RoundedCornerShape(CitrosDimensions.cardRadius)
            )
            .clickable(
                interactionSource = interactionSource,
                indication = ripple(),
                onClick = onClick
            )
            .padding(16.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            CitrusHeroBadge(
                flavor = flavor,
                size = 48
            )

            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 16.dp)
            ) {
                Text(
                    text = flavor.displayName,
                    style = MaterialTheme.typography.titleMedium,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold
                )

                if (selected) {
                    Text(
                        text = "Selected",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.extendedColors.textDim,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
            }
        }
    }
}
