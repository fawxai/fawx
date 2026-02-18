package ai.citros.preview.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
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
import ai.citros.preview.theme.extendedColors

/**
 * Selectable chip for personality/preference options.
 * Unselected: bordered with transparent background. Selected: colored background and border.
 */
@Composable
fun PersonalityOptionChip(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE
) {
    val interactionSource = remember { MutableInteractionSource() }

    val backgroundColor = if (selected) {
        flavor.primary.copy(alpha = 0.1f)
    } else {
        Color.Transparent
    }

    val borderColor = if (selected) {
        flavor.primary
    } else {
        MaterialTheme.extendedColors.border
    }

    val textColor = if (selected) {
        flavor.primary
    } else {
        Color.White
    }

    Box(
        modifier = Modifier
            .border(
                width = 1.dp,
                color = borderColor,
                shape = RoundedCornerShape(999.dp)
            )
            .background(
                color = backgroundColor,
                shape = RoundedCornerShape(999.dp)
            )
            .clickable(
                interactionSource = interactionSource,
                indication = ripple(),
                onClick = onClick
            )
            .padding(horizontal = 20.dp, vertical = 12.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.bodyMedium.copy(
                fontWeight = FontWeight.SemiBold
            ),
            color = textColor
        )
    }
}
