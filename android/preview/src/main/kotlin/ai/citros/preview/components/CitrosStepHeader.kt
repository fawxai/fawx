package ai.citros.preview.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronLeft
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ai.citros.preview.theme.CitrosDimensions
import ai.citros.preview.theme.extendedColors
import androidx.compose.foundation.layout.padding

/**
 * Step header with progress indicator and optional back button.
 * Displays step title, current/total step counter, and visual progress bars.
 */
@Composable
fun CitrosStepHeader(
    title: String,
    stepIndex: Int,
    totalSteps: Int,
    onBack: (() -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier.fillMaxWidth()
    ) {
        // Header row with back button, title, and step counter
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(56.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (onBack != null) {
                IconButton(
                    onClick = onBack,
                    modifier = Modifier.size(40.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.ChevronLeft,
                        contentDescription = "Back",
                        tint = Color.White,
                        modifier = Modifier.size(24.dp)
                    )
                }
            } else {
                Spacer(modifier = Modifier.width(40.dp))
            }

            Text(
                text = title,
                style = MaterialTheme.typography.headlineSmall,
                color = Color.White,
                modifier = Modifier.weight(1f)
            )

            Text(
                text = "${stepIndex + 1}/$totalSteps",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.extendedColors.textDim,
                modifier = Modifier.padding(end = 16.dp)
            )
        }

        // Progress bars
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(4.dp)
        ) {
            repeat(totalSteps) { index ->
                val progressColor = when {
                    index < stepIndex -> MaterialTheme.colorScheme.primary
                    index == stepIndex -> MaterialTheme.colorScheme.primary.copy(alpha = 0.6f)
                    else -> MaterialTheme.extendedColors.border
                }

                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(4.dp)
                        .background(
                            color = progressColor,
                            shape = RoundedCornerShape(2.dp)
                        )
                )

                if (index < totalSteps - 1) {
                    Spacer(modifier = Modifier.width(4.dp))
                }
            }
        }
    }
}
